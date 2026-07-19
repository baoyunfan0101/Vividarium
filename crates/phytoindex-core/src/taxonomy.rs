mod view;

pub use view::{
    TaxonBreadcrumbItem, TaxonDetail, TaxonDisplayNames, TaxonIdentifierDetail, TaxonNameDetail,
    TaxonNamesDetail, TaxonSummary, get_taxon_detail, get_taxon_summary,
};

use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use self::view::load_taxon_summary;
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

    fn as_str(self) -> &'static str {
        match self {
            Self::Kingdom => "kingdom",
            Self::Order => "order",
            Self::Family => "family",
            Self::Genus => "genus",
            Self::Species => "species",
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
pub enum TaxonomyOperationType {
    CreateTaxon,
    AppendName,
    UpdateMetadata,
    SwitchAcceptedName,
    Mixed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaxonomyNameKind {
    Scientific,
    English,
    Chinese,
}

impl TaxonomyNameKind {
    fn table(self) -> &'static str {
        match self {
            Self::Scientific => "scientific",
            Self::English => "english",
            Self::Chinese => "chinese",
        }
    }

    fn column(self) -> &'static str {
        match self {
            Self::Scientific => "scientific_name",
            Self::English => "english_name",
            Self::Chinese => "chinese_name",
        }
    }
}

