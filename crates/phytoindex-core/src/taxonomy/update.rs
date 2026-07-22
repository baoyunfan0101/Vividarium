use std::io::Cursor;

use rusqlite::hooks::Action;
use rusqlite::session::{ConflictAction, ConflictType, Session, invert_strm};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::page::{
    TaxonomyCursor, TaxonomyPage, decode_cursor, encode_cursor, invalid_cursor, page_limit,
};
use super::view::{TaxonSummary, load_taxon_summaries, load_taxon_summary};
use crate::mapping;
use crate::{CoreError, CoreResult, Database};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaxonRank {
    Kingdom,
    Order,
    Family,
    Genus,
    Species,
}

impl TaxonRank {
    const ALL: [Self; 5] = [
        Self::Kingdom,
        Self::Order,
        Self::Family,
        Self::Genus,
        Self::Species,
    ];

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Kingdom => "kingdom",
            Self::Order => "order",
            Self::Family => "family",
            Self::Genus => "genus",
            Self::Species => "species",
        }
    }

    pub(crate) fn code(self) -> i64 {
        match self {
            Self::Kingdom => 1,
            Self::Order => 2,
            Self::Family => 3,
            Self::Genus => 4,
            Self::Species => 5,
        }
    }

    pub(crate) fn from_code(value: i64) -> CoreResult<Self> {
        match value {
            1 => Ok(Self::Kingdom),
            2 => Ok(Self::Order),
            3 => Ok(Self::Family),
            4 => Ok(Self::Genus),
            5 => Ok(Self::Species),
            _ => Err(CoreError::InvalidArgument(format!(
                "invalid taxon rank code: {value}"
            ))),
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Kingdom => 0,
            Self::Order => 1,
            Self::Family => 2,
            Self::Genus => 3,
            Self::Species => 4,
        }
    }

    fn parent(self) -> Option<Self> {
        match self {
            Self::Kingdom => None,
            Self::Order => Some(Self::Kingdom),
            Self::Family => Some(Self::Order),
            Self::Genus => Some(Self::Family),
            Self::Species => Some(Self::Genus),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonNameInput {
    pub name: String,
    pub is_accepted: Option<bool>,
    pub authority_year: Option<String>,
    pub category: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonInputRow {
    pub selected_taxon_id: Option<i64>,
    pub kingdom: Option<String>,
    pub order: Option<String>,
    pub family: Option<String>,
    pub genus: Option<String>,
    pub species: Option<String>,
    pub geological_range: Option<String>,
    pub scientific: Option<TaxonNameInput>,
    pub english: Option<TaxonNameInput>,
    pub chinese: Option<TaxonNameInput>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TaxonUpdateOptions {
    pub allow_new_names: bool,
    pub allow_new_taxa: bool,
    pub allow_overwrite: bool,
    pub allow_switch_accepted_name: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaxonRowStatus {
    Ready,
    Applied,
    NoChange,
    NotFound,
    Ambiguous,
    Conflict,
    Invalid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaxonChangeKind {
    CreateTaxon,
    AppendName,
    Supplement,
    Overwrite,
    ChangeAcceptedName,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonChange {
    pub kind: TaxonChangeKind,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaxonomyNameKind {
    Scientific,
    English,
    Chinese,
}

impl TaxonomyNameKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Scientific => "scientific",
            Self::English => "english",
            Self::Chinese => "chinese",
        }
    }

    pub(crate) fn code(self) -> i64 {
        match self {
            Self::Scientific => 1,
            Self::English => 2,
            Self::Chinese => 3,
        }
    }

    pub(crate) fn from_code(value: i64) -> CoreResult<Self> {
        match value {
            1 => Ok(Self::Scientific),
            2 => Ok(Self::English),
            3 => Ok(Self::Chinese),
            _ => Err(CoreError::InvalidArgument(format!(
                "invalid taxonomy name kind code: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonRowOutcome {
    pub row_number: usize,
    pub operation_id: Option<i64>,
    pub status: TaxonRowStatus,
    pub message: String,
    pub target: Option<TaxonSummary>,
    pub candidates: Vec<TaxonSummary>,
    pub changes: Vec<TaxonChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonBatchResult {
    pub batch_id: Option<i64>,
    pub rows: Vec<TaxonRowOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyCustomSqlTempTable {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyCustomSqlTempTableMetadata {
    pub columns: Vec<String>,
    pub row_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum TaxonomyBatchContext {
    BatchUpdate {
        options: TaxonUpdateOptions,
    },
    QueryUpdate {
        options: TaxonUpdateOptions,
    },
    QueryDeleteName,
    QueryDeleteTaxon,
    CustomSql {
        input: Option<TaxonomyCustomSqlTempTableMetadata>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaxonomyOperationStatus {
    Applied,
    Reverted,
}

impl TaxonomyOperationStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Reverted => "reverted",
        }
    }

    fn from_str(value: &str) -> CoreResult<Self> {
        match value {
            "applied" => Ok(Self::Applied),
            "reverted" => Ok(Self::Reverted),
            _ => Err(CoreError::InvalidArgument(format!(
                "invalid taxonomy operation status: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyOperationBatch {
    pub batch_id: i64,
    pub context: TaxonomyBatchContext,
    pub input: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyOperation {
    pub operation_id: i64,
    pub batch_id: i64,
    pub row_number: usize,
    pub status: TaxonomyOperationStatus,
    pub changeset_size: usize,
    pub applied_at: String,
    pub reverted_at: Option<String>,
}

pub fn preview_rows(
    database: &Database,
    rows: &[TaxonInputRow],
    options: TaxonUpdateOptions,
) -> CoreResult<TaxonBatchResult> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut outcomes = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let outcome = match prepare_row(&transaction, row, options) {
            Ok(plan) => execute_plan(&transaction, index + 1, plan)?,
            Err(issue) => issue_outcome(index + 1, issue),
        };
        outcomes.push(outcome);
    }
    transaction.rollback()?;
    Ok(TaxonBatchResult {
        batch_id: None,
        rows: outcomes,
    })
}

pub fn apply_rows(
    database: &Database,
    rows: &[TaxonInputRow],
    options: TaxonUpdateOptions,
) -> CoreResult<TaxonBatchResult> {
    let mut connection = database.connect()?;
    let mut batch_id = None;
    let mut outcomes = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let outcome = apply_taxon_row_with_log(
            &mut connection,
            index + 1,
            row,
            options,
            &mut batch_id,
            rows,
            &TaxonomyBatchContext::BatchUpdate { options },
        )?;
        outcomes.push(outcome);
    }
    if outcomes
        .iter()
        .any(|outcome| outcome.status == TaxonRowStatus::Applied)
    {
        mapping::refresh_after_taxonomy_change(database)?;
    }
    Ok(TaxonBatchResult {
        batch_id,
        rows: outcomes,
    })
}

pub(super) fn apply_taxon_row_with_log<T: Serialize + ?Sized>(
    connection: &mut Connection,
    row_number: usize,
    row: &TaxonInputRow,
    options: TaxonUpdateOptions,
    batch_id: &mut Option<i64>,
    batch_inputs: &T,
    batch_context: &TaxonomyBatchContext,
) -> CoreResult<TaxonRowOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let plan = match prepare_row(&transaction, row, options) {
        Ok(plan) => plan,
        Err(issue) => {
            transaction.rollback()?;
            return Ok(issue_outcome(row_number, issue));
        }
    };
    apply_prepared_taxon_plan_with_log(
        transaction,
        row_number,
        plan,
        batch_id,
        batch_inputs,
        batch_context,
    )
}

pub(super) fn apply_existing_taxon_update_with_log<T: Serialize + ?Sized>(
    connection: &mut Connection,
    row_number: usize,
    taxon_id: i64,
    update: ExistingTaxonUpdate<'_>,
    options: TaxonUpdateOptions,
    batch_id: &mut Option<i64>,
    batch_inputs: &T,
    batch_context: &TaxonomyBatchContext,
) -> CoreResult<TaxonRowOutcome> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let plan = match prepare_existing_taxon(&transaction, update, options, taxon_id) {
        Ok(plan) => plan,
        Err(issue) => {
            transaction.rollback()?;
            return Ok(issue_outcome(row_number, issue));
        }
    };
    apply_prepared_taxon_plan_with_log(
        transaction,
        row_number,
        plan,
        batch_id,
        batch_inputs,
        batch_context,
    )
}

fn apply_prepared_taxon_plan_with_log<T: Serialize + ?Sized>(
    transaction: Transaction<'_>,
    row_number: usize,
    plan: RowPlan,
    batch_id: &mut Option<i64>,
    batch_inputs: &T,
    batch_context: &TaxonomyBatchContext,
) -> CoreResult<TaxonRowOutcome> {
    let mut session = start_taxonomy_session(&transaction)?;
    let mut outcome = execute_plan(&transaction, row_number, plan)?;
    if outcome.status == TaxonRowStatus::NoChange {
        drop(session);
        transaction.rollback()?;
        return Ok(outcome);
    }
    let taxon_id = outcome
        .target
        .as_ref()
        .map(|target| target.taxon_id)
        .ok_or_else(|| CoreError::InvalidArgument("applied operation has no target".into()))?;
    ensure_taxon_exists_in_connection(&transaction, taxon_id)?;
    let changeset_blob = finish_taxonomy_session(&mut session)?;
    drop(session);
    let current_batch_id = match *batch_id {
        Some(value) => value,
        None => {
            let value = insert_operation_batch(&transaction, batch_inputs, batch_context)?;
            *batch_id = Some(value);
            value
        }
    };
    let operation_id =
        insert_operation_log(&transaction, current_batch_id, row_number, &changeset_blob)?;
    transaction.commit()?;
    outcome.operation_id = Some(operation_id);
    outcome.status = TaxonRowStatus::Applied;
    outcome.message = "applied".into();
    Ok(outcome)
}

pub fn list_taxonomy_operations(
    database: &Database,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<TaxonomyPage<TaxonomyOperation>> {
    let connection = database.connect()?;
    let cursor_operation_id = match decode_cursor(cursor)? {
        None => None,
        Some(TaxonomyCursor::Operations { operation_id }) => Some(operation_id),
        Some(_) => return Err(invalid_cursor()),
    };
    let limit = page_limit(limit);
    let fetch_limit = limit + 1;
    let mut items = if let Some(cursor_operation_id) = cursor_operation_id {
        let mut statement = connection.prepare(
            r#"
            SELECT operation_id, batch_id, row_number, status, length(changeset_blob),
                   applied_at, reverted_at
            FROM taxonomy_operations
            WHERE operation_id < ?1
            ORDER BY operation_id DESC
            LIMIT ?2
            "#,
        )?;
        let rows = statement.query_map(
            params![cursor_operation_id, fetch_limit as i64],
            taxonomy_operation_row,
        )?;
        rows.map(taxonomy_operation_from_row)
            .collect::<CoreResult<Vec<_>>>()?
    } else {
        let mut statement = connection.prepare(
            r#"
            SELECT operation_id, batch_id, row_number, status, length(changeset_blob),
                   applied_at, reverted_at
            FROM taxonomy_operations
            ORDER BY operation_id DESC
            LIMIT ?1
            "#,
        )?;
        let rows = statement.query_map([fetch_limit as i64], taxonomy_operation_row)?;
        rows.map(taxonomy_operation_from_row)
            .collect::<CoreResult<Vec<_>>>()?
    };
    let next_cursor = if items.len() > limit {
        items.truncate(limit);
        items.last().map(|operation| {
            encode_cursor(&TaxonomyCursor::Operations {
                operation_id: operation.operation_id,
            })
        })
    } else {
        None
    }
    .transpose()?;
    Ok(TaxonomyPage { items, next_cursor })
}

pub fn list_taxonomy_operation_batches(
    database: &Database,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<TaxonomyPage<TaxonomyOperationBatch>> {
    let connection = database.connect()?;
    let batch_cursor = match decode_cursor(cursor)? {
        None => None,
        Some(TaxonomyCursor::OperationBatches {
            created_at,
            batch_id,
        }) => Some((created_at, batch_id)),
        Some(_) => return Err(invalid_cursor()),
    };
    let limit = page_limit(limit);
    let fetch_limit = limit + 1;
    let mut items = if let Some((cursor_created_at, cursor_batch_id)) = batch_cursor {
        let mut statement = connection.prepare(
            r#"
            SELECT batch_id, context_json, input_json, created_at
            FROM taxonomy_operation_batches
            WHERE (created_at, batch_id) < (?1, ?2)
            ORDER BY created_at DESC, batch_id DESC
            LIMIT ?3
            "#,
        )?;
        let rows = statement.query_map(
            params![cursor_created_at, cursor_batch_id, fetch_limit as i64],
            taxonomy_operation_batch_row,
        )?;
        rows.map(taxonomy_operation_batch_from_row)
            .collect::<CoreResult<Vec<_>>>()?
    } else {
        let mut statement = connection.prepare(
            r#"
            SELECT batch_id, context_json, input_json, created_at
            FROM taxonomy_operation_batches
            ORDER BY created_at DESC, batch_id DESC
            LIMIT ?1
            "#,
        )?;
        let rows = statement.query_map([fetch_limit as i64], taxonomy_operation_batch_row)?;
        rows.map(taxonomy_operation_batch_from_row)
            .collect::<CoreResult<Vec<_>>>()?
    };
    let next_cursor = if items.len() > limit {
        items.truncate(limit);
        items.last().map(|batch| {
            encode_cursor(&TaxonomyCursor::OperationBatches {
                created_at: batch.created_at.clone(),
                batch_id: batch.batch_id,
            })
        })
    } else {
        None
    }
    .transpose()?;
    Ok(TaxonomyPage { items, next_cursor })
}

pub fn list_taxonomy_operations_for_batch(
    database: &Database,
    batch_id: i64,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<TaxonomyPage<TaxonomyOperation>> {
    let connection = database.connect()?;
    list_taxonomy_operations_for_batch_from_connection(&connection, batch_id, cursor, limit)
}

fn taxonomy_operation_batch_from_row(
    row: rusqlite::Result<(i64, String, String, String)>,
) -> CoreResult<TaxonomyOperationBatch> {
    let (batch_id, context_json, input_json, created_at) = row?;
    Ok(TaxonomyOperationBatch {
        batch_id,
        context: deserialize_json(&context_json, "batch context")?,
        input: deserialize_json(&input_json, "batch input")?,
        created_at,
    })
}

fn taxonomy_operation_batch_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(i64, String, String, String)> {
    Ok((
        row.get::<_, i64>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
    ))
}

fn list_taxonomy_operations_for_batch_from_connection(
    connection: &Connection,
    batch_id: i64,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<TaxonomyPage<TaxonomyOperation>> {
    let operation_cursor = match decode_cursor(cursor)? {
        None => None,
        Some(TaxonomyCursor::BatchOperations {
            batch_id: cursor_batch_id,
            row_number,
            operation_id,
        }) if cursor_batch_id == batch_id => Some((row_number as i64, operation_id)),
        Some(_) => return Err(invalid_cursor()),
    };
    let limit = page_limit(limit);
    let fetch_limit = limit + 1;
    let mut items = if let Some((cursor_row_number, cursor_operation_id)) = operation_cursor {
        let mut statement = connection.prepare(
            r#"
            SELECT operation_id, batch_id, row_number, status, length(changeset_blob),
                   applied_at, reverted_at
            FROM taxonomy_operations
            WHERE batch_id = ?1 AND (row_number, operation_id) > (?2, ?3)
            ORDER BY row_number, operation_id
            LIMIT ?4
            "#,
        )?;
        let rows = statement.query_map(
            params![
                batch_id,
                cursor_row_number,
                cursor_operation_id,
                fetch_limit as i64
            ],
            taxonomy_operation_row,
        )?;
        rows.map(taxonomy_operation_from_row)
            .collect::<CoreResult<Vec<_>>>()?
    } else {
        let mut statement = connection.prepare(
            r#"
            SELECT operation_id, batch_id, row_number, status, length(changeset_blob),
                   applied_at, reverted_at
            FROM taxonomy_operations
            WHERE batch_id = ?1
            ORDER BY row_number, operation_id
            LIMIT ?2
            "#,
        )?;
        let rows = statement.query_map(
            params![batch_id, fetch_limit as i64],
            taxonomy_operation_row,
        )?;
        rows.map(taxonomy_operation_from_row)
            .collect::<CoreResult<Vec<_>>>()?
    };
    let next_cursor = if items.len() > limit {
        items.truncate(limit);
        items.last().map(|operation| {
            encode_cursor(&TaxonomyCursor::BatchOperations {
                batch_id,
                row_number: operation.row_number,
                operation_id: operation.operation_id,
            })
        })
    } else {
        None
    }
    .transpose()?;
    Ok(TaxonomyPage { items, next_cursor })
}

fn taxonomy_operation_from_row(
    row: rusqlite::Result<(i64, i64, i64, String, i64, String, Option<String>)>,
) -> CoreResult<TaxonomyOperation> {
    let (operation_id, batch_id, row_number, status, changeset_size, applied_at, reverted_at) =
        row?;
    Ok(TaxonomyOperation {
        operation_id,
        batch_id,
        row_number: row_number as usize,
        status: TaxonomyOperationStatus::from_str(&status)?,
        changeset_size: changeset_size as usize,
        applied_at,
        reverted_at,
    })
}

fn taxonomy_operation_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(i64, i64, i64, String, i64, String, Option<String>)> {
    Ok((
        row.get::<_, i64>(0)?,
        row.get::<_, i64>(1)?,
        row.get::<_, i64>(2)?,
        row.get::<_, String>(3)?,
        row.get::<_, i64>(4)?,
        row.get::<_, String>(5)?,
        row.get::<_, Option<String>>(6)?,
    ))
}

pub fn revert_taxonomy_operation(database: &Database, operation_id: i64) -> CoreResult<()> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (status, changeset_blob): (String, Vec<u8>) = transaction
        .query_row(
            r#"
            SELECT status, changeset_blob
            FROM taxonomy_operations
            WHERE operation_id = ?
            "#,
            [operation_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound(format!("taxonomy operation {operation_id}")))?;
    let status = TaxonomyOperationStatus::from_str(&status)?;
    if status != TaxonomyOperationStatus::Applied {
        return Err(CoreError::InvalidArgument(format!(
            "taxonomy operation {operation_id} is already {}",
            status.as_str()
        )));
    }
    let mut inverted = Vec::new();
    invert_strm(&mut Cursor::new(changeset_blob), &mut inverted)?;
    transaction.apply_strm(
        &mut Cursor::new(inverted),
        Some(is_taxonomy_session_table),
        |conflict_type, item| match item.op() {
            Ok(operation)
                if conflict_type == ConflictType::SQLITE_CHANGESET_NOTFOUND
                    && operation.code() == Action::SQLITE_DELETE =>
            {
                ConflictAction::SQLITE_CHANGESET_OMIT
            }
            _ => ConflictAction::SQLITE_CHANGESET_ABORT,
        },
    )?;
    validate_taxonomy(&transaction)?;
    transaction.execute(
        r#"
        UPDATE taxonomy_operations
        SET status = 'reverted', reverted_at = CURRENT_TIMESTAMP
        WHERE operation_id = ?
        "#,
        [operation_id],
    )?;
    transaction.commit()?;
    mapping::refresh_after_taxonomy_change(database)?;
    Ok(())
}

fn issue_outcome(row_number: usize, issue: RowIssue) -> TaxonRowOutcome {
    TaxonRowOutcome {
        row_number,
        operation_id: None,
        status: issue.status,
        message: issue.message,
        target: None,
        candidates: issue.candidates,
        changes: Vec::new(),
    }
}

#[derive(Debug)]
struct RowIssue {
    status: TaxonRowStatus,
    message: String,
    candidates: Vec<TaxonSummary>,
}

impl RowIssue {
    fn new(status: TaxonRowStatus, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            candidates: Vec::new(),
        }
    }

    fn ambiguous(message: impl Into<String>, candidates: Vec<TaxonSummary>) -> Self {
        Self {
            status: TaxonRowStatus::Ambiguous,
            message: message.into(),
            candidates,
        }
    }
}

#[derive(Debug, Clone)]
struct NormalizedPath {
    values: [Option<String>; 5],
}

impl NormalizedPath {
    fn from_row(row: &TaxonInputRow) -> Self {
        Self {
            values: [
                normalize_name(row.kingdom.as_deref()),
                normalize_name(row.order.as_deref()),
                normalize_name(row.family.as_deref()),
                normalize_name(row.genus.as_deref()),
                normalize_name(row.species.as_deref()),
            ],
        }
    }

    fn get(&self, rank: TaxonRank) -> Option<&str> {
        self.values[rank.index()].as_deref()
    }

    fn set(&mut self, rank: TaxonRank, value: String) {
        self.values[rank.index()] = Some(value);
    }

    fn deepest(&self) -> Option<(TaxonRank, &str)> {
        TaxonRank::ALL
            .into_iter()
            .rev()
            .find_map(|rank| self.get(rank).map(|name| (rank, name)))
    }
}

#[derive(Debug)]
struct RowPlan {
    target: PlannedTarget,
    geological_range: Option<FieldUpdate>,
    names: Vec<NamePlan>,
    changes: Vec<TaxonChange>,
}

#[derive(Debug)]
enum PlannedTarget {
    Existing(i64),
    New {
        rank: TaxonRank,
        parent_taxon_id: Option<i64>,
        geological_range: Option<String>,
    },
}

#[derive(Debug)]
struct FieldUpdate {
    value: String,
}

#[derive(Debug)]
struct NamePlan {
    kind: TaxonomyNameKind,
    name: String,
    insert: Option<NameRecord>,
    updates: Vec<NameFieldUpdate>,
    demote_accepted: Option<String>,
    promote: bool,
}

#[derive(Debug, Clone)]
struct NameRecord {
    is_accepted: bool,
    authority_year: Option<String>,
    category: Option<String>,
    source: Option<String>,
}

#[derive(Debug)]
struct NameFieldUpdate {
    field: NameField,
    value: String,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ExistingTaxonUpdate<'a> {
    geological_range: Option<&'a str>,
    scientific: Option<&'a TaxonNameInput>,
    english: Option<&'a TaxonNameInput>,
    chinese: Option<&'a TaxonNameInput>,
}

impl<'a> ExistingTaxonUpdate<'a> {
    fn from_row(row: &'a TaxonInputRow) -> Self {
        Self::new(
            row.geological_range.as_deref(),
            row.scientific.as_ref(),
            row.english.as_ref(),
            row.chinese.as_ref(),
        )
    }

    pub(super) fn new(
        geological_range: Option<&'a str>,
        scientific: Option<&'a TaxonNameInput>,
        english: Option<&'a TaxonNameInput>,
        chinese: Option<&'a TaxonNameInput>,
    ) -> Self {
        Self {
            geological_range,
            scientific,
            english,
            chinese,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum NameField {
    AuthorityYear,
    Category,
    Source,
}

impl NameField {
    fn as_str(self) -> &'static str {
        match self {
            Self::AuthorityYear => "authority_year",
            Self::Category => "category",
            Self::Source => "source",
        }
    }
}

fn prepare_row(
    transaction: &Transaction<'_>,
    row: &TaxonInputRow,
    options: TaxonUpdateOptions,
) -> Result<RowPlan, RowIssue> {
    let mut path = NormalizedPath::from_row(row);
    let (target_rank, target_name) = path.deepest().ok_or_else(|| {
        RowIssue::new(
            TaxonRowStatus::Invalid,
            "at least one rank scientific name is required",
        )
    })?;
    let target_name = target_name.to_string();
    validate_required_parent(target_rank, &target_name, &mut path, options.allow_new_taxa)?;
    let search =
        find_candidates(transaction, target_rank, &target_name, &path).map_err(core_issue)?;
    let had_name_match = search.had_name_match;
    let mut candidates = search.candidates;
    if let Some(selected) = row.selected_taxon_id {
        if let Some(position) = candidates
            .iter()
            .position(|candidate| candidate.taxon_id == selected)
        {
            candidates = vec![candidates.swap_remove(position)];
        } else if !candidates.is_empty() {
            return Err(RowIssue::new(
                TaxonRowStatus::Invalid,
                "selected_taxon_id is not one of the matching candidates",
            ));
        }
    }
    match candidates.len() {
        0 if row.selected_taxon_id.is_some() => Err(RowIssue::new(
            TaxonRowStatus::Invalid,
            "selected_taxon_id does not match the row locator",
        )),
        0 if had_name_match => Err(RowIssue::new(
            TaxonRowStatus::Conflict,
            format!(
                "{} '{}' exists, but its lineage does not match the coarse rank filters",
                target_rank.as_str(),
                target_name
            ),
        )),
        0 if !options.allow_new_taxa => Err(RowIssue::new(
            TaxonRowStatus::NotFound,
            format!("{} '{}' was not found", target_rank.as_str(), target_name),
        )),
        0 => prepare_new_taxon(transaction, row, options, path, target_rank, target_name),
        1 => prepare_existing_taxon(
            transaction,
            ExistingTaxonUpdate::from_row(row),
            options,
            candidates[0].taxon_id,
        ),
        _ => Err(RowIssue::ambiguous(
            format!(
                "{} '{}' matched multiple taxa",
                target_rank.as_str(),
                target_name
            ),
            candidates,
        )),
    }
}

fn validate_required_parent(
    target_rank: TaxonRank,
    target_name: &str,
    path: &mut NormalizedPath,
    allow_new_taxa: bool,
) -> Result<(), RowIssue> {
    if !allow_new_taxa {
        return Ok(());
    }
    let Some(parent_rank) = target_rank.parent() else {
        return Ok(());
    };
    if path.get(parent_rank).is_some() {
        return Ok(());
    }
    if target_rank != TaxonRank::Species {
        return Err(RowIssue::new(
            TaxonRowStatus::Invalid,
            format!(
                "{} requires its immediate parent {} scientific name",
                target_rank.as_str(),
                parent_rank.as_str()
            ),
        ));
    }
    let mut words = target_name.split_whitespace();
    let genus = words.next().unwrap_or_default();
    if genus.is_empty() || words.next().is_none() {
        return Err(RowIssue::new(
            TaxonRowStatus::Invalid,
            "species without an explicit genus must use a binomial scientific name",
        ));
    }
    path.set(TaxonRank::Genus, genus.to_string());
    Ok(())
}

fn prepare_new_taxon(
    transaction: &Transaction<'_>,
    row: &TaxonInputRow,
    _options: TaxonUpdateOptions,
    path: NormalizedPath,
    target_rank: TaxonRank,
    target_name: String,
) -> Result<RowPlan, RowIssue> {
    let parent_taxon_id = if let Some(parent_rank) = target_rank.parent() {
        let parent_name = path.get(parent_rank).ok_or_else(|| {
            RowIssue::new(
                TaxonRowStatus::Invalid,
                "the immediate parent scientific name is required",
            )
        })?;
        let parent_search =
            find_candidates(transaction, parent_rank, parent_name, &path).map_err(core_issue)?;
        let parents = parent_search.candidates;
        match parents.len() {
            0 if parent_search.had_name_match => {
                return Err(RowIssue::new(
                    TaxonRowStatus::Conflict,
                    format!(
                        "parent {} '{}' exists, but its lineage does not match the coarse rank filters",
                        parent_rank.as_str(),
                        parent_name
                    ),
                ));
            }
            0 => {
                return Err(RowIssue::new(
                    TaxonRowStatus::NotFound,
                    format!(
                        "parent {} '{}' was not found",
                        parent_rank.as_str(),
                        parent_name
                    ),
                ));
            }
            1 => Some(parents[0].taxon_id),
            _ => {
                return Err(RowIssue::ambiguous(
                    format!(
                        "parent {} '{}' matched multiple taxa",
                        parent_rank.as_str(),
                        parent_name
                    ),
                    parents,
                ));
            }
        }
    } else {
        None
    };
    let geological_range = normalize(row.geological_range.as_deref());
    let mut changes = vec![TaxonChange {
        kind: TaxonChangeKind::CreateTaxon,
        field: "taxa".into(),
        old_value: None,
        new_value: Some(format!("{}:{target_name}", target_rank.as_str())),
    }];
    let mut names = prepare_new_taxon_names(row, target_name, &mut changes)?;
    for (kind, input) in [
        (TaxonomyNameKind::English, row.english.as_ref()),
        (TaxonomyNameKind::Chinese, row.chinese.as_ref()),
    ] {
        if let Some(input) = input {
            names.push(new_first_name(kind, input, &mut changes)?);
        }
    }
    Ok(RowPlan {
        target: PlannedTarget::New {
            rank: target_rank,
            parent_taxon_id,
            geological_range,
        },
        geological_range: None,
        names,
        changes,
    })
}

fn prepare_new_taxon_names(
    row: &TaxonInputRow,
    locator_name: String,
    changes: &mut Vec<TaxonChange>,
) -> Result<Vec<NamePlan>, RowIssue> {
    let mut names = Vec::new();
    match row.scientific.as_ref() {
        Some(input)
            if normalize_name(Some(&input.name)).as_deref() == Some(locator_name.as_str()) =>
        {
            if input.is_accepted == Some(false) {
                return Err(RowIssue::new(
                    TaxonRowStatus::Conflict,
                    "a new taxon's only scientific name must be accepted",
                ));
            }
            names.push(insert_name_plan(
                TaxonomyNameKind::Scientific,
                locator_name,
                input,
                true,
                changes,
            )?);
        }
        Some(input) => {
            let input_name = required_name(input)?;
            let input_accepted = input.is_accepted == Some(true);
            names.push(insert_name_plan(
                TaxonomyNameKind::Scientific,
                locator_name,
                &TaxonNameInput::default(),
                !input_accepted,
                changes,
            )?);
            names.push(insert_name_plan(
                TaxonomyNameKind::Scientific,
                input_name,
                input,
                input_accepted,
                changes,
            )?);
        }
        None => names.push(insert_name_plan(
            TaxonomyNameKind::Scientific,
            locator_name,
            &TaxonNameInput::default(),
            true,
            changes,
        )?),
    }
    Ok(names)
}

fn new_first_name(
    kind: TaxonomyNameKind,
    input: &TaxonNameInput,
    changes: &mut Vec<TaxonChange>,
) -> Result<NamePlan, RowIssue> {
    if input.is_accepted == Some(false) {
        return Err(RowIssue::new(
            TaxonRowStatus::Conflict,
            format!("a new taxon's only {} name must be accepted", kind.as_str()),
        ));
    }
    insert_name_plan(kind, required_name(input)?, input, true, changes)
}

fn insert_name_plan(
    kind: TaxonomyNameKind,
    name: String,
    input: &TaxonNameInput,
    is_accepted: bool,
    changes: &mut Vec<TaxonChange>,
) -> Result<NamePlan, RowIssue> {
    if name.is_empty() {
        return Err(RowIssue::new(
            TaxonRowStatus::Invalid,
            format!("{} name cannot be empty", kind.as_str()),
        ));
    }
    changes.push(TaxonChange {
        kind: TaxonChangeKind::AppendName,
        field: format!("{}.{}", kind.as_str(), "name"),
        old_value: None,
        new_value: Some(name.clone()),
    });
    Ok(NamePlan {
        kind,
        name,
        insert: Some(NameRecord {
            is_accepted,
            authority_year: normalize(input.authority_year.as_deref()),
            category: normalize(input.category.as_deref()),
            source: normalize(input.source.as_deref()),
        }),
        updates: Vec::new(),
        demote_accepted: None,
        promote: false,
    })
}

fn prepare_existing_taxon(
    transaction: &Transaction<'_>,
    update: ExistingTaxonUpdate<'_>,
    options: TaxonUpdateOptions,
    taxon_id: i64,
) -> Result<RowPlan, RowIssue> {
    let mut changes = Vec::new();
    let geological_range = plan_geological_range(
        transaction,
        taxon_id,
        update.geological_range,
        options,
        &mut changes,
    )?;
    let mut names = Vec::new();
    for (kind, input) in [
        (TaxonomyNameKind::Scientific, update.scientific),
        (TaxonomyNameKind::English, update.english),
        (TaxonomyNameKind::Chinese, update.chinese),
    ] {
        if let Some(input) = input {
            names.push(plan_existing_name(
                transaction,
                taxon_id,
                kind,
                input,
                options,
                &mut changes,
            )?);
        }
    }
    Ok(RowPlan {
        target: PlannedTarget::Existing(taxon_id),
        geological_range,
        names,
        changes,
    })
}

fn plan_geological_range(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    input: Option<&str>,
    options: TaxonUpdateOptions,
    changes: &mut Vec<TaxonChange>,
) -> Result<Option<FieldUpdate>, RowIssue> {
    let Some(value) = normalize(input) else {
        return Ok(None);
    };
    let existing: Option<String> = transaction
        .query_row(
            "SELECT geological_range FROM taxa WHERE taxon_id = ?",
            [taxon_id],
            |row| row.get(0),
        )
        .map_err(database_issue)?;
    if existing.as_deref() == Some(&value) {
        return Ok(None);
    }
    if existing.is_none() {
        changes.push(TaxonChange {
            kind: TaxonChangeKind::Supplement,
            field: "taxa.geological_range".into(),
            old_value: None,
            new_value: Some(value.clone()),
        });
        return Ok(Some(FieldUpdate { value }));
    }
    if !options.allow_overwrite {
        return Err(RowIssue::new(
            TaxonRowStatus::Conflict,
            "geological_range differs and overwrite is not allowed",
        ));
    }
    changes.push(TaxonChange {
        kind: TaxonChangeKind::Overwrite,
        field: "taxa.geological_range".into(),
        old_value: existing,
        new_value: Some(value.clone()),
    });
    Ok(Some(FieldUpdate { value }))
}

fn plan_existing_name(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: TaxonomyNameKind,
    input: &TaxonNameInput,
    options: TaxonUpdateOptions,
    changes: &mut Vec<TaxonChange>,
) -> Result<NamePlan, RowIssue> {
    let name = required_name(input)?;
    let existing = transaction
        .query_row(
            r#"
            SELECT is_accepted, authority_year, category, source
            FROM taxon_names
            WHERE taxon_id = ? AND name_kind = ? AND name = ?
            "#,
            params![taxon_id, kind.code(), name],
            |row| {
                Ok(NameRecord {
                    is_accepted: row.get::<_, i64>(0)? != 0,
                    authority_year: row.get(1)?,
                    category: row.get(2)?,
                    source: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(database_issue)?;
    let accepted_name = accepted_name(transaction, taxon_id, kind).map_err(database_issue)?;
    let total_names = count_names(transaction, taxon_id, kind).map_err(database_issue)?;
    let Some(existing) = existing else {
        if !options.allow_new_names {
            return Err(RowIssue::new(
                TaxonRowStatus::Conflict,
                format!("new {} names are not allowed", kind.as_str()),
            ));
        }
        let is_accepted = input.is_accepted.unwrap_or(total_names == 0);
        if !is_accepted && total_names == 0 {
            return Err(RowIssue::new(
                TaxonRowStatus::Conflict,
                format!("the first {} name must be accepted", kind.as_str()),
            ));
        }
        let demote_accepted = if is_accepted {
            match accepted_name {
                Some(value) if options.allow_switch_accepted_name => Some(value),
                Some(_) => {
                    return Err(RowIssue::new(
                        TaxonRowStatus::Conflict,
                        format!(
                            "{} already has an accepted name and switching it is not allowed",
                            kind.as_str()
                        ),
                    ));
                }
                None => None,
            }
        } else {
            None
        };
        changes.push(TaxonChange {
            kind: TaxonChangeKind::AppendName,
            field: format!("{}.{}", kind.as_str(), "name"),
            old_value: None,
            new_value: Some(name.clone()),
        });
        if let Some(old) = demote_accepted.as_ref() {
            changes.push(accepted_change(kind, Some(old.clone()), name.clone()));
        }
        return Ok(NamePlan {
            kind,
            name,
            insert: Some(NameRecord {
                is_accepted,
                authority_year: normalize(input.authority_year.as_deref()),
                category: normalize(input.category.as_deref()),
                source: normalize(input.source.as_deref()),
            }),
            updates: Vec::new(),
            demote_accepted,
            promote: false,
        });
    };

    let mut plan = NamePlan {
        kind,
        name: name.clone(),
        insert: None,
        updates: Vec::new(),
        demote_accepted: None,
        promote: false,
    };
    for (field, old, new) in [
        (
            NameField::AuthorityYear,
            existing.authority_year,
            normalize(input.authority_year.as_deref()),
        ),
        (
            NameField::Category,
            existing.category,
            normalize(input.category.as_deref()),
        ),
        (
            NameField::Source,
            existing.source,
            normalize(input.source.as_deref()),
        ),
    ] {
        let Some(new) = new else {
            continue;
        };
        if old.as_deref() == Some(&new) {
            continue;
        }
        if old.is_none() {
            changes.push(TaxonChange {
                kind: TaxonChangeKind::Supplement,
                field: format!("{}.{}", kind.as_str(), field.as_str()),
                old_value: None,
                new_value: Some(new.clone()),
            });
            plan.updates.push(NameFieldUpdate { field, value: new });
            continue;
        }
        if !options.allow_overwrite {
            return Err(RowIssue::new(
                TaxonRowStatus::Conflict,
                format!(
                    "{}.{} differs and overwrite is not allowed",
                    kind.as_str(),
                    field.as_str()
                ),
            ));
        }
        changes.push(TaxonChange {
            kind: TaxonChangeKind::Overwrite,
            field: format!("{}.{}", kind.as_str(), field.as_str()),
            old_value: old,
            new_value: Some(new.clone()),
        });
        plan.updates.push(NameFieldUpdate { field, value: new });
    }
    match input.is_accepted {
        None => {}
        Some(value) if value == existing.is_accepted => {}
        Some(false) => {
            return Err(RowIssue::new(
                TaxonRowStatus::Conflict,
                "the accepted name cannot be demoted without selecting a replacement",
            ));
        }
        Some(true) if !options.allow_switch_accepted_name => {
            return Err(RowIssue::new(
                TaxonRowStatus::Conflict,
                format!(
                    "{}.is_accepted differs and switching the accepted name is not allowed",
                    kind.as_str()
                ),
            ));
        }
        Some(true) => {
            plan.demote_accepted = accepted_name.filter(|accepted| accepted != &name);
            plan.promote = true;
            changes.push(accepted_change(kind, plan.demote_accepted.clone(), name));
        }
    }
    Ok(plan)
}

fn accepted_change(kind: TaxonomyNameKind, old: Option<String>, new: String) -> TaxonChange {
    TaxonChange {
        kind: TaxonChangeKind::ChangeAcceptedName,
        field: format!("{}.is_accepted", kind.as_str()),
        old_value: old,
        new_value: Some(new),
    }
}

fn execute_plan(
    transaction: &Transaction<'_>,
    row_number: usize,
    plan: RowPlan,
) -> CoreResult<TaxonRowOutcome> {
    let taxon_id = match plan.target {
        PlannedTarget::Existing(taxon_id) => {
            if let Some(update) = plan.geological_range {
                transaction.execute(
                    "UPDATE taxa SET geological_range = ? WHERE taxon_id = ?",
                    params![update.value, taxon_id],
                )?;
            }
            taxon_id
        }
        PlannedTarget::New {
            rank,
            parent_taxon_id,
            geological_range,
        } => {
            transaction.execute(
                "INSERT INTO taxa (parent_taxon_id, rank, geological_range) VALUES (?, ?, ?)",
                params![parent_taxon_id, rank.code(), geological_range],
            )?;
            transaction.last_insert_rowid()
        }
    };
    for name in &plan.names {
        execute_name_plan(transaction, taxon_id, name)?;
    }
    for kind in [
        TaxonomyNameKind::Scientific,
        TaxonomyNameKind::English,
        TaxonomyNameKind::Chinese,
    ] {
        if plan.names.iter().any(|name| name.kind == kind) {
            validate_accepted_name(transaction, taxon_id, kind)?;
        }
    }
    let target = load_taxon_summary(transaction, taxon_id)?.ok_or_else(|| {
        CoreError::InvalidArgument(format!("applied taxon {taxon_id} no longer exists"))
    })?;
    let status = if plan.changes.is_empty() {
        TaxonRowStatus::NoChange
    } else {
        TaxonRowStatus::Ready
    };
    Ok(TaxonRowOutcome {
        row_number,
        operation_id: None,
        status,
        message: if status == TaxonRowStatus::NoChange {
            "no change".into()
        } else {
            "ready".into()
        },
        target: Some(target),
        candidates: Vec::new(),
        changes: plan.changes,
    })
}

fn execute_name_plan(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    plan: &NamePlan,
) -> CoreResult<()> {
    if let Some(old_name) = plan.demote_accepted.as_ref() {
        transaction.execute(
            r#"
            UPDATE taxon_names
            SET is_accepted = 0
            WHERE taxon_id = ? AND name_kind = ? AND name = ?
            "#,
            params![taxon_id, plan.kind.code(), old_name],
        )?;
    }
    if let Some(record) = plan.insert.as_ref() {
        transaction.execute(
            r#"
            INSERT INTO taxon_names (
                taxon_id, name_kind, name, is_accepted, authority_year, category, source
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            params![
                taxon_id,
                plan.kind.code(),
                plan.name,
                i64::from(record.is_accepted),
                record.authority_year,
                record.category,
                record.source
            ],
        )?;
    }
    for update in &plan.updates {
        let sql = format!(
            "UPDATE taxon_names SET {} = ? WHERE taxon_id = ? AND name_kind = ? AND name = ?",
            update.field.as_str(),
        );
        transaction.execute(
            &sql,
            params![update.value, taxon_id, plan.kind.code(), plan.name],
        )?;
    }
    if plan.promote {
        transaction.execute(
            r#"
            UPDATE taxon_names
            SET is_accepted = 1
            WHERE taxon_id = ? AND name_kind = ? AND name = ?
            "#,
            params![taxon_id, plan.kind.code(), plan.name],
        )?;
    }
    Ok(())
}

pub(super) fn insert_operation_batch<T: Serialize + ?Sized>(
    transaction: &Transaction<'_>,
    inputs: &T,
    context: &TaxonomyBatchContext,
) -> CoreResult<i64> {
    let input_json = serialize_json(inputs, "taxonomy inputs")?;
    let context_json = serialize_json(context, "taxonomy batch context")?;
    transaction.execute(
        r#"
        INSERT INTO taxonomy_operation_batches (context_json, input_json)
        VALUES (?, ?)
        "#,
        params![context_json, input_json],
    )?;
    Ok(transaction.last_insert_rowid())
}

pub(super) fn insert_operation_log(
    transaction: &Transaction<'_>,
    batch_id: i64,
    row_number: usize,
    changeset_blob: &[u8],
) -> CoreResult<i64> {
    transaction.execute(
        r#"
        INSERT INTO taxonomy_operations (
            batch_id, row_number, status, changeset_blob
        ) VALUES (?, ?, 'applied', ?)
        "#,
        params![batch_id, row_number as i64, changeset_blob],
    )?;
    Ok(transaction.last_insert_rowid())
}

const TAXONOMY_SESSION_TABLES: [&str; 3] = ["taxa", "taxon_names", "taxon_identifiers"];

pub(super) fn is_taxonomy_session_table(table_name: &str) -> bool {
    TAXONOMY_SESSION_TABLES.contains(&table_name)
}

pub(super) fn start_taxonomy_session(connection: &Connection) -> CoreResult<Session<'_>> {
    let mut session = Session::new(connection)?;
    for table in TAXONOMY_SESSION_TABLES {
        session.attach(Some(table))?;
    }
    Ok(session)
}

pub(super) fn finish_taxonomy_session(session: &mut Session<'_>) -> CoreResult<Vec<u8>> {
    if session.is_empty() {
        return Err(CoreError::InvalidArgument(
            "taxonomy operation did not change any tracked rows".into(),
        ));
    }
    let mut changeset_blob = Vec::new();
    session.changeset_strm(&mut changeset_blob)?;
    if changeset_blob.is_empty() {
        return Err(CoreError::InvalidArgument(
            "taxonomy operation did not produce a changeset".into(),
        ));
    }
    Ok(changeset_blob)
}

fn ensure_taxon_exists_in_connection(connection: &Connection, taxon_id: i64) -> CoreResult<()> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM taxa WHERE taxon_id = ?)",
        [taxon_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(CoreError::InvalidArgument(format!(
            "applied operation target taxon {taxon_id} no longer exists"
        )));
    }
    Ok(())
}

pub(super) fn validate_taxonomy(connection: &Connection) -> CoreResult<()> {
    validate_taxon_parentage(connection)?;
    validate_taxon_names(connection)?;
    Ok(())
}

fn validate_taxon_parentage(connection: &Connection) -> CoreResult<()> {
    let invalid_taxon_id = connection
        .query_row(
            r#"
            SELECT child.taxon_id
            FROM taxa AS child
            LEFT JOIN taxa AS parent ON parent.taxon_id = child.parent_taxon_id
            WHERE (child.rank = 1 AND child.parent_taxon_id IS NOT NULL)
               OR (child.rank > 1 AND (parent.taxon_id IS NULL OR parent.rank != child.rank - 1))
            LIMIT 1
            "#,
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if let Some(taxon_id) = invalid_taxon_id {
        return Err(CoreError::InvalidArgument(format!(
            "taxon {taxon_id} has invalid parentage"
        )));
    }
    Ok(())
}

fn validate_taxon_names(connection: &Connection) -> CoreResult<()> {
    let mut statement = connection.prepare(
        r#"
        SELECT name_id, name
        FROM taxon_names
        ORDER BY name_id
        "#,
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (name_id, name) = row?;
        if normalize_name(Some(&name)).as_deref() != Some(name.as_str()) {
            return Err(CoreError::InvalidArgument(format!(
                "taxon name {name_id} is not normalized"
            )));
        }
    }

    for kind in [
        TaxonomyNameKind::Scientific,
        TaxonomyNameKind::English,
        TaxonomyNameKind::Chinese,
    ] {
        validate_accepted_names_for_kind(connection, kind)?;
    }
    Ok(())
}

fn validate_accepted_names_for_kind(
    connection: &Connection,
    kind: TaxonomyNameKind,
) -> CoreResult<()> {
    let invalid_taxon = connection
        .query_row(
            r#"
            SELECT taxa.taxon_id, COUNT(taxon_names.name_id), COALESCE(SUM(taxon_names.is_accepted), 0)
            FROM taxa
            LEFT JOIN taxon_names
              ON taxon_names.taxon_id = taxa.taxon_id
             AND taxon_names.name_kind = ?
            GROUP BY taxa.taxon_id
            HAVING (? = 1 AND COALESCE(SUM(taxon_names.is_accepted), 0) != 1)
                OR (? != 1 AND COUNT(taxon_names.name_id) > 0 AND COALESCE(SUM(taxon_names.is_accepted), 0) != 1)
            LIMIT 1
            "#,
            params![kind.code(), kind.code(), kind.code()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    if let Some((taxon_id, total, accepted)) = invalid_taxon {
        return Err(CoreError::InvalidArgument(format!(
            "{} names for taxon {} have invalid accepted count: {} of {}",
            kind.as_str(),
            taxon_id,
            accepted,
            total
        )));
    }
    Ok(())
}

pub(super) fn serialize_json<T: Serialize + ?Sized>(value: &T, label: &str) -> CoreResult<String> {
    serde_json::to_string(value)
        .map_err(|error| CoreError::InvalidArgument(format!("invalid {label}: {error}")))
}

fn deserialize_json<T: for<'de> Deserialize<'de>>(value: &str, label: &str) -> CoreResult<T> {
    serde_json::from_str(value)
        .map_err(|error| CoreError::InvalidArgument(format!("invalid {label}: {error}")))
}

fn validate_accepted_name(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: TaxonomyNameKind,
) -> CoreResult<()> {
    let (total, accepted): (i64, i64) = transaction.query_row(
        r#"
        SELECT COUNT(*), COALESCE(SUM(is_accepted), 0)
        FROM taxon_names
        WHERE taxon_id = ? AND name_kind = ?
        "#,
        params![taxon_id, kind.code()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if total > 0 && accepted != 1 {
        return Err(CoreError::InvalidArgument(format!(
            "{} names must have exactly one accepted value for taxon {taxon_id}",
            kind.as_str()
        )));
    }
    Ok(())
}

fn find_candidates(
    transaction: &Transaction<'_>,
    target_rank: TaxonRank,
    target_name: &str,
    path: &NormalizedPath,
) -> CoreResult<CandidateSearch> {
    let mut statement = transaction.prepare(
        r#"
        SELECT DISTINCT taxa.taxon_id
        FROM taxa
        JOIN taxon_names ON taxon_names.taxon_id = taxa.taxon_id
        WHERE taxa.rank = ?
          AND taxon_names.name_kind = ?
          AND taxon_names.name = ? COLLATE BINARY
        ORDER BY taxa.taxon_id
        "#,
    )?;
    let rows = statement.query_map(
        params![
            target_rank.code(),
            TaxonomyNameKind::Scientific.code(),
            target_name
        ],
        |row| row.get::<_, i64>(0),
    )?;
    let ids = rows.collect::<Result<Vec<_>, _>>()?;
    let had_name_match = !ids.is_empty();
    let mut candidate_ids = Vec::new();
    for taxon_id in ids {
        let mut matches = true;
        for rank in TaxonRank::ALL.into_iter().take(target_rank.index()) {
            if let Some(name) = path.get(rank)
                && !lineage_has_name(transaction, taxon_id, rank, name)?
            {
                matches = false;
                break;
            }
        }
        if matches {
            candidate_ids.push(taxon_id);
        }
    }
    let candidates = load_taxon_summaries(transaction, &candidate_ids)?;
    if candidates.len() != candidate_ids.len() {
        return Err(CoreError::InvalidArgument(
            "candidate taxon no longer exists".into(),
        ));
    }
    Ok(CandidateSearch {
        had_name_match,
        candidates,
    })
}

struct CandidateSearch {
    had_name_match: bool,
    candidates: Vec<TaxonSummary>,
}

fn lineage_has_name(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    rank: TaxonRank,
    name: &str,
) -> CoreResult<bool> {
    Ok(transaction.query_row(
        r#"
        WITH RECURSIVE lineage(taxon_id, parent_taxon_id, rank) AS (
            SELECT taxon_id, parent_taxon_id, rank FROM taxa WHERE taxon_id = ?
            UNION ALL
            SELECT parent.taxon_id, parent.parent_taxon_id, parent.rank
            FROM taxa AS parent
            JOIN lineage AS child ON child.parent_taxon_id = parent.taxon_id
        )
        SELECT EXISTS(
            SELECT 1
            FROM lineage
            JOIN taxon_names ON taxon_names.taxon_id = lineage.taxon_id
            WHERE lineage.rank = ?
              AND taxon_names.name_kind = ?
              AND taxon_names.name = ? COLLATE BINARY
        )
        "#,
        params![
            taxon_id,
            rank.code(),
            TaxonomyNameKind::Scientific.code(),
            name
        ],
        |row| row.get(0),
    )?)
}

fn accepted_name(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: TaxonomyNameKind,
) -> rusqlite::Result<Option<String>> {
    transaction
        .query_row(
            r#"
            SELECT name
            FROM taxon_names
            WHERE taxon_id = ? AND name_kind = ? AND is_accepted = 1
            "#,
            params![taxon_id, kind.code()],
            |row| row.get(0),
        )
        .optional()
}

fn count_names(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: TaxonomyNameKind,
) -> rusqlite::Result<i64> {
    transaction.query_row(
        "SELECT COUNT(*) FROM taxon_names WHERE taxon_id = ? AND name_kind = ?",
        params![taxon_id, kind.code()],
        |row| row.get(0),
    )
}

fn required_name(input: &TaxonNameInput) -> Result<String, RowIssue> {
    normalize_name(Some(&input.name)).ok_or_else(|| {
        RowIssue::new(
            TaxonRowStatus::Invalid,
            "a supplied name group must include a non-empty name",
        )
    })
}

pub(super) fn normalize_name(value: Option<&str>) -> Option<String> {
    value.and_then(|value| {
        let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
        (!value.is_empty()).then_some(value)
    })
}

fn normalize(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn database_issue(error: rusqlite::Error) -> RowIssue {
    RowIssue::new(TaxonRowStatus::Invalid, format!("database error: {error}"))
}

fn core_issue(error: CoreError) -> RowIssue {
    RowIssue::new(TaxonRowStatus::Invalid, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taxonomy::{
        DeleteTaxonNameInput, TaxonUpdateInput, delete_taxon, delete_taxon_name,
        execute_custom_taxonomy_sql, get_taxon_detail, get_taxon_detail_node, get_taxon_summary,
        list_taxon_children, search_taxa, update_taxon,
    };

    fn database() -> (tempfile::TempDir, Database) {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(directory.path().join("vividarium.db")).unwrap();
        (directory, database)
    }

    fn seed_lineage(database: &Database) -> [i64; 4] {
        let mut connection = database.connect().unwrap();
        let transaction = connection.transaction().unwrap();
        let mut ids = [0; 4];
        let mut parent = None;
        for (index, (rank, name)) in [
            (TaxonRank::Kingdom, "Animalia"),
            (TaxonRank::Order, "Carnivora"),
            (TaxonRank::Family, "Canidae"),
            (TaxonRank::Genus, "Canis"),
        ]
        .into_iter()
        .enumerate()
        {
            transaction
                .execute(
                    "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, ?)",
                    params![parent, rank.code()],
                )
                .unwrap();
            let id = transaction.last_insert_rowid();
            transaction
                .execute(
                    "INSERT INTO taxon_names (taxon_id, name_kind, name, is_accepted) VALUES (?, ?, ?, 1)",
                    params![id, TaxonomyNameKind::Scientific.code(), name],
                )
                .unwrap();
            ids[index] = id;
            parent = Some(id);
        }
        transaction.commit().unwrap();
        ids
    }

    fn species_row() -> TaxonInputRow {
        species_row_named("Canis lupus")
    }

    fn species_row_named(name: &str) -> TaxonInputRow {
        TaxonInputRow {
            species: Some(name.into()),
            ..TaxonInputRow::default()
        }
    }

    #[test]
    fn creates_a_species_and_derives_its_genus() {
        let (_directory, database) = database();
        let ids = seed_lineage(&database);
        let result = apply_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(result.batch_id.is_some());
        assert_eq!(result.rows[0].status, TaxonRowStatus::Applied);
        let connection = database.connect().unwrap();
        let parent: i64 = connection
            .query_row(
                "SELECT parent_taxon_id FROM taxa WHERE rank = 5",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(parent, ids[3]);
        let accepted: i64 = connection
            .query_row(
                "SELECT is_accepted FROM taxon_names WHERE name_kind = 1 AND name = 'Canis lupus'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(accepted, 1);
    }

    #[test]
    fn loads_summary_and_detail_views_for_a_taxon() {
        let (_directory, database) = database();
        let ids = seed_lineage(&database);
        let created = apply_rows(
            &database,
            &[TaxonInputRow {
                species: Some("Canis lupus".into()),
                geological_range: Some("Holocene".into()),
                scientific: Some(TaxonNameInput {
                    name: "Canis lupus".into(),
                    authority_year: Some("Linnaeus, 1758".into()),
                    source: Some("local".into()),
                    ..TaxonNameInput::default()
                }),
                english: Some(TaxonNameInput {
                    name: "gray wolf".into(),
                    ..TaxonNameInput::default()
                }),
                chinese: Some(TaxonNameInput {
                    name: "wolf".into(),
                    ..TaxonNameInput::default()
                }),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let taxon_id = created.rows[0].target.as_ref().unwrap().taxon_id;
        let connection = database.connect().unwrap();
        connection
            .execute(
                r#"
                INSERT INTO taxon_names (
                    taxon_id, name_kind, name, is_accepted, category, source
                ) VALUES (?, 1, 'Canis lycaon', 0, 'synonym', 'local')
                "#,
                [taxon_id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO taxon_identifiers (taxon_id, source, external_id) VALUES (?, 'biolib', '123')",
                [taxon_id],
            )
            .unwrap();
        drop(connection);

        let summary = get_taxon_summary(&database, taxon_id).unwrap().unwrap();
        assert_eq!(summary.rank, TaxonRank::Species);
        assert_eq!(summary.names.scientific.as_deref(), Some("Canis lupus"));
        assert_eq!(summary.names.english.as_deref(), Some("gray wolf"));
        assert_eq!(summary.names.chinese.as_deref(), Some("wolf"));
        assert_eq!(summary.breadcrumb.len(), 4);
        assert_eq!(summary.breadcrumb[0].rank, TaxonRank::Kingdom);
        assert_eq!(summary.breadcrumb[3].taxon_id, ids[3]);
        assert_eq!(
            summary.breadcrumb[3].names.scientific.as_deref(),
            Some("Canis")
        );

        let detail = get_taxon_detail(&database, taxon_id).unwrap().unwrap();
        assert_eq!(detail.taxon_id, taxon_id);
        assert_eq!(detail.rank, TaxonRank::Species);
        assert_eq!(detail.parent_taxon_id, Some(ids[3]));
        assert_eq!(detail.geological_range.as_deref(), Some("Holocene"));
        assert_eq!(detail.names.scientific.len(), 2);
        assert!(detail.names.scientific[0].is_accepted);
        assert_eq!(
            detail.names.scientific[0].authority_year.as_deref(),
            Some("Linnaeus, 1758")
        );
        assert_eq!(detail.identifiers.len(), 1);
        assert_eq!(detail.identifiers[0].external_id, "123");
    }

    #[test]
    fn searches_taxa_by_any_name_and_loads_child_summaries() {
        let (_directory, database) = database();
        let ids = seed_lineage(&database);
        let result = apply_rows(
            &database,
            &[TaxonInputRow {
                species: Some("Canis lupus".into()),
                english: Some(TaxonNameInput {
                    name: "gray wolf".into(),
                    ..TaxonNameInput::default()
                }),
                chinese: Some(TaxonNameInput {
                    name: "wolf".into(),
                    ..TaxonNameInput::default()
                }),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                allow_new_names: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let species_id = result.rows[0].target.as_ref().unwrap().taxon_id;

        let matches = search_taxa(&database, "wolf", 10).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].summary.taxon_id, species_id);
        assert_eq!(matches[0].detail.taxon_id, species_id);
        assert!(matches[0].matches.iter().any(|value| {
            value.name_kind == TaxonomyNameKind::English && value.name == "gray wolf"
        }));
        assert_eq!(matches[0].summary.breadcrumb[3].taxon_id, ids[3]);

        let word_prefix_matches = search_taxa(&database, "lu", 10).unwrap();
        assert_eq!(word_prefix_matches.len(), 1);
        assert_eq!(word_prefix_matches[0].summary.taxon_id, species_id);

        assert!(search_taxa(&database, "up", 10).unwrap().is_empty());
        let contains_matches = search_taxa(&database, "upu", 10).unwrap();
        assert_eq!(contains_matches.len(), 1);
        assert_eq!(contains_matches[0].summary.taxon_id, species_id);

        let genus = get_taxon_detail_node(&database, ids[3], None, 50)
            .unwrap()
            .unwrap();
        assert_eq!(genus.summary.taxon_id, ids[3]);
        assert_eq!(genus.children.items.len(), 1);
        assert_eq!(genus.children.items[0].taxon_id, species_id);
        assert_eq!(genus.children.items[0].rank, TaxonRank::Species);
        assert_eq!(
            genus.children.items[0].names.scientific.as_deref(),
            Some("Canis lupus")
        );
    }

    #[test]
    fn searches_taxa_by_trigram_candidates_and_edit_distance() {
        let (_directory, database) = database();
        seed_lineage(&database);
        let result = apply_rows(
            &database,
            &[
                TaxonInputRow {
                    species: Some("Canis lupus".into()),
                    english: Some(TaxonNameInput {
                        name: "gray wolf".into(),
                        ..TaxonNameInput::default()
                    }),
                    ..TaxonInputRow::default()
                },
                species_row_named("Canis lupis"),
            ],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                allow_new_names: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let lupus_id = result.rows[0].target.as_ref().unwrap().taxon_id;
        let lupis_id = result.rows[1].target.as_ref().unwrap().taxon_id;

        let typo_matches = search_taxa(&database, "Canis lupuz", 10).unwrap();
        assert_eq!(typo_matches[0].summary.taxon_id, lupus_id);
        assert!(typo_matches[0].matches.iter().any(|value| {
            value.name_kind == TaxonomyNameKind::Scientific && value.name == "Canis lupus"
        }));

        let alternate_name_matches = search_taxa(&database, "gray wlf", 10).unwrap();
        assert_eq!(alternate_name_matches.len(), 1);
        assert_eq!(alternate_name_matches[0].summary.taxon_id, lupus_id);
        assert!(alternate_name_matches[0].matches.iter().any(|value| {
            value.name_kind == TaxonomyNameKind::English && value.name == "gray wolf"
        }));

        let layered_matches = search_taxa(&database, "Canis lupus", 2).unwrap();
        assert_eq!(layered_matches.len(), 2);
        assert_eq!(layered_matches[0].summary.taxon_id, lupus_id);
        assert_eq!(layered_matches[1].summary.taxon_id, lupis_id);

        assert!(
            search_taxa(&database, "Canis lxxxxx", 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn normalizes_name_input_and_search_whitespace() {
        let (_directory, database) = database();
        seed_lineage(&database);
        let result = apply_rows(
            &database,
            &[TaxonInputRow {
                species: Some("  Canis   lupus  ".into()),
                english: Some(TaxonNameInput {
                    name: " gray\t\n wolf ".into(),
                    ..TaxonNameInput::default()
                }),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                allow_new_names: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let taxon_id = result.rows[0].target.as_ref().unwrap().taxon_id;
        let connection = database.connect().unwrap();
        let scientific: String = connection
            .query_row(
                "SELECT name FROM taxon_names WHERE taxon_id = ? AND name_kind = 1",
                [taxon_id],
                |row| row.get(0),
            )
            .unwrap();
        let english: String = connection
            .query_row(
                "SELECT name FROM taxon_names WHERE taxon_id = ? AND name_kind = 2",
                [taxon_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(scientific, "Canis lupus");
        assert_eq!(english, "gray wolf");

        let matches = search_taxa(&database, " canis   lu ", 10).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].summary.taxon_id, taxon_id);
    }

    #[test]
    fn limits_taxon_search_and_pages_children_with_cursors() {
        let (_directory, database) = database();
        let ids = seed_lineage(&database);
        apply_rows(
            &database,
            &[
                species_row_named("Canis lupus"),
                species_row_named("Canis latrans"),
                species_row_named("Canis rufus"),
            ],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();

        let exact_matches = search_taxa(&database, "Canis", 1).unwrap();
        assert_eq!(exact_matches.len(), 1);
        assert_eq!(exact_matches[0].summary.taxon_id, ids[3]);

        let limited_matches = search_taxa(&database, "Canis", 2).unwrap();
        assert_eq!(limited_matches.len(), 2);

        let first_children = list_taxon_children(&database, ids[3], None, 2).unwrap();
        assert_eq!(first_children.items.len(), 2);
        assert!(first_children.next_cursor.is_some());
        let second_children =
            list_taxon_children(&database, ids[3], first_children.next_cursor.as_deref(), 2)
                .unwrap();
        assert_eq!(second_children.items.len(), 1);
        assert!(second_children.next_cursor.is_none());

        let genus = get_taxon_detail_node(&database, ids[3], None, 2)
            .unwrap()
            .unwrap();
        assert_eq!(genus.children.items.len(), 2);
        assert!(genus.children.next_cursor.is_some());
    }

    #[test]
    fn updates_a_taxon_from_query_through_the_shared_operation_log() {
        let (_directory, database) = database();
        seed_lineage(&database);
        let created = apply_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let taxon_id = created.rows[0].target.as_ref().unwrap().taxon_id;

        let updated = update_taxon(
            &database,
            TaxonUpdateInput {
                taxon_id,
                geological_range: None,
                scientific: None,
                english: Some(TaxonNameInput {
                    name: "gray wolf".into(),
                    ..TaxonNameInput::default()
                }),
                chinese: None,
            },
            TaxonUpdateOptions {
                allow_new_names: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert_eq!(updated.outcome.status, TaxonRowStatus::Applied);
        assert!(updated.batch_id.is_some());
        let operation_id = updated.outcome.operation_id.unwrap();
        let operation = list_taxonomy_operations(&database, None, 1)
            .unwrap()
            .items
            .remove(0);
        assert_eq!(operation.operation_id, operation_id);
        assert!(operation.changeset_size > 0);

        let detail = get_taxon_detail(&database, taxon_id).unwrap().unwrap();
        assert_eq!(detail.names.english[0].name, "gray wolf");
        revert_taxonomy_operation(&database, operation_id).unwrap();
        let reverted = get_taxon_detail(&database, taxon_id).unwrap().unwrap();
        assert!(reverted.names.english.is_empty());
    }

    #[test]
    fn deletes_a_taxon_name_and_can_revert_it() {
        let (_directory, database) = database();
        seed_lineage(&database);
        let result = apply_rows(
            &database,
            &[TaxonInputRow {
                species: Some("Canis lupus".into()),
                scientific: Some(TaxonNameInput {
                    name: "Canis lycaon".into(),
                    category: Some("synonym".into()),
                    ..TaxonNameInput::default()
                }),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                allow_new_names: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let taxon_id = result.rows[0].target.as_ref().unwrap().taxon_id;

        let deleted = delete_taxon_name(
            &database,
            DeleteTaxonNameInput {
                taxon_id,
                name_kind: TaxonomyNameKind::Scientific,
                name: "Canis lycaon".into(),
                replacement_accepted_name: None,
            },
        )
        .unwrap();
        let connection = database.connect().unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM taxon_names WHERE name_kind = 1 AND name = 'Canis lycaon'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        drop(connection);

        revert_taxonomy_operation(&database, deleted.operation_id).unwrap();
        let connection = database.connect().unwrap();
        let restored: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM taxon_names WHERE name_kind = 1 AND name = 'Canis lycaon'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(restored, 1);
    }

    #[test]
    fn deletes_a_leaf_taxon_and_can_revert_it() {
        let (_directory, database) = database();
        seed_lineage(&database);
        let result = apply_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let taxon_id = result.rows[0].target.as_ref().unwrap().taxon_id;

        let deleted = delete_taxon(&database, taxon_id).unwrap();
        assert!(get_taxon_detail(&database, taxon_id).unwrap().is_none());
        revert_taxonomy_operation(&database, deleted.operation_id).unwrap();
        assert!(get_taxon_detail(&database, taxon_id).unwrap().is_some());
    }

    #[test]
    fn executes_custom_sql_and_records_a_batch() {
        let (_directory, database) = database();
        let ids = seed_lineage(&database);
        let result = execute_custom_taxonomy_sql(
            &database,
            &format!(
                "UPDATE taxa SET geological_range = 'Holocene' WHERE taxon_id = {}",
                ids[3]
            ),
            None,
        )
        .unwrap();
        let batch_id = result.batch_id.unwrap();
        let operation_id = result.operation_id.unwrap();
        assert!(batch_id > 0);
        assert!(operation_id > 0);
        assert!(result.changeset_size > 0);
        let connection = database.connect().unwrap();
        let range: String = connection
            .query_row(
                "SELECT geological_range FROM taxa WHERE taxon_id = ?",
                [ids[3]],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(range, "Holocene");
        let context_json: String = connection
            .query_row(
                "SELECT context_json FROM taxonomy_operation_batches WHERE batch_id = ?",
                [batch_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            deserialize_json::<TaxonomyBatchContext>(&context_json, "context").unwrap(),
            TaxonomyBatchContext::CustomSql { input: None }
        );
    }

    #[test]
    fn executes_custom_sql_with_temp_input_and_can_revert_it() {
        let (_directory, database) = database();
        let ids = seed_lineage(&database);
        let result = execute_custom_taxonomy_sql(
            &database,
            "UPDATE taxa
             SET geological_range = (
                 SELECT geological_range FROM temp.input
                 WHERE input.taxon_id = taxa.taxon_id
             )
             WHERE taxon_id IN (SELECT taxon_id FROM temp.input)",
            Some(TaxonomyCustomSqlTempTable {
                columns: vec!["taxon_id".into(), "geological_range".into()],
                rows: vec![vec![ids[3].to_string(), "Holocene".into()]],
            }),
        )
        .unwrap();
        let batch_id = result.batch_id.unwrap();
        let operation_id = result.operation_id.unwrap();
        assert!(operation_id > 0);
        assert!(result.changeset_size > 0);
        let connection = database.connect().unwrap();
        let range: String = connection
            .query_row(
                "SELECT geological_range FROM taxa WHERE taxon_id = ?",
                [ids[3]],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(range, "Holocene");
        let context_json: String = connection
            .query_row(
                "SELECT context_json FROM taxonomy_operation_batches WHERE batch_id = ?",
                [batch_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            deserialize_json::<TaxonomyBatchContext>(&context_json, "context").unwrap(),
            TaxonomyBatchContext::CustomSql {
                input: Some(TaxonomyCustomSqlTempTableMetadata {
                    columns: vec!["taxon_id".into(), "geological_range".into()],
                    row_count: 1,
                })
            }
        );
        drop(connection);

        revert_taxonomy_operation(&database, operation_id).unwrap();
        let connection = database.connect().unwrap();
        let range: Option<String> = connection
            .query_row(
                "SELECT geological_range FROM taxa WHERE taxon_id = ?",
                [ids[3]],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(range, None);
    }

    #[test]
    fn custom_sql_without_changes_does_not_record_logs() {
        let (_directory, database) = database();
        let ids = seed_lineage(&database);
        let result = execute_custom_taxonomy_sql(
            &database,
            &format!(
                "UPDATE taxa SET geological_range = geological_range WHERE taxon_id = {}",
                ids[3]
            ),
            None,
        )
        .unwrap();
        assert_eq!(result.batch_id, None);
        assert_eq!(result.operation_id, None);
        assert_eq!(result.changeset_size, 0);
        let connection = database.connect().unwrap();
        let batch_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM taxonomy_operation_batches",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let operation_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM taxonomy_operations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(batch_count, 0);
        assert_eq!(operation_count, 0);
    }

    #[test]
    fn restricts_custom_sql_to_taxonomy_tables() {
        let (_directory, database) = database();
        let error = execute_custom_taxonomy_sql(
            &database,
            "UPDATE photos SET filename = 'blocked.jpg'",
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("not authorized"));

        let error = execute_custom_taxonomy_sql(&database, "DROP TABLE taxa", None).unwrap_err();
        assert!(error.to_string().contains("not authorized"));
    }

    #[test]
    fn validates_taxonomy_after_custom_sql() {
        let (_directory, database) = database();
        seed_lineage(&database);
        let error = execute_custom_taxonomy_sql(
            &database,
            "UPDATE taxon_names SET name = ' Canis  ' WHERE name = 'Canis'",
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("not normalized"));
        let connection = database.connect().unwrap();
        let name: String = connection
            .query_row(
                "SELECT name FROM taxon_names WHERE name_kind = 1 AND name = 'Canis'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "Canis");
    }

    #[test]
    fn requires_the_immediate_parent_for_non_species_taxa() {
        let (_directory, database) = database();
        let result = preview_rows(
            &database,
            &[TaxonInputRow {
                genus: Some("Canis".into()),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert_eq!(result.rows[0].status, TaxonRowStatus::Invalid);
        assert!(result.rows[0].message.contains("family"));
    }

    #[test]
    fn does_not_require_the_parent_when_new_taxa_are_disabled() {
        let (_directory, database) = database();
        let ids = seed_lineage(&database);
        let result = preview_rows(
            &database,
            &[TaxonInputRow {
                genus: Some("Canis".into()),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions::default(),
        )
        .unwrap();
        assert_eq!(result.rows[0].status, TaxonRowStatus::NoChange);
        assert_eq!(result.rows[0].target.as_ref().unwrap().taxon_id, ids[3]);
    }

    #[test]
    fn filters_duplicate_names_with_coarse_ranks() {
        let (_directory, database) = database();
        let ids = seed_lineage(&database);
        let connection = database.connect().unwrap();
        connection
            .execute(
                "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, ?)",
                params![ids[3], TaxonRank::Species.code()],
            )
            .unwrap();
        let first = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO taxon_names (taxon_id, name_kind, name, is_accepted) VALUES (?, 1, 'Canis lupus', 1)",
                [first],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO taxa (rank) VALUES (?)",
                [TaxonRank::Genus.code()],
            )
            .unwrap();
        let other_genus = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO taxon_names (taxon_id, name_kind, name, is_accepted) VALUES (?, 1, 'Canis', 1)",
                [other_genus],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, ?)",
                params![other_genus, TaxonRank::Species.code()],
            )
            .unwrap();
        let second = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO taxon_names (taxon_id, name_kind, name, is_accepted) VALUES (?, 1, 'Canis lupus', 1)",
                [second],
            )
            .unwrap();
        drop(connection);

        let ambiguous =
            preview_rows(&database, &[species_row()], TaxonUpdateOptions::default()).unwrap();
        assert_eq!(ambiguous.rows[0].status, TaxonRowStatus::Ambiguous);
        assert_eq!(ambiguous.rows[0].candidates.len(), 2);
        let first_candidate = ambiguous.rows[0]
            .candidates
            .iter()
            .find(|candidate| candidate.taxon_id == first)
            .unwrap();
        assert_eq!(
            first_candidate.names.scientific.as_deref(),
            Some("Canis lupus")
        );
        assert_eq!(first_candidate.breadcrumb.len(), 4);
        assert_eq!(
            first_candidate.breadcrumb[2].names.scientific.as_deref(),
            Some("Canidae")
        );

        let filtered = preview_rows(
            &database,
            &[TaxonInputRow {
                family: Some("Canidae".into()),
                genus: Some("Canis".into()),
                species: Some("Canis lupus".into()),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions::default(),
        )
        .unwrap();
        assert_eq!(filtered.rows[0].status, TaxonRowStatus::NoChange);
        assert_eq!(filtered.rows[0].target.as_ref().unwrap().taxon_id, first);
    }

    #[test]
    fn does_not_create_a_duplicate_when_coarse_filters_are_wrong() {
        let (_directory, database) = database();
        let ids = seed_lineage(&database);
        let connection = database.connect().unwrap();
        connection
            .execute(
                "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, ?)",
                params![ids[3], TaxonRank::Species.code()],
            )
            .unwrap();
        let species_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO taxon_names (taxon_id, name_kind, name, is_accepted) VALUES (?, 1, 'Canis lupus', 1)",
                [species_id],
            )
            .unwrap();
        drop(connection);

        let result = apply_rows(
            &database,
            &[TaxonInputRow {
                family: Some("Felidae".into()),
                genus: Some("Canis".into()),
                species: Some("Canis lupus".into()),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert_eq!(result.batch_id, None);
        assert_eq!(result.rows[0].status, TaxonRowStatus::Conflict);
        let connection = database.connect().unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM taxon_names WHERE name_kind = 1 AND name = 'Canis lupus'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn appends_names_only_when_the_permission_is_enabled() {
        let (_directory, database) = database();
        seed_lineage(&database);
        apply_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let row = TaxonInputRow {
            species: Some("Canis lupus".into()),
            chinese: Some(TaxonNameInput {
                name: "wolf".into(),
                ..TaxonNameInput::default()
            }),
            ..TaxonInputRow::default()
        };
        let blocked = apply_rows(
            &database,
            std::slice::from_ref(&row),
            TaxonUpdateOptions::default(),
        )
        .unwrap();
        assert_eq!(blocked.batch_id, None);
        assert_eq!(blocked.rows[0].status, TaxonRowStatus::Conflict);

        let applied = apply_rows(
            &database,
            &[row],
            TaxonUpdateOptions {
                allow_new_names: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(applied.batch_id.is_some());
        let connection = database.connect().unwrap();
        let accepted: i64 = connection
            .query_row(
                "SELECT is_accepted FROM taxon_names WHERE name_kind = 3",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(accepted, 1);
    }

    #[test]
    fn supplements_empty_taxon_metadata_without_permissions() {
        let (_directory, database) = database();
        seed_lineage(&database);
        apply_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let row = TaxonInputRow {
            species: Some("Canis lupus".into()),
            geological_range: Some("Holocene".into()),
            ..TaxonInputRow::default()
        };
        let supplemented = apply_rows(
            &database,
            std::slice::from_ref(&row),
            TaxonUpdateOptions::default(),
        )
        .unwrap();
        assert!(supplemented.batch_id.is_some());
        assert_eq!(
            supplemented.rows[0].changes,
            vec![TaxonChange {
                kind: TaxonChangeKind::Supplement,
                field: "taxa.geological_range".into(),
                old_value: None,
                new_value: Some("Holocene".into()),
            }]
        );

        let overwrite = TaxonInputRow {
            species: Some("Canis lupus".into()),
            geological_range: Some("Pleistocene".into()),
            ..TaxonInputRow::default()
        };
        let blocked = apply_rows(
            &database,
            std::slice::from_ref(&overwrite),
            TaxonUpdateOptions::default(),
        )
        .unwrap();
        assert_eq!(blocked.rows[0].status, TaxonRowStatus::Conflict);
        let applied = apply_rows(
            &database,
            &[overwrite],
            TaxonUpdateOptions {
                allow_overwrite: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(applied.batch_id.is_some());
        let connection = database.connect().unwrap();
        let value: String = connection
            .query_row(
                "SELECT geological_range FROM taxa WHERE rank = 5",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(value, "Pleistocene");
    }

    #[test]
    fn supplements_empty_name_metadata_without_permissions() {
        let (_directory, database) = database();
        seed_lineage(&database);
        apply_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let supplemented = apply_rows(
            &database,
            &[TaxonInputRow {
                species: Some("Canis lupus".into()),
                scientific: Some(TaxonNameInput {
                    name: "Canis lupus".into(),
                    authority_year: Some("Linnaeus, 1758".into()),
                    category: Some("zoological".into()),
                    source: Some("local".into()),
                    ..TaxonNameInput::default()
                }),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions::default(),
        )
        .unwrap();
        assert!(supplemented.batch_id.is_some());
        assert!(
            supplemented.rows[0]
                .changes
                .iter()
                .all(|change| change.kind == TaxonChangeKind::Supplement)
        );
        let connection = database.connect().unwrap();
        let values: (String, String, String) = connection
            .query_row(
                r#"
                SELECT authority_year, category, source
                FROM taxon_names
                WHERE name_kind = 1 AND name = 'Canis lupus'
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            values,
            ("Linnaeus, 1758".into(), "zoological".into(), "local".into())
        );
        drop(connection);

        let conflict = apply_rows(
            &database,
            &[TaxonInputRow {
                species: Some("Canis lupus".into()),
                scientific: Some(TaxonNameInput {
                    name: "Canis lupus".into(),
                    source: Some("biolib".into()),
                    ..TaxonNameInput::default()
                }),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions::default(),
        )
        .unwrap();
        assert_eq!(conflict.rows[0].status, TaxonRowStatus::Conflict);
    }

    #[test]
    fn switches_the_accepted_name_atomically() {
        let (_directory, database) = database();
        seed_lineage(&database);
        apply_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let row = TaxonInputRow {
            species: Some("Canis lupus".into()),
            scientific: Some(TaxonNameInput {
                name: "Canis lycaon".into(),
                is_accepted: Some(true),
                ..TaxonNameInput::default()
            }),
            ..TaxonInputRow::default()
        };
        let blocked = apply_rows(
            &database,
            std::slice::from_ref(&row),
            TaxonUpdateOptions {
                allow_new_names: true,
                allow_overwrite: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert_eq!(blocked.rows[0].status, TaxonRowStatus::Conflict);
        let result = apply_rows(
            &database,
            &[row],
            TaxonUpdateOptions {
                allow_new_names: true,
                allow_switch_accepted_name: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(result.batch_id.is_some());
        let connection = database.connect().unwrap();
        let accepted: String = connection
            .query_row(
                "SELECT name FROM taxon_names WHERE name_kind = 1 AND is_accepted = 1 AND taxon_id = (SELECT taxon_id FROM taxon_names WHERE name_kind = 1 AND name = 'Canis lupus')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(accepted, "Canis lycaon");
    }

    #[test]
    fn switches_to_an_existing_name_but_never_demotes_without_a_replacement() {
        let (_directory, database) = database();
        seed_lineage(&database);
        apply_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        apply_rows(
            &database,
            &[TaxonInputRow {
                species: Some("Canis lupus".into()),
                scientific: Some(TaxonNameInput {
                    name: "Canis lycaon".into(),
                    is_accepted: Some(false),
                    ..TaxonNameInput::default()
                }),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_new_names: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let switched = apply_rows(
            &database,
            &[TaxonInputRow {
                species: Some("Canis lupus".into()),
                scientific: Some(TaxonNameInput {
                    name: "Canis lycaon".into(),
                    is_accepted: Some(true),
                    ..TaxonNameInput::default()
                }),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_switch_accepted_name: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(switched.batch_id.is_some());
        let demotion = apply_rows(
            &database,
            &[TaxonInputRow {
                species: Some("Canis lupus".into()),
                scientific: Some(TaxonNameInput {
                    name: "Canis lycaon".into(),
                    is_accepted: Some(false),
                    ..TaxonNameInput::default()
                }),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_switch_accepted_name: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert_eq!(demotion.rows[0].status, TaxonRowStatus::Conflict);
    }

    #[test]
    fn commits_valid_rows_when_another_row_is_blocked() {
        let (_directory, database) = database();
        seed_lineage(&database);
        let result = apply_rows(
            &database,
            &[
                species_row(),
                TaxonInputRow {
                    genus: Some("Vulpes".into()),
                    ..TaxonInputRow::default()
                },
            ],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(result.batch_id.is_some());
        assert_eq!(result.rows[0].status, TaxonRowStatus::Applied);
        assert_eq!(result.rows[1].status, TaxonRowStatus::Invalid);
        let connection = database.connect().unwrap();
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM taxa WHERE rank = 5", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
        let operations: i64 = connection
            .query_row("SELECT COUNT(*) FROM taxonomy_operations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(operations, 1);
    }

    #[test]
    fn does_not_create_empty_operation_batches() {
        let (_directory, database) = database();
        seed_lineage(&database);
        let result = apply_rows(
            &database,
            &[
                TaxonInputRow {
                    genus: Some("Canis".into()),
                    ..TaxonInputRow::default()
                },
                TaxonInputRow {
                    species: Some("Unknown species".into()),
                    ..TaxonInputRow::default()
                },
            ],
            TaxonUpdateOptions::default(),
        )
        .unwrap();
        assert_eq!(result.batch_id, None);
        assert_eq!(result.batch_id, None);
        assert_eq!(result.rows[0].status, TaxonRowStatus::NoChange);
        assert_eq!(result.rows[1].status, TaxonRowStatus::NotFound);
        let connection = database.connect().unwrap();
        let batch_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM taxonomy_operation_batches",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(batch_count, 0);
    }

    #[test]
    fn stores_batch_context_once_and_complete_reversible_changes_per_operation() {
        let (_directory, database) = database();
        seed_lineage(&database);
        let inputs = vec![
            species_row(),
            TaxonInputRow {
                genus: Some("Vulpes".into()),
                ..TaxonInputRow::default()
            },
        ];
        let options = TaxonUpdateOptions {
            allow_new_taxa: true,
            ..TaxonUpdateOptions::default()
        };
        let result = apply_rows(&database, &inputs, options).unwrap();
        let batch_id = result.batch_id.unwrap();
        let connection = database.connect().unwrap();
        let (context_json, input_json): (String, String) = connection
            .query_row(
                r#"
                SELECT context_json, input_json
                FROM taxonomy_operation_batches
                WHERE batch_id = ?
                "#,
                [batch_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let context = deserialize_json::<TaxonomyBatchContext>(&context_json, "context").unwrap();
        assert_eq!(context, TaxonomyBatchContext::BatchUpdate { options });
        assert_eq!(
            deserialize_json::<Vec<TaxonInputRow>>(&input_json, "inputs").unwrap(),
            inputs
        );
        drop(connection);

        let batches = list_taxonomy_operation_batches(&database, None, 10)
            .unwrap()
            .items;
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].batch_id, batch_id);
        assert_eq!(
            batches[0].context,
            TaxonomyBatchContext::BatchUpdate { options }
        );
        assert_eq!(
            serde_json::from_value::<Vec<TaxonInputRow>>(batches[0].input.clone()).unwrap(),
            inputs
        );

        let batch_operations = list_taxonomy_operations_for_batch(&database, batch_id, None, 10)
            .unwrap()
            .items;
        assert_eq!(batch_operations.len(), 1);
        assert_eq!(batch_operations[0].batch_id, batch_id);

        let operations = list_taxonomy_operations(&database, None, 10).unwrap().items;
        assert_eq!(operations.len(), 1);
        let operation = &operations[0];
        assert_eq!(operation.row_number, 1);
        assert!(operation.changeset_size > 0);
    }

    #[test]
    fn pages_taxonomy_operation_logs_with_cursors() {
        let (_directory, database) = database();
        seed_lineage(&database);
        let options = TaxonUpdateOptions {
            allow_new_taxa: true,
            ..TaxonUpdateOptions::default()
        };
        let first_batch = apply_rows(
            &database,
            &[
                species_row_named("Canis lupus"),
                species_row_named("Canis latrans"),
            ],
            options,
        )
        .unwrap()
        .batch_id
        .unwrap();
        let second_batch = apply_rows(&database, &[species_row_named("Canis rufus")], options)
            .unwrap()
            .batch_id
            .unwrap();
        let third_batch = apply_rows(&database, &[species_row_named("Canis simensis")], options)
            .unwrap()
            .batch_id
            .unwrap();

        let first_batch_page = list_taxonomy_operation_batches(&database, None, 2).unwrap();
        assert_eq!(first_batch_page.items.len(), 2);
        assert_eq!(first_batch_page.items[0].batch_id, third_batch);
        assert_eq!(first_batch_page.items[1].batch_id, second_batch);
        assert!(first_batch_page.next_cursor.is_some());
        let second_batch_page =
            list_taxonomy_operation_batches(&database, first_batch_page.next_cursor.as_deref(), 2)
                .unwrap();
        assert_eq!(second_batch_page.items.len(), 1);
        assert_eq!(second_batch_page.items[0].batch_id, first_batch);
        assert!(second_batch_page.next_cursor.is_none());

        let first_operation_page = list_taxonomy_operations(&database, None, 2).unwrap();
        assert_eq!(first_operation_page.items.len(), 2);
        assert!(
            first_operation_page.items[0].operation_id > first_operation_page.items[1].operation_id
        );
        assert!(first_operation_page.next_cursor.is_some());
        let second_operation_page =
            list_taxonomy_operations(&database, first_operation_page.next_cursor.as_deref(), 2)
                .unwrap();
        assert_eq!(second_operation_page.items.len(), 2);
        assert!(second_operation_page.next_cursor.is_none());

        let first_batch_operations =
            list_taxonomy_operations_for_batch(&database, first_batch, None, 1).unwrap();
        assert_eq!(first_batch_operations.items.len(), 1);
        assert_eq!(first_batch_operations.items[0].row_number, 1);
        assert!(first_batch_operations.next_cursor.is_some());
        let second_batch_operations = list_taxonomy_operations_for_batch(
            &database,
            first_batch,
            first_batch_operations.next_cursor.as_deref(),
            1,
        )
        .unwrap();
        assert_eq!(second_batch_operations.items.len(), 1);
        assert_eq!(second_batch_operations.items[0].row_number, 2);
        assert!(second_batch_operations.next_cursor.is_none());
    }

    #[test]
    fn reverts_operations_one_at_a_time() {
        let (_directory, database) = database();
        seed_lineage(&database);
        let created = apply_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let create_operation = created.rows[0].operation_id.unwrap();
        let appended = apply_rows(
            &database,
            &[TaxonInputRow {
                species: Some("Canis lupus".into()),
                chinese: Some(TaxonNameInput {
                    name: "wolf".into(),
                    ..TaxonNameInput::default()
                }),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_new_names: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let append_operation = appended.rows[0].operation_id.unwrap();

        revert_taxonomy_operation(&database, append_operation).unwrap();
        let connection = database.connect().unwrap();
        let chinese_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM taxon_names WHERE name_kind = 3",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(chinese_count, 0);
        drop(connection);

        revert_taxonomy_operation(&database, create_operation).unwrap();
        let connection = database.connect().unwrap();
        let species_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM taxa WHERE rank = 5", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(species_count, 0);
        let operations = list_taxonomy_operations(&database, None, 10).unwrap().items;
        assert_eq!(operations.len(), 2);
        assert!(
            operations
                .iter()
                .all(|operation| operation.status == TaxonomyOperationStatus::Reverted)
        );
    }

    #[test]
    fn reverts_an_accepted_name_switch_from_row_level_changes() {
        let (_directory, database) = database();
        seed_lineage(&database);
        apply_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        apply_rows(
            &database,
            &[TaxonInputRow {
                species: Some("Canis lupus".into()),
                scientific: Some(TaxonNameInput {
                    name: "Canis lycaon".into(),
                    is_accepted: Some(false),
                    ..TaxonNameInput::default()
                }),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_new_names: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let switched = apply_rows(
            &database,
            &[TaxonInputRow {
                species: Some("Canis lupus".into()),
                scientific: Some(TaxonNameInput {
                    name: "Canis lycaon".into(),
                    is_accepted: Some(true),
                    ..TaxonNameInput::default()
                }),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_switch_accepted_name: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let operation_id = switched.rows[0].operation_id.unwrap();
        let operation = list_taxonomy_operations(&database, None, 1)
            .unwrap()
            .items
            .remove(0);
        assert!(operation.changeset_size > 0);

        revert_taxonomy_operation(&database, operation_id).unwrap();
        let connection = database.connect().unwrap();
        let accepted: String = connection
            .query_row(
                "SELECT name FROM taxon_names WHERE name_kind = 1 AND is_accepted = 1 AND taxon_id = (SELECT taxon_id FROM taxon_names WHERE name_kind = 1 AND name = 'Canis lupus')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(accepted, "Canis lupus");
    }

    #[test]
    fn changeset_blocks_revert_when_an_affected_name_changes() {
        let (_directory, database) = database();
        seed_lineage(&database);
        apply_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let appended = apply_rows(
            &database,
            &[TaxonInputRow {
                species: Some("Canis lupus".into()),
                chinese: Some(TaxonNameInput {
                    name: "wolf".into(),
                    ..TaxonNameInput::default()
                }),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_new_names: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let operation_id = appended.rows[0].operation_id.unwrap();
        let connection = database.connect().unwrap();
        connection
            .execute(
                "UPDATE taxon_names SET source = 'later' WHERE name_kind = 3 AND name = 'wolf'",
                [],
            )
            .unwrap();
        drop(connection);

        let error = revert_taxonomy_operation(&database, operation_id).unwrap_err();
        assert!(error.to_string().contains("database error"));
    }

    #[test]
    fn refuses_to_revert_over_later_taxon_changes() {
        let (_directory, database) = database();
        seed_lineage(&database);
        apply_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let first = apply_rows(
            &database,
            &[TaxonInputRow {
                species: Some("Canis lupus".into()),
                geological_range: Some("Holocene".into()),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_overwrite: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let second = apply_rows(
            &database,
            &[TaxonInputRow {
                species: Some("Canis lupus".into()),
                geological_range: Some("Pleistocene".into()),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_overwrite: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let first_id = first.rows[0].operation_id.unwrap();
        let second_id = second.rows[0].operation_id.unwrap();
        let error = revert_taxonomy_operation(&database, first_id).unwrap_err();
        assert!(error.to_string().contains("database error"));
        revert_taxonomy_operation(&database, second_id).unwrap();
        revert_taxonomy_operation(&database, first_id).unwrap();
        let connection = database.connect().unwrap();
        let value: Option<String> = connection
            .query_row(
                "SELECT geological_range FROM taxa WHERE rank = 5",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(value, None);
    }
}