impl TaxonomyOperationType {
    fn as_str(self) -> &'static str {
        match self {
            Self::CreateTaxon => "create_taxon",
            Self::AppendName => "append_name",
            Self::UpdateMetadata => "update_metadata",
            Self::SwitchAcceptedName => "switch_accepted_name",
            Self::Mixed => "mixed",
        }
    }

    fn from_str(value: &str) -> CoreResult<Self> {
        match value {
            "create_taxon" => Ok(Self::CreateTaxon),
            "append_name" => Ok(Self::AppendName),
            "update_metadata" => Ok(Self::UpdateMetadata),
            "switch_accepted_name" => Ok(Self::SwitchAcceptedName),
            "mixed" => Ok(Self::Mixed),
            _ => Err(CoreError::InvalidArgument(format!(
                "invalid taxonomy operation type: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonLogRecord {
    pub taxon_id: i64,
    pub parent_taxon_id: Option<i64>,
    pub rank: String,
    pub geological_range: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonNameLogRecord {
    pub taxon_id: i64,
    pub name: String,
    pub is_accepted: bool,
    pub authority_year: Option<String>,
    pub category: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum TaxonomyLogChange {
    TaxonInserted {
        after: TaxonLogRecord,
    },
    TaxonUpdated {
        before: TaxonLogRecord,
        after: TaxonLogRecord,
    },
    NameInserted {
        name_kind: TaxonomyNameKind,
        after: TaxonNameLogRecord,
    },
    NameUpdated {
        name_kind: TaxonomyNameKind,
        before: TaxonNameLogRecord,
        after: TaxonNameLogRecord,
    },
}

impl TaxonomyLogChange {
    fn taxon_id(&self) -> i64 {
        match self {
            Self::TaxonInserted { after } | Self::TaxonUpdated { after, .. } => after.taxon_id,
            Self::NameInserted { after, .. } | Self::NameUpdated { after, .. } => after.taxon_id,
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
    pub committed: bool,
    pub rows: Vec<TaxonRowOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyOperation {
    pub operation_id: i64,
    pub batch_id: i64,
    pub row_number: usize,
    pub operation_type: TaxonomyOperationType,
    pub status: String,
    pub changes: Vec<TaxonomyLogChange>,
    pub after_hash: String,
    pub applied_at: String,
    pub reverted_at: Option<String>,
}

pub fn preview_taxon_rows(
    database: &Database,
    rows: &[TaxonInputRow],
    options: TaxonUpdateOptions,
) -> CoreResult<TaxonBatchResult> {
    preview_rows(database, rows, options)
}

pub fn apply_taxon_rows(
    database: &Database,
    rows: &[TaxonInputRow],
    options: TaxonUpdateOptions,
) -> CoreResult<TaxonBatchResult> {
    apply_rows(database, rows, options)
}

fn preview_rows(
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
        committed: false,
        rows: outcomes,
    })
}

fn apply_rows(
    database: &Database,
    rows: &[TaxonInputRow],
    options: TaxonUpdateOptions,
) -> CoreResult<TaxonBatchResult> {
    let mut connection = database.connect()?;
    let mut batch_id = None;
    let mut outcomes = Vec::with_capacity(rows.len());
    let mut committed = false;
    for (index, row) in rows.iter().enumerate() {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let plan = match prepare_row(&transaction, row, options) {
            Ok(plan) => plan,
            Err(issue) => {
                transaction.rollback()?;
                outcomes.push(issue_outcome(index + 1, issue));
                continue;
            }
        };
        let before = match &plan.target {
            PlannedTarget::Existing(taxon_id) => taxon_snapshot(&transaction, *taxon_id)?,
            PlannedTarget::New { .. } => None,
        };
        let mut outcome = execute_plan(&transaction, index + 1, plan)?;
        if outcome.status == TaxonRowStatus::NoChange {
            transaction.rollback()?;
            outcomes.push(outcome);
            continue;
        }
        let taxon_id = outcome
            .target
            .as_ref()
            .map(|target| target.taxon_id)
            .ok_or_else(|| CoreError::InvalidArgument("applied operation has no target".into()))?;
        let after = taxon_snapshot(&transaction, taxon_id)?.ok_or_else(|| {
            CoreError::InvalidArgument("applied operation target no longer exists".into())
        })?;
        let log_changes = diff_taxon_snapshots(before.as_ref(), &after);
        let operation_type = operation_type(&outcome.changes);
        let after_hash = hash_affected_taxa(&transaction, &log_changes)?;
        let current_batch_id = match batch_id {
            Some(value) => value,
            None => {
                let value = insert_operation_batch(&transaction, rows, options)?;
                batch_id = Some(value);
                value
            }
        };
        let operation_id = insert_operation_log(
            &transaction,
            current_batch_id,
            index + 1,
            operation_type,
            &log_changes,
            &after_hash,
        )?;
        transaction.commit()?;
        outcome.operation_id = Some(operation_id);
        outcome.status = TaxonRowStatus::Applied;
        outcome.message = "applied".into();
        committed = true;
        outcomes.push(outcome);
    }
    Ok(TaxonBatchResult {
        batch_id,
        committed,
        rows: outcomes,
    })
}

pub fn list_taxonomy_operations(
    database: &Database,
    limit: usize,
) -> CoreResult<Vec<TaxonomyOperation>> {
    let connection = database.connect()?;
    let mut statement = connection.prepare(
        r#"
        SELECT operation_id, batch_id, row_number, operation_type, status,
               changes_json, after_hash, applied_at, reverted_at
        FROM taxonomy_operations
        ORDER BY operation_id DESC
        LIMIT ?
        "#,
    )?;
    let rows = statement.query_map([limit as i64], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, Option<String>>(8)?,
        ))
    })?;
    rows.map(|row| {
        let (
            operation_id,
            batch_id,
            row_number,
            operation_type,
            status,
            changes_json,
            after_hash,
            applied_at,
            reverted_at,
        ) = row?;
        Ok(TaxonomyOperation {
            operation_id,
            batch_id,
            row_number: row_number as usize,
            operation_type: TaxonomyOperationType::from_str(&operation_type)?,
            status,
            changes: deserialize_json(&changes_json, "operation changes")?,
            after_hash,
            applied_at,
            reverted_at,
        })
    })
    .collect()
}

pub fn revert_taxonomy_operation(
    database: &Database,
    operation_id: i64,
) -> CoreResult<TaxonomyOperation> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (status, changes_json, after_hash): (String, String, String) = transaction
        .query_row(
            r#"
            SELECT status, changes_json, after_hash
            FROM taxonomy_operations
            WHERE operation_id = ?
            "#,
            [operation_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound(format!("taxonomy operation {operation_id}")))?;
    if status != "applied" {
        return Err(CoreError::InvalidArgument(format!(
            "taxonomy operation {operation_id} is already {status}"
        )));
    }
    let changes: Vec<TaxonomyLogChange> = deserialize_json(&changes_json, "operation changes")?;
    if hash_affected_taxa(&transaction, &changes)? != after_hash {
        return Err(CoreError::InvalidArgument(format!(
            "taxonomy operation {operation_id} cannot be reverted because an affected taxon changed later"
        )));
    }
    revert_changes(&transaction, &changes, operation_id)?;
    transaction.execute(
        r#"
        UPDATE taxonomy_operations
        SET status = 'reverted', reverted_at = CURRENT_TIMESTAMP
        WHERE operation_id = ?
        "#,
        [operation_id],
    )?;
    transaction.commit()?;
    list_taxonomy_operations(database, usize::MAX)?
        .into_iter()
        .find(|operation| operation.operation_id == operation_id)
        .ok_or_else(|| CoreError::NotFound(format!("taxonomy operation {operation_id}")))
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
                normalize(row.kingdom.as_deref()),
                normalize(row.order.as_deref()),
                normalize(row.family.as_deref()),
                normalize(row.genus.as_deref()),
                normalize(row.species.as_deref()),
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

#[derive(Debug, Clone, Copy)]
enum NameKind {
    Scientific,
    English,
    Chinese,
}

impl NameKind {
    fn table(self) -> &'static str {
        match self {
            Self::Scientific => "scientific",
            Self::English => "english",
            Self::Chinese => "chinese",
        }
    }

    fn column(self) -> &'static str {
        match self {
            Self::Scientific => "scientific_name",
            Self::English => "english_name",
            Self::Chinese => "chinese_name",
        }
    }
}

#[derive(Debug)]
struct NamePlan {
    kind: NameKind,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct TaxonSnapshot {
    taxon_id: i64,
    parent_taxon_id: Option<i64>,
    rank: String,
    geological_range: Option<String>,
    scientific: Vec<NameSnapshot>,
    english: Vec<NameSnapshot>,
    chinese: Vec<NameSnapshot>,
    identifiers: Vec<IdentifierSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct NameSnapshot {
    name: String,
    is_accepted: bool,
    authority_year: Option<String>,
    category: Option<String>,
    source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct IdentifierSnapshot {
    source: String,
    external_id: String,
}

#[derive(Debug)]
struct NameFieldUpdate {
    field: NameField,
    value: String,
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
        1 => prepare_existing_taxon(transaction, row, options, candidates[0].taxon_id),
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
        (NameKind::English, row.english.as_ref()),
        (NameKind::Chinese, row.chinese.as_ref()),
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
        Some(input) if normalize(Some(&input.name)).as_deref() == Some(locator_name.as_str()) => {
            if input.is_accepted == Some(false) {
                return Err(RowIssue::new(
                    TaxonRowStatus::Conflict,
                    "a new taxon's only scientific name must be accepted",
                ));
            }
            names.push(insert_name_plan(
                NameKind::Scientific,
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
                NameKind::Scientific,
                locator_name,
                &TaxonNameInput::default(),
                !input_accepted,
                changes,
            )?);
            names.push(insert_name_plan(
                NameKind::Scientific,
                input_name,
                input,
                input_accepted,
                changes,
            )?);
        }
        None => names.push(insert_name_plan(
            NameKind::Scientific,
            locator_name,
            &TaxonNameInput::default(),
            true,
            changes,
        )?),
    }
    Ok(names)
}

fn new_first_name(
    kind: NameKind,
    input: &TaxonNameInput,
    changes: &mut Vec<TaxonChange>,
) -> Result<NamePlan, RowIssue> {
    if input.is_accepted == Some(false) {
        return Err(RowIssue::new(
            TaxonRowStatus::Conflict,
            format!("a new taxon's only {} name must be accepted", kind.table()),
        ));
    }
    insert_name_plan(kind, required_name(input)?, input, true, changes)
}

fn insert_name_plan(
    kind: NameKind,
    name: String,
    input: &TaxonNameInput,
    is_accepted: bool,
    changes: &mut Vec<TaxonChange>,
) -> Result<NamePlan, RowIssue> {
    if name.trim().is_empty() {
        return Err(RowIssue::new(
            TaxonRowStatus::Invalid,
            format!("{} name cannot be empty", kind.table()),
        ));
    }
    changes.push(TaxonChange {
        kind: TaxonChangeKind::AppendName,
        field: format!("{}.{}", kind.table(), kind.column()),
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
    row: &TaxonInputRow,
    options: TaxonUpdateOptions,
    taxon_id: i64,
) -> Result<RowPlan, RowIssue> {
    let mut changes = Vec::new();
    let geological_range = plan_geological_range(
        transaction,
        taxon_id,
        row.geological_range.as_deref(),
        options,
        &mut changes,
    )?;
    let mut names = Vec::new();
    for (kind, input) in [
        (NameKind::Scientific, row.scientific.as_ref()),
        (NameKind::English, row.english.as_ref()),
        (NameKind::Chinese, row.chinese.as_ref()),
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
    kind: NameKind,
    input: &TaxonNameInput,
    options: TaxonUpdateOptions,
    changes: &mut Vec<TaxonChange>,
) -> Result<NamePlan, RowIssue> {
    let name = required_name(input)?;
    let sql = format!(
        "SELECT is_accepted, authority_year, category, source FROM {} WHERE taxon_id = ? AND {} = ?",
        kind.table(),
        kind.column()
    );
    let existing = transaction
        .query_row(&sql, params![taxon_id, name], |row| {
            Ok(NameRecord {
                is_accepted: row.get::<_, i64>(0)? != 0,
                authority_year: row.get(1)?,
                category: row.get(2)?,
                source: row.get(3)?,
            })
        })
        .optional()
        .map_err(database_issue)?;
    let accepted_name = accepted_name(transaction, taxon_id, kind).map_err(database_issue)?;
    let total_names = count_names(transaction, taxon_id, kind).map_err(database_issue)?;
    let Some(existing) = existing else {
        if !options.allow_new_names {
            return Err(RowIssue::new(
                TaxonRowStatus::Conflict,
                format!("new {} names are not allowed", kind.table()),
            ));
        }
        let is_accepted = input.is_accepted.unwrap_or(total_names == 0);
        if !is_accepted && total_names == 0 {
            return Err(RowIssue::new(
                TaxonRowStatus::Conflict,
                format!("the first {} name must be accepted", kind.table()),
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
                            kind.table()
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
            field: format!("{}.{}", kind.table(), kind.column()),
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
                field: format!("{}.{}", kind.table(), field.as_str()),
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
                    kind.table(),
                    field.as_str()
                ),
            ));
        }
        changes.push(TaxonChange {
            kind: TaxonChangeKind::Overwrite,
            field: format!("{}.{}", kind.table(), field.as_str()),
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
                    kind.table()
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

fn accepted_change(kind: NameKind, old: Option<String>, new: String) -> TaxonChange {
    TaxonChange {
        kind: TaxonChangeKind::ChangeAcceptedName,
        field: format!("{}.is_accepted", kind.table()),
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
                params![parent_taxon_id, rank.as_str(), geological_range],
            )?;
            transaction.last_insert_rowid()
        }
    };
    for name in &plan.names {
        execute_name_plan(transaction, taxon_id, name)?;
    }
    for kind in [NameKind::Scientific, NameKind::English, NameKind::Chinese] {
        if plan
            .names
            .iter()
            .any(|name| name.kind.table() == kind.table())
        {
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
        let sql = format!(
            "UPDATE {} SET is_accepted = 0 WHERE taxon_id = ? AND {} = ?",
            plan.kind.table(),
            plan.kind.column()
        );
        transaction.execute(&sql, params![taxon_id, old_name])?;
    }
    if let Some(record) = plan.insert.as_ref() {
        let sql = format!(
            "INSERT INTO {} (taxon_id, {}, is_accepted, authority_year, category, source) VALUES (?, ?, ?, ?, ?, ?)",
            plan.kind.table(),
            plan.kind.column()
        );
        transaction.execute(
            &sql,
            params![
                taxon_id,
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
            "UPDATE {} SET {} = ? WHERE taxon_id = ? AND {} = ?",
            plan.kind.table(),
            update.field.as_str(),
            plan.kind.column()
        );
        transaction.execute(&sql, params![update.value, taxon_id, plan.name])?;
    }
    if plan.promote {
        let sql = format!(
            "UPDATE {} SET is_accepted = 1 WHERE taxon_id = ? AND {} = ?",
            plan.kind.table(),
            plan.kind.column()
        );
        transaction.execute(&sql, params![taxon_id, plan.name])?;
    }
    Ok(())
}

fn insert_operation_batch(
    transaction: &Transaction<'_>,
    inputs: &[TaxonInputRow],
    options: TaxonUpdateOptions,
) -> CoreResult<i64> {
    let input_json = serialize_json(inputs, "taxonomy inputs")?;
    let options_json = serialize_json(&options, "taxonomy options")?;
    transaction.execute(
        r#"
        INSERT INTO taxonomy_operation_batches (options_json, input_json)
        VALUES (?, ?)
        "#,
        params![options_json, input_json],
    )?;
    Ok(transaction.last_insert_rowid())
}

fn insert_operation_log(
    transaction: &Transaction<'_>,
    batch_id: i64,
    row_number: usize,
    operation_type: TaxonomyOperationType,
    changes: &[TaxonomyLogChange],
    after_hash: &str,
) -> CoreResult<i64> {
    let changes_json = serialize_json(changes, "taxonomy changes")?;
    transaction.execute(
        r#"
        INSERT INTO taxonomy_operations (
            batch_id, row_number, operation_type, status, changes_json, after_hash
        ) VALUES (?, ?, ?, 'applied', ?, ?)
        "#,
        params![
            batch_id,
            row_number as i64,
            operation_type.as_str(),
            changes_json,
            after_hash,
        ],
    )?;
    Ok(transaction.last_insert_rowid())
}

fn taxon_snapshot(
    transaction: &Transaction<'_>,
    taxon_id: i64,
) -> CoreResult<Option<TaxonSnapshot>> {
    let base = transaction
        .query_row(
            r#"
            SELECT parent_taxon_id, rank, geological_range
            FROM taxa
            WHERE taxon_id = ?
            "#,
            [taxon_id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((parent_taxon_id, rank, geological_range)) = base else {
        return Ok(None);
    };
    let mut identifiers_statement = transaction.prepare(
        r#"
        SELECT source, external_id
        FROM taxon_identifiers
        WHERE taxon_id = ?
        ORDER BY source, external_id
        "#,
    )?;
    let identifiers = identifiers_statement
        .query_map([taxon_id], |row| {
            Ok(IdentifierSnapshot {
                source: row.get(0)?,
                external_id: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(TaxonSnapshot {
        taxon_id,
        parent_taxon_id,
        rank,
        geological_range,
        scientific: name_snapshots(transaction, taxon_id, NameKind::Scientific)?,
        english: name_snapshots(transaction, taxon_id, NameKind::English)?,
        chinese: name_snapshots(transaction, taxon_id, NameKind::Chinese)?,
        identifiers,
    }))
}

fn name_snapshots(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: NameKind,
) -> CoreResult<Vec<NameSnapshot>> {
    let sql = format!(
        "SELECT {}, is_accepted, authority_year, category, source FROM {} WHERE taxon_id = ? ORDER BY {}",
        kind.column(),
        kind.table(),
        kind.column()
    );
    let mut statement = transaction.prepare(&sql)?;
    let rows = statement.query_map([taxon_id], |row| {
        Ok(NameSnapshot {
            name: row.get(0)?,
            is_accepted: row.get::<_, i64>(1)? != 0,
            authority_year: row.get(2)?,
            category: row.get(3)?,
            source: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn operation_type(changes: &[TaxonChange]) -> TaxonomyOperationType {
    if changes
        .iter()
        .any(|change| change.kind == TaxonChangeKind::CreateTaxon)
    {
        return TaxonomyOperationType::CreateTaxon;
    }
    let switches_name = changes
        .iter()
        .any(|change| change.kind == TaxonChangeKind::ChangeAcceptedName);
    let appends_name = changes
        .iter()
        .any(|change| change.kind == TaxonChangeKind::AppendName);
    let updates_metadata = changes.iter().any(|change| {
        matches!(
            change.kind,
            TaxonChangeKind::Supplement | TaxonChangeKind::Overwrite
        )
    });
    match (switches_name, appends_name, updates_metadata) {
        (true, _, false) => TaxonomyOperationType::SwitchAcceptedName,
        (false, true, false) => TaxonomyOperationType::AppendName,
        (false, false, true) => TaxonomyOperationType::UpdateMetadata,
        _ => TaxonomyOperationType::Mixed,
    }
}

fn diff_taxon_snapshots(
    before: Option<&TaxonSnapshot>,
    after: &TaxonSnapshot,
) -> Vec<TaxonomyLogChange> {
    let mut changes = Vec::new();
    let after_taxon = taxon_log_record(after);
    match before {
        Some(before) => {
            let before_taxon = taxon_log_record(before);
            if before_taxon != after_taxon {
                changes.push(TaxonomyLogChange::TaxonUpdated {
                    before: before_taxon,
                    after: after_taxon,
                });
            }
            for (kind, before_names, after_names) in [
                (
                    TaxonomyNameKind::Scientific,
                    before.scientific.as_slice(),
                    after.scientific.as_slice(),
                ),
                (
                    TaxonomyNameKind::English,
                    before.english.as_slice(),
                    after.english.as_slice(),
                ),
                (
                    TaxonomyNameKind::Chinese,
                    before.chinese.as_slice(),
                    after.chinese.as_slice(),
                ),
            ] {
                diff_names(
                    after.taxon_id,
                    kind,
                    before_names,
                    after_names,
                    &mut changes,
                );
            }
        }
        None => {
            changes.push(TaxonomyLogChange::TaxonInserted { after: after_taxon });
            for (kind, names) in [
                (TaxonomyNameKind::Scientific, after.scientific.as_slice()),
                (TaxonomyNameKind::English, after.english.as_slice()),
                (TaxonomyNameKind::Chinese, after.chinese.as_slice()),
            ] {
                for name in names {
                    changes.push(TaxonomyLogChange::NameInserted {
                        name_kind: kind,
                        after: name_log_record(after.taxon_id, name),
                    });
                }
            }
        }
    }
    changes
}

fn diff_names(
    taxon_id: i64,
    kind: TaxonomyNameKind,
    before: &[NameSnapshot],
    after: &[NameSnapshot],
    changes: &mut Vec<TaxonomyLogChange>,
) {
    for after_name in after {
        match before.iter().find(|before| before.name == after_name.name) {
            Some(before_name) if before_name != after_name => {
                changes.push(TaxonomyLogChange::NameUpdated {
                    name_kind: kind,
                    before: name_log_record(taxon_id, before_name),
                    after: name_log_record(taxon_id, after_name),
                });
            }
            None => changes.push(TaxonomyLogChange::NameInserted {
                name_kind: kind,
                after: name_log_record(taxon_id, after_name),
            }),
            Some(_) => {}
        }
    }
}

fn taxon_log_record(snapshot: &TaxonSnapshot) -> TaxonLogRecord {
    TaxonLogRecord {
        taxon_id: snapshot.taxon_id,
        parent_taxon_id: snapshot.parent_taxon_id,
        rank: snapshot.rank.clone(),
        geological_range: snapshot.geological_range.clone(),
    }
}

fn name_log_record(taxon_id: i64, snapshot: &NameSnapshot) -> TaxonNameLogRecord {
    TaxonNameLogRecord {
        taxon_id,
        name: snapshot.name.clone(),
        is_accepted: snapshot.is_accepted,
        authority_year: snapshot.authority_year.clone(),
        category: snapshot.category.clone(),
        source: snapshot.source.clone(),
    }
}

fn hash_affected_taxa(
    transaction: &Transaction<'_>,
    changes: &[TaxonomyLogChange],
) -> CoreResult<String> {
    let taxon_ids = changes
        .iter()
        .map(TaxonomyLogChange::taxon_id)
        .collect::<BTreeSet<_>>();
    if taxon_ids.is_empty() {
        return Err(CoreError::InvalidArgument(
            "taxonomy operation has no affected taxa".into(),
        ));
    }
    let snapshots = taxon_ids
        .into_iter()
        .map(|taxon_id| Ok((taxon_id, taxon_snapshot(transaction, taxon_id)?)))
        .collect::<CoreResult<Vec<_>>>()?;
    let json = serialize_json(&snapshots, "affected taxa")?;
    Ok(format!("{:x}", Sha256::digest(json.as_bytes())))
}

fn revert_changes(
    transaction: &Transaction<'_>,
    changes: &[TaxonomyLogChange],
    operation_id: i64,
) -> CoreResult<()> {
    for change in changes.iter().rev() {
        if let TaxonomyLogChange::NameInserted { name_kind, after } = change {
            let sql = format!(
                "DELETE FROM {} WHERE taxon_id = ? AND {} = ?",
                name_kind.table(),
                name_kind.column()
            );
            transaction.execute(&sql, params![after.taxon_id, after.name])?;
        }
    }
    for change in changes {
        if let TaxonomyLogChange::NameUpdated {
            name_kind,
            before,
            after,
        } = change
            && before.is_accepted != after.is_accepted
        {
            let sql = format!(
                "UPDATE {} SET is_accepted = 0 WHERE taxon_id = ? AND {} = ?",
                name_kind.table(),
                name_kind.column()
            );
            transaction.execute(&sql, params![after.taxon_id, after.name])?;
        }
    }
    for change in changes.iter().rev() {
        match change {
            TaxonomyLogChange::NameUpdated {
                name_kind, before, ..
            } => restore_name_record(transaction, *name_kind, before)?,
            TaxonomyLogChange::TaxonUpdated { before, .. } => {
                transaction.execute(
                    r#"
                    UPDATE taxa
                    SET parent_taxon_id = ?, rank = ?, geological_range = ?
                    WHERE taxon_id = ?
                    "#,
                    params![
                        before.parent_taxon_id,
                        before.rank,
                        before.geological_range,
                        before.taxon_id,
                    ],
                )?;
            }
            TaxonomyLogChange::TaxonInserted { after } => {
                delete_created_taxon(transaction, after.taxon_id, operation_id)?;
            }
            TaxonomyLogChange::NameInserted { .. } => {}
        }
    }
    Ok(())
}

fn restore_name_record(
    transaction: &Transaction<'_>,
    kind: TaxonomyNameKind,
    record: &TaxonNameLogRecord,
) -> CoreResult<()> {
    let sql = format!(
        "UPDATE {} SET is_accepted = ?, authority_year = ?, category = ?, source = ? WHERE taxon_id = ? AND {} = ?",
        kind.table(),
        kind.column()
    );
    transaction.execute(
        &sql,
        params![
            i64::from(record.is_accepted),
            record.authority_year,
            record.category,
            record.source,
            record.taxon_id,
            record.name,
        ],
    )?;
    Ok(())
}

fn delete_created_taxon(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    operation_id: i64,
) -> CoreResult<()> {
    let child_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM taxa WHERE parent_taxon_id = ?",
        [taxon_id],
        |row| row.get(0),
    )?;
    let photo_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM photos_taxa_mapping WHERE taxon_id = ?",
        [taxon_id],
        |row| row.get(0),
    )?;
    if child_count > 0 || photo_count > 0 {
        return Err(CoreError::InvalidArgument(format!(
            "taxonomy operation {operation_id} cannot be reverted because the created taxon is in use"
        )));
    }
    transaction.execute("DELETE FROM taxa WHERE taxon_id = ?", [taxon_id])?;
    Ok(())
}

fn serialize_json<T: Serialize + ?Sized>(value: &T, label: &str) -> CoreResult<String> {
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
    kind: NameKind,
) -> CoreResult<()> {
    let sql = format!(
        "SELECT COUNT(*), COALESCE(SUM(is_accepted), 0) FROM {} WHERE taxon_id = ?",
        kind.table()
    );
    let (total, accepted): (i64, i64) =
        transaction.query_row(&sql, [taxon_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    if total > 0 && accepted != 1 {
        return Err(CoreError::InvalidArgument(format!(
            "{} names must have exactly one accepted value for taxon {taxon_id}",
            kind.table()
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
        JOIN scientific ON scientific.taxon_id = taxa.taxon_id
        WHERE taxa.rank = ? AND scientific.scientific_name = ? COLLATE BINARY
        ORDER BY taxa.taxon_id
        "#,
    )?;
    let rows = statement.query_map(params![target_rank.as_str(), target_name], |row| {
        row.get::<_, i64>(0)
    })?;
    let ids = rows.collect::<Result<Vec<_>, _>>()?;
    let had_name_match = !ids.is_empty();
    let mut candidates = Vec::new();
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
            let candidate = load_taxon_summary(transaction, taxon_id)?.ok_or_else(|| {
                CoreError::InvalidArgument(format!("candidate taxon {taxon_id} no longer exists"))
            })?;
            candidates.push(candidate);
        }
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
            JOIN scientific ON scientific.taxon_id = lineage.taxon_id
            WHERE lineage.rank = ? AND scientific.scientific_name = ? COLLATE BINARY
        )
        "#,
        params![taxon_id, rank.as_str(), name],
        |row| row.get(0),
    )?)
}

fn accepted_name(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: NameKind,
) -> rusqlite::Result<Option<String>> {
    let sql = format!(
        "SELECT {} FROM {} WHERE taxon_id = ? AND is_accepted = 1",
        kind.column(),
        kind.table()
    );
    transaction
        .query_row(&sql, [taxon_id], |row| row.get(0))
        .optional()
}

fn count_names(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: NameKind,
) -> rusqlite::Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {} WHERE taxon_id = ?", kind.table());
    transaction.query_row(&sql, [taxon_id], |row| row.get(0))
}

fn required_name(input: &TaxonNameInput) -> Result<String, RowIssue> {
    normalize(Some(&input.name)).ok_or_else(|| {
        RowIssue::new(
            TaxonRowStatus::Invalid,
            "a supplied name group must include a non-empty name",
        )
    })
}

fn normalize(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parse_rank(value: &str) -> CoreResult<TaxonRank> {
    match value {
        "kingdom" => Ok(TaxonRank::Kingdom),
        "order" => Ok(TaxonRank::Order),
        "family" => Ok(TaxonRank::Family),
        "genus" => Ok(TaxonRank::Genus),
        "species" => Ok(TaxonRank::Species),
        value => Err(CoreError::InvalidArgument(format!(
            "invalid taxon rank: {value}"
        ))),
    }
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
            ("kingdom", "Animalia"),
            ("order", "Carnivora"),
            ("family", "Canidae"),
            ("genus", "Canis"),
        ]
        .into_iter()
        .enumerate()
        {
            transaction
                .execute(
                    "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, ?)",
                    params![parent, rank],
                )
                .unwrap();
            let id = transaction.last_insert_rowid();
            transaction
                .execute(
                    "INSERT INTO scientific (taxon_id, scientific_name, is_accepted) VALUES (?, ?, 1)",
                    params![id, name],
                )
                .unwrap();
            ids[index] = id;
            parent = Some(id);
        }
        transaction.commit().unwrap();
        ids
    }

    fn species_row() -> TaxonInputRow {
        TaxonInputRow {
            species: Some("Canis lupus".into()),
            ..TaxonInputRow::default()
        }
    }

    #[test]
    fn creates_a_species_and_derives_its_genus() {
        let (_directory, database) = database();
        let ids = seed_lineage(&database);
        let result = apply_taxon_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(result.committed);
        assert_eq!(result.rows[0].status, TaxonRowStatus::Applied);
        let connection = database.connect().unwrap();
        let parent: i64 = connection
            .query_row(
                "SELECT parent_taxon_id FROM taxa WHERE rank = 'species'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(parent, ids[3]);
        let accepted: i64 = connection
            .query_row(
                "SELECT is_accepted FROM scientific WHERE scientific_name = 'Canis lupus'",
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
        let created = apply_taxon_rows(
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
                INSERT INTO scientific (
                    taxon_id, scientific_name, is_accepted, category, source
                ) VALUES (?, 'Canis lycaon', 0, 'synonym', 'local')
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
    fn requires_the_immediate_parent_for_non_species_taxa() {
        let (_directory, database) = database();
        let result = preview_taxon_rows(
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
        let result = preview_taxon_rows(
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
                "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, 'species')",
                [ids[3]],
            )
            .unwrap();
        let first = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO scientific (taxon_id, scientific_name, is_accepted) VALUES (?, 'Canis lupus', 1)",
                [first],
            )
            .unwrap();
        connection
            .execute("INSERT INTO taxa (rank) VALUES ('genus')", [])
            .unwrap();
        let other_genus = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO scientific (taxon_id, scientific_name, is_accepted) VALUES (?, 'Canis', 1)",
                [other_genus],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, 'species')",
                [other_genus],
            )
            .unwrap();
        let second = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO scientific (taxon_id, scientific_name, is_accepted) VALUES (?, 'Canis lupus', 1)",
                [second],
            )
            .unwrap();
        drop(connection);

        let ambiguous =
            preview_taxon_rows(&database, &[species_row()], TaxonUpdateOptions::default()).unwrap();
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

        let filtered = preview_taxon_rows(
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
                "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, 'species')",
                [ids[3]],
            )
            .unwrap();
        let species_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO scientific (taxon_id, scientific_name, is_accepted) VALUES (?, 'Canis lupus', 1)",
                [species_id],
            )
            .unwrap();
        drop(connection);

        let result = apply_taxon_rows(
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
        assert!(!result.committed);
        assert_eq!(result.rows[0].status, TaxonRowStatus::Conflict);
        let connection = database.connect().unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM scientific WHERE scientific_name = 'Canis lupus'",
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
        apply_taxon_rows(
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
        let blocked = apply_taxon_rows(
            &database,
            std::slice::from_ref(&row),
            TaxonUpdateOptions::default(),
        )
        .unwrap();
        assert!(!blocked.committed);
        assert_eq!(blocked.rows[0].status, TaxonRowStatus::Conflict);

        let applied = apply_taxon_rows(
            &database,
            &[row],
            TaxonUpdateOptions {
                allow_new_names: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(applied.committed);
        let connection = database.connect().unwrap();
        let accepted: i64 = connection
            .query_row("SELECT is_accepted FROM chinese", [], |row| row.get(0))
            .unwrap();
        assert_eq!(accepted, 1);
    }

    #[test]
    fn supplements_empty_taxon_metadata_without_permissions() {
        let (_directory, database) = database();
        seed_lineage(&database);
        apply_taxon_rows(
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
        let supplemented = apply_taxon_rows(
            &database,
            std::slice::from_ref(&row),
            TaxonUpdateOptions::default(),
        )
        .unwrap();
        assert!(supplemented.committed);
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
        let blocked = apply_taxon_rows(
            &database,
            std::slice::from_ref(&overwrite),
            TaxonUpdateOptions::default(),
        )
        .unwrap();
        assert_eq!(blocked.rows[0].status, TaxonRowStatus::Conflict);
        let applied = apply_taxon_rows(
            &database,
            &[overwrite],
            TaxonUpdateOptions {
                allow_overwrite: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(applied.committed);
        let connection = database.connect().unwrap();
        let value: String = connection
            .query_row(
                "SELECT geological_range FROM taxa WHERE rank = 'species'",
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
        apply_taxon_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let supplemented = apply_taxon_rows(
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
        assert!(supplemented.committed);
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
                FROM scientific
                WHERE scientific_name = 'Canis lupus'
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

        let conflict = apply_taxon_rows(
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
        apply_taxon_rows(
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
        let blocked = apply_taxon_rows(
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
        let result = apply_taxon_rows(
            &database,
            &[row],
            TaxonUpdateOptions {
                allow_new_names: true,
                allow_switch_accepted_name: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(result.committed);
        let connection = database.connect().unwrap();
        let accepted: String = connection
            .query_row(
                "SELECT scientific_name FROM scientific WHERE is_accepted = 1 AND taxon_id = (SELECT taxon_id FROM scientific WHERE scientific_name = 'Canis lupus')",
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
        apply_taxon_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        apply_taxon_rows(
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
        let switched = apply_taxon_rows(
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
        assert!(switched.committed);
        let demotion = apply_taxon_rows(
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
        let result = apply_taxon_rows(
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
        assert!(result.committed);
        assert_eq!(result.rows[0].status, TaxonRowStatus::Applied);
        assert_eq!(result.rows[1].status, TaxonRowStatus::Invalid);
        let connection = database.connect().unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM taxa WHERE rank = 'species'",
                [],
                |row| row.get(0),
            )
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
        let result = apply_taxon_rows(
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
        assert!(!result.committed);
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
        let result = apply_taxon_rows(&database, &inputs, options).unwrap();
        let batch_id = result.batch_id.unwrap();
        let connection = database.connect().unwrap();
        let (options_json, input_json): (String, String) = connection
            .query_row(
                r#"
                SELECT options_json, input_json
                FROM taxonomy_operation_batches
                WHERE batch_id = ?
                "#,
                [batch_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            deserialize_json::<TaxonUpdateOptions>(&options_json, "options").unwrap(),
            options
        );
        assert_eq!(
            deserialize_json::<Vec<TaxonInputRow>>(&input_json, "inputs").unwrap(),
            inputs
        );
        drop(connection);

        let operations = list_taxonomy_operations(&database, 10).unwrap();
        assert_eq!(operations.len(), 1);
        let operation = &operations[0];
        assert_eq!(operation.row_number, 1);
        assert_eq!(operation.operation_type, TaxonomyOperationType::CreateTaxon);
        assert_eq!(operation.after_hash.len(), 64);
        assert!(
            operation
                .changes
                .iter()
                .any(|change| matches!(change, TaxonomyLogChange::TaxonInserted { .. }))
        );
        assert!(operation.changes.iter().any(|change| matches!(
            change,
            TaxonomyLogChange::NameInserted {
                name_kind: TaxonomyNameKind::Scientific,
                ..
            }
        )));
    }

    #[test]
    fn reverts_operations_one_at_a_time() {
        let (_directory, database) = database();
        seed_lineage(&database);
        let created = apply_taxon_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let create_operation = created.rows[0].operation_id.unwrap();
        let appended = apply_taxon_rows(
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

        let reverted = revert_taxonomy_operation(&database, append_operation).unwrap();
        assert_eq!(reverted.status, "reverted");
        let connection = database.connect().unwrap();
        let chinese_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM chinese", [], |row| row.get(0))
            .unwrap();
        assert_eq!(chinese_count, 0);
        drop(connection);

        revert_taxonomy_operation(&database, create_operation).unwrap();
        let connection = database.connect().unwrap();
        let species_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM taxa WHERE rank = 'species'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(species_count, 0);
        let operations = list_taxonomy_operations(&database, 10).unwrap();
        assert_eq!(operations.len(), 2);
        assert!(
            operations
                .iter()
                .all(|operation| operation.status == "reverted")
        );
    }

    #[test]
    fn reverts_an_accepted_name_switch_from_row_level_changes() {
        let (_directory, database) = database();
        seed_lineage(&database);
        apply_taxon_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        apply_taxon_rows(
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
        let switched = apply_taxon_rows(
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
        let operation = list_taxonomy_operations(&database, 1).unwrap().remove(0);
        assert_eq!(
            operation.operation_type,
            TaxonomyOperationType::SwitchAcceptedName
        );
        assert_eq!(
            operation
                .changes
                .iter()
                .filter(|change| matches!(change, TaxonomyLogChange::NameUpdated { .. }))
                .count(),
            2
        );

        revert_taxonomy_operation(&database, operation_id).unwrap();
        let connection = database.connect().unwrap();
        let accepted: String = connection
            .query_row(
                "SELECT scientific_name FROM scientific WHERE is_accepted = 1 AND taxon_id = (SELECT taxon_id FROM scientific WHERE scientific_name = 'Canis lupus')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(accepted, "Canis lupus");
    }

    #[test]
    fn after_hash_blocks_revert_when_an_unrelated_taxon_field_changes() {
        let (_directory, database) = database();
        seed_lineage(&database);
        apply_taxon_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let appended = apply_taxon_rows(
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
                "UPDATE taxa SET geological_range = 'Holocene' WHERE rank = 'species'",
                [],
            )
            .unwrap();
        drop(connection);

        let error = revert_taxonomy_operation(&database, operation_id).unwrap_err();
        assert!(error.to_string().contains("affected taxon changed later"));
    }

    #[test]
    fn refuses_to_revert_over_later_taxon_changes() {
        let (_directory, database) = database();
        seed_lineage(&database);
        apply_taxon_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let first = apply_taxon_rows(
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
        let second = apply_taxon_rows(
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
        assert!(error.to_string().contains("changed later"));
        revert_taxonomy_operation(&database, second_id).unwrap();
        revert_taxonomy_operation(&database, first_id).unwrap();
        let connection = database.connect().unwrap();
        let value: Option<String> = connection
            .query_row(
                "SELECT geological_range FROM taxa WHERE rank = 'species'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(value, None);
    }
}
