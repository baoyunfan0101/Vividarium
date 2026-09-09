//! Formatted taxonomy input, preview, apply, operation history, and rollback.

use std::collections::HashSet;
use std::io::Cursor;

use csv::{ReaderBuilder, WriterBuilder};
use rusqlite::session::ConflictAction;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::changeset::{
    affected_taxon_ids_from_changeset, apply_inverse_taxonomy_changeset, is_taxonomy_session_table,
    start_taxonomy_session, validate_foreign_key_integrity,
};
use super::match_exact_taxonomy_name;
use super::types::{TaxonRank, TaxonomyNameType};
use super::validation::{normalize_name, validate_taxonomy};
use super::view::{TaxonSummary, load_taxon_summaries, load_taxon_summary};
use crate::naming::SynonymAuthorityParser;
use crate::operations::{
    self, NewAuditRow, NewOperation, OperationAuditRow, OperationInput, OperationPage,
    OperationSummary,
};
use crate::{CancellationToken, CoreError, CoreResult, Database};

pub const TAXONOMY_INPUT_COLUMNS: [&str; 13] = [
    "kingdom",
    "order",
    "family",
    "genus",
    "species",
    "authority_year",
    "synonyms",
    "zh_name",
    "zh_alias",
    "en_name",
    "en_alias",
    "geological_range",
    "source",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TaxonInputRow {
    pub kingdom: Option<String>,
    pub order: Option<String>,
    pub family: Option<String>,
    pub genus: Option<String>,
    pub species: Option<String>,
    pub authority_year: Option<String>,
    pub synonyms: Vec<String>,
    pub zh_name: Option<String>,
    pub zh_alias: Vec<String>,
    pub en_name: Option<String>,
    pub en_alias: Vec<String>,
    pub geological_range: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaxonRowStatus {
    NoChange,
    Supplement,
    NewName,
    NewTaxon,
    Overwrite,
    Invalid,
    NotMatched,
    MultipleCandidates,
}

impl TaxonRowStatus {
    fn is_failure(self) -> bool {
        matches!(
            self,
            Self::Invalid | Self::NotMatched | Self::MultipleCandidates
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaxonChangeKind {
    CreateTaxon,
    AppendName,
    Supplement,
    Overwrite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonChange {
    pub kind: TaxonChangeKind,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonRowOutcome {
    pub row_number: usize,
    pub operation_types: Vec<TaxonRowStatus>,
    pub message: String,
    pub target: Option<TaxonSummary>,
    pub parent: Option<TaxonSummary>,
    pub candidates: Vec<TaxonSummary>,
    pub changes: Vec<TaxonChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyPreviewResult {
    pub delimiter: String,
    pub encoding: String,
    pub rows: Vec<TaxonRowOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyOperationResult {
    pub operation_id: i64,
    pub total_rows: usize,
    pub succeeded_rows: usize,
    pub failed_rows: usize,
    pub delimiter: String,
    pub encoding: String,
    pub rows: Vec<TaxonRowOutcome>,
}

#[derive(Debug)]
pub struct PreparedTaxonomyUpdate {
    rows: Vec<TaxonInputRow>,
    preview: TaxonomyPreviewResult,
    changeset_blob: Vec<u8>,
    revision: TaxonomyRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TaxonomyRevision {
    taxonomy_identity: String,
    latest_operation_id: i64,
    operation_count: i64,
}

impl PreparedTaxonomyUpdate {
    pub fn preview_result(&self) -> &TaxonomyPreviewResult {
        &self.preview
    }
}

pub fn preview_rows(
    database: &Database,
    rows: &[TaxonInputRow],
) -> CoreResult<TaxonomyPreviewResult> {
    Ok(prepare_rows(database, rows)?.preview)
}

pub fn prepare_rows(
    database: &Database,
    rows: &[TaxonInputRow],
) -> CoreResult<PreparedTaxonomyUpdate> {
    prepare_rows_with_cancellation(database, rows, &CancellationToken::new())
}

pub fn prepare_rows_with_cancellation(
    database: &Database,
    rows: &[TaxonInputRow],
    cancellation: &CancellationToken,
) -> CoreResult<PreparedTaxonomyUpdate> {
    cancellation.check()?;
    let delimiter = crate::general::get_csv_delimiter(database)?;
    let _guard = database.try_taxonomy_mutation()?;
    let mut connection = database.connect_taxonomy_metadata_context()?;
    cancellation.install_sqlite_progress_handler(&connection);
    let result = (|| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let revision = taxonomy_revision(&transaction)?;
        let mut session = start_taxonomy_session(&transaction)?;
        let outcomes = process_rows(&transaction, rows, cancellation)?;
        validate_taxonomy(&transaction)?;
        cancellation.check()?;
        let mut changeset_blob = Vec::new();
        session.changeset_strm(&mut changeset_blob)?;
        drop(session);
        transaction.rollback()?;
        Ok(PreparedTaxonomyUpdate {
            rows: rows.to_vec(),
            preview: TaxonomyPreviewResult {
                delimiter,
                encoding: "UTF-8".into(),
                rows: outcomes,
            },
            changeset_blob,
            revision,
        })
    })();
    cancellation.normalize(result)
}

pub fn apply_rows(
    database: &Database,
    rows: &[TaxonInputRow],
) -> CoreResult<TaxonomyOperationResult> {
    let delimiter = crate::general::get_csv_delimiter(database)?;
    let _guard = database.try_taxonomy_mutation()?;
    let mut connection = database.connect_taxonomy_metadata_context()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut session = start_taxonomy_session(&transaction)?;
    let outcomes = process_rows(&transaction, rows, &CancellationToken::new())?;
    validate_taxonomy(&transaction)?;
    let mut changeset_blob = Vec::new();
    session.changeset_strm(&mut changeset_blob)?;
    drop(session);

    let result = store_applied_rows(&transaction, rows, outcomes, changeset_blob, delimiter)?;
    transaction.commit()?;
    Ok(result)
}

pub fn apply_prepared_rows(
    database: &Database,
    prepared: PreparedTaxonomyUpdate,
) -> CoreResult<TaxonomyOperationResult> {
    apply_prepared_rows_with_cancellation(database, prepared, &CancellationToken::new())
}

pub fn apply_prepared_rows_with_cancellation(
    database: &Database,
    prepared: PreparedTaxonomyUpdate,
    cancellation: &CancellationToken,
) -> CoreResult<TaxonomyOperationResult> {
    cancellation.check()?;
    let delimiter = prepared.preview.delimiter.clone();
    let _guard = database.try_taxonomy_mutation()?;
    let mut connection = database.connect_taxonomy_metadata_context()?;
    cancellation.install_sqlite_progress_handler(&connection);
    let result = (|| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if taxonomy_revision(&transaction)? != prepared.revision {
            return Err(CoreError::InvalidArgument(
                "formatted update preview is stale; preview again".into(),
            ));
        }
        if !prepared.changeset_blob.is_empty() {
            transaction
                .apply_strm(
                    &mut Cursor::new(&prepared.changeset_blob),
                    Some(is_taxonomy_session_table),
                    |_, _| ConflictAction::SQLITE_CHANGESET_ABORT,
                )
                .map_err(|_| {
                    CoreError::InvalidArgument(
                        "formatted update preview is stale; preview again".into(),
                    )
                })?;
        }
        validate_taxonomy(&transaction)?;
        let result = store_applied_rows(
            &transaction,
            &prepared.rows,
            prepared.preview.rows,
            prepared.changeset_blob,
            delimiter,
        )?;
        cancellation.check()?;
        transaction.commit()?;
        Ok(result)
    })();
    cancellation.normalize(result)
}

fn store_applied_rows(
    transaction: &Transaction<'_>,
    rows: &[TaxonInputRow],
    outcomes: Vec<TaxonRowOutcome>,
    changeset_blob: Vec<u8>,
    delimiter: String,
) -> CoreResult<TaxonomyOperationResult> {
    let failed_rows = outcomes
        .iter()
        .filter(|row| row.operation_types.iter().any(|value| value.is_failure()))
        .count();
    let operation_id = operations::insert_operation(
        transaction,
        NewOperation {
            kind: "taxonomy_update",
            source: "formatted_update",
            total_items: outcomes.len(),
            succeeded_items: outcomes.len() - failed_rows,
            failed_items: failed_rows,
            rollbackable: true,
            has_formatted_input: true,
        },
    )?;
    let result = TaxonomyOperationResult {
        operation_id,
        total_rows: outcomes.len(),
        succeeded_rows: outcomes.len() - failed_rows,
        failed_rows,
        delimiter,
        encoding: "UTF-8".into(),
        rows: outcomes,
    };
    transaction.execute(
        r#"
        INSERT INTO operation_changesets (operation_id, changeset_blob)
        VALUES (?, ?)
        "#,
        params![operation_id, changeset_blob],
    )?;
    let mut insert_input = transaction.prepare_cached(
        r#"
        INSERT INTO operation_formatted_inputs (
            operation_id, sequence, input_json
        ) VALUES (?, ?, ?)
        "#,
    )?;
    for (index, input) in rows.iter().enumerate() {
        insert_input.execute(params![
            operation_id,
            (index + 1) as i64,
            serialize_json(input, "taxonomy operation input")?
        ])?;
    }
    drop(insert_input);
    insert_operation_audit(transaction, operation_id, &result.rows)?;
    let affected_taxon_ids = affected_taxon_ids_from_changeset(transaction, &changeset_blob)?;
    super::sync::record_event(transaction, Some(operation_id), affected_taxon_ids, false)?;
    Ok(result)
}

fn taxonomy_revision(connection: &Connection) -> CoreResult<TaxonomyRevision> {
    connection
        .query_row(
            r#"
            SELECT taxonomy_identity,
                   (SELECT COALESCE(MAX(operation_id), 0) FROM operations),
                   (SELECT COUNT(*) FROM operations)
            FROM taxonomy_identity
            WHERE identity_id = 1
            "#,
            [],
            |row| {
                Ok(TaxonomyRevision {
                    taxonomy_identity: row.get(0)?,
                    latest_operation_id: row.get(1)?,
                    operation_count: row.get(2)?,
                })
            },
        )
        .map_err(Into::into)
}

fn insert_operation_audit(
    transaction: &Transaction<'_>,
    operation_id: i64,
    outcomes: &[TaxonRowOutcome],
) -> CoreResult<()> {
    for outcome in outcomes {
        let succeeded = !outcome
            .operation_types
            .iter()
            .any(|status| status.is_failure());
        let before = outcome
            .changes
            .iter()
            .map(|change| {
                serde_json::json!({
                    "field": change.field,
                    "value": change.old_value,
                })
            })
            .collect::<Vec<_>>();
        let after = outcome
            .changes
            .iter()
            .map(|change| {
                serde_json::json!({
                    "field": change.field,
                    "value": change.new_value,
                })
            })
            .collect::<Vec<_>>();
        operations::insert_audit_row(
            transaction,
            operation_id,
            NewAuditRow {
                sequence: outcome.row_number,
                entity_type: "taxon",
                entity_id: outcome
                    .target
                    .as_ref()
                    .map(|target| target.taxon_id.to_string()),
                action: "formatted_update",
                before_json: Some(serde_json::json!({ "fields": before })),
                after_json: Some(serde_json::json!({
                    "operation_types": outcome.operation_types,
                    "fields": after,
                })),
                succeeded,
                message: &outcome.message,
            },
        )?;
    }
    Ok(())
}

fn process_rows(
    transaction: &Transaction<'_>,
    rows: &[TaxonInputRow],
    cancellation: &CancellationToken,
) -> CoreResult<Vec<TaxonRowOutcome>> {
    let synonym_parser = SynonymAuthorityParser::load(transaction)?;
    let mut outcomes = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        cancellation.check()?;
        outcomes.push(process_row(transaction, &synonym_parser, index + 1, row)?);
    }
    Ok(outcomes)
}

fn process_row(
    transaction: &Transaction<'_>,
    synonym_parser: &SynonymAuthorityParser,
    row_number: usize,
    row: &TaxonInputRow,
) -> CoreResult<TaxonRowOutcome> {
    let normalized = match NormalizedInput::from_row(row, synonym_parser) {
        Ok(value) => value,
        Err(message) => return Ok(failed_outcome(row_number, TaxonRowStatus::Invalid, message)),
    };
    let match_result = find_target(transaction, &normalized)?;

    match match_result {
        MatchResult::Many(candidates) => Ok(TaxonRowOutcome {
            row_number,
            operation_types: vec![TaxonRowStatus::MultipleCandidates],
            message: candidate_message(&candidates),
            target: None,
            parent: None,
            candidates,
            changes: Vec::new(),
        }),
        MatchResult::None => create_taxon(transaction, row_number, &normalized),
        MatchResult::One(summary, matched_name) => {
            update_existing(transaction, row_number, &normalized, summary, &matched_name)
        }
    }
}

fn create_taxon(
    transaction: &Transaction<'_>,
    row_number: usize,
    input: &NormalizedInput,
) -> CoreResult<TaxonRowOutcome> {
    let mut changes = Vec::new();
    let parent = if let Some(parent_rank) = input.target_rank.parent() {
        match resolve_or_create_lineage(
            transaction,
            input,
            parent_rank,
            input.target_rank,
            &mut changes,
        )? {
            Ok(parent) => Some(parent),
            Err(LineageFailure::MissingParent {
                child_rank,
                parent_rank,
            }) => {
                return Ok(failed_outcome(
                    row_number,
                    TaxonRowStatus::NotMatched,
                    format!(
                        "new {} taxon requires a {} scientific name",
                        child_rank.as_str(),
                        parent_rank.as_str()
                    ),
                ));
            }
            Err(LineageFailure::MultipleCandidates(candidates)) => {
                return Ok(TaxonRowOutcome {
                    row_number,
                    operation_types: vec![TaxonRowStatus::MultipleCandidates],
                    message: candidate_message(&candidates),
                    target: None,
                    parent: None,
                    candidates,
                    changes: Vec::new(),
                });
            }
        }
    } else {
        None
    };

    let target = insert_taxon_with_scientific_name(
        transaction,
        input.target_rank,
        &input.target_name,
        parent.as_ref(),
        input.geological_range.as_deref(),
        input.authority_year.as_deref(),
        input.source.as_deref(),
        &mut changes,
    )?;
    let taxon_id = target.taxon_id;
    apply_additional_names(transaction, taxon_id, input, &mut changes)?;
    let mut operation_types = classify_changes(&changes);
    operation_types.insert(0, TaxonRowStatus::NewTaxon);
    Ok(TaxonRowOutcome {
        row_number,
        operation_types,
        message: describe_changes(&changes),
        target: Some(target),
        parent,
        candidates: Vec::new(),
        changes,
    })
}

fn update_existing(
    transaction: &Transaction<'_>,
    row_number: usize,
    input: &NormalizedInput,
    summary: TaxonSummary,
    matched_name: &MatchedName,
) -> CoreResult<TaxonRowOutcome> {
    let taxon_id = summary.taxon_id;
    let mut changes = Vec::new();
    update_taxon_field(
        transaction,
        taxon_id,
        "geological_range",
        input.geological_range.as_deref(),
        &mut changes,
    )?;
    update_name_fields(
        transaction,
        taxon_id,
        matched_name.existing_type,
        &matched_name.name,
        matched_name.authority_year.as_deref(),
        input.source.as_deref(),
        &mut changes,
    )?;
    for (index, name) in input.scientific_names().into_iter().enumerate() {
        if index == matched_name.input_index {
            continue;
        }
        add_or_supplement_name(
            transaction,
            taxon_id,
            TaxonomyNameType::Synonym,
            &name.name,
            name.authority_year.as_deref(),
            input.source.as_deref(),
            &mut changes,
        )?;
    }
    apply_localized_input_names(transaction, taxon_id, input, &mut changes)?;
    let operation_types = classify_changes(&changes);
    let target = load_taxon_summary(transaction, taxon_id)?;
    Ok(TaxonRowOutcome {
        row_number,
        operation_types,
        message: if changes.is_empty() {
            "input produces no change".into()
        } else {
            describe_changes(&changes)
        },
        target,
        parent: None,
        candidates: Vec::new(),
        changes,
    })
}

fn apply_additional_names(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    input: &NormalizedInput,
    changes: &mut Vec<TaxonChange>,
) -> CoreResult<()> {
    for synonym in &input.synonyms {
        add_or_supplement_name(
            transaction,
            taxon_id,
            TaxonomyNameType::Synonym,
            &synonym.name,
            synonym.authority_year.as_deref(),
            input.source.as_deref(),
            changes,
        )?;
    }
    apply_localized_input_names(transaction, taxon_id, input, changes)
}

fn apply_localized_input_names(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    input: &NormalizedInput,
    changes: &mut Vec<TaxonChange>,
) -> CoreResult<()> {
    apply_localized_names(
        transaction,
        taxon_id,
        TaxonomyNameType::ZhName,
        TaxonomyNameType::ZhAlias,
        &input.zh_names,
        input.source.as_deref(),
        changes,
    )?;
    apply_localized_names(
        transaction,
        taxon_id,
        TaxonomyNameType::EnName,
        TaxonomyNameType::EnAlias,
        &input.en_names,
        input.source.as_deref(),
        changes,
    )
}

fn apply_localized_names(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    accepted_type: TaxonomyNameType,
    alias_type: TaxonomyNameType,
    names: &[String],
    source: Option<&str>,
    changes: &mut Vec<TaxonChange>,
) -> CoreResult<()> {
    let mut has_accepted: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM taxon_names WHERE taxon_id = ? AND name_type = ?)",
        params![taxon_id, accepted_type.code()],
        |row| row.get(0),
    )?;
    for name in names {
        let existing_type = existing_name_type(transaction, taxon_id, name, accepted_type)?;
        if let Some(existing_type) = existing_type {
            update_name_fields(
                transaction,
                taxon_id,
                existing_type,
                name,
                None,
                source,
                changes,
            )?;
            continue;
        }
        let name_type = if has_accepted {
            alias_type
        } else {
            has_accepted = true;
            accepted_type
        };
        insert_name(
            transaction,
            taxon_id,
            name_type,
            name,
            None,
            source,
            changes,
        )?;
    }
    Ok(())
}

fn add_or_supplement_name(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    requested_type: TaxonomyNameType,
    name: &str,
    authority_year: Option<&str>,
    source: Option<&str>,
    changes: &mut Vec<TaxonChange>,
) -> CoreResult<()> {
    if let Some(existing_type) = existing_name_type(transaction, taxon_id, name, requested_type)? {
        update_name_fields(
            transaction,
            taxon_id,
            existing_type,
            name,
            authority_year,
            source,
            changes,
        )
    } else {
        insert_name(
            transaction,
            taxon_id,
            requested_type,
            name,
            authority_year,
            source,
            changes,
        )
    }
}

fn insert_name(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    name_type: TaxonomyNameType,
    name: &str,
    authority_year: Option<&str>,
    source: Option<&str>,
    changes: &mut Vec<TaxonChange>,
) -> CoreResult<()> {
    transaction.execute(
        r#"
        INSERT INTO taxon_names (taxon_id, name_type, name, authority_year, source)
        VALUES (?, ?, ?, ?, ?)
        "#,
        params![taxon_id, name_type.code(), name, authority_year, source],
    )?;
    changes.push(TaxonChange {
        kind: TaxonChangeKind::AppendName,
        field: name_type.as_str().into(),
        old_value: None,
        new_value: Some(name.into()),
    });
    Ok(())
}

fn update_taxon_field(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    field: &str,
    new_value: Option<&str>,
    changes: &mut Vec<TaxonChange>,
) -> CoreResult<()> {
    let Some(new_value) = new_value else {
        return Ok(());
    };
    let old_value: Option<String> = transaction.query_row(
        "SELECT geological_range FROM taxa WHERE taxon_id = ?",
        [taxon_id],
        |row| row.get(0),
    )?;
    if old_value.as_deref() == Some(new_value) {
        return Ok(());
    }
    transaction.execute(
        "UPDATE taxa SET geological_range = ? WHERE taxon_id = ?",
        params![new_value, taxon_id],
    )?;
    changes.push(TaxonChange {
        kind: if old_value.is_some() {
            TaxonChangeKind::Overwrite
        } else {
            TaxonChangeKind::Supplement
        },
        field: format!("taxa.{field}"),
        old_value,
        new_value: Some(new_value.into()),
    });
    Ok(())
}

fn update_name_fields(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    name_type: TaxonomyNameType,
    name: &str,
    authority_year: Option<&str>,
    source: Option<&str>,
    changes: &mut Vec<TaxonChange>,
) -> CoreResult<()> {
    let current = transaction
        .query_row(
            r#"
            SELECT authority_year, source
            FROM taxon_names
            WHERE taxon_id = ? AND name_type = ? AND name = ?
            "#,
            params![taxon_id, name_type.code(), name],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()?;
    let Some((old_authority, old_source)) = current else {
        return Ok(());
    };
    if let Some(authority_year) = authority_year
        && old_authority.as_deref() != Some(authority_year)
    {
        transaction.execute(
            r#"
            UPDATE taxon_names SET authority_year = ?
            WHERE taxon_id = ? AND name_type = ? AND name = ?
            "#,
            params![authority_year, taxon_id, name_type.code(), name],
        )?;
        changes.push(TaxonChange {
            kind: if old_authority.is_some() {
                TaxonChangeKind::Overwrite
            } else {
                TaxonChangeKind::Supplement
            },
            field: format!("{}.authority_year", name_type.as_str()),
            old_value: old_authority,
            new_value: Some(authority_year.into()),
        });
    }
    if old_source.is_none()
        && let Some(source) = source
    {
        transaction.execute(
            r#"
            UPDATE taxon_names SET source = ?
            WHERE taxon_id = ? AND name_type = ? AND name = ?
            "#,
            params![source, taxon_id, name_type.code(), name],
        )?;
        changes.push(TaxonChange {
            kind: TaxonChangeKind::Supplement,
            field: format!("{}.source", name_type.as_str()),
            old_value: None,
            new_value: Some(source.into()),
        });
    }
    Ok(())
}

fn existing_name_type(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    name: &str,
    family: TaxonomyNameType,
) -> CoreResult<Option<TaxonomyNameType>> {
    let accepted_type = family.accepted_type();
    let alias_type = family.alias_type();
    transaction
        .query_row(
            r#"
            SELECT name_type
            FROM taxon_names
            WHERE taxon_id = ?
              AND name = ? COLLATE BINARY
              AND name_type IN (?, ?)
            "#,
            params![taxon_id, name, accepted_type.code(), alias_type.code()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .map(TaxonomyNameType::from_code)
        .transpose()
}

fn find_target(transaction: &Transaction<'_>, input: &NormalizedInput) -> CoreResult<MatchResult> {
    for (input_index, input_name) in input.scientific_names().into_iter().enumerate() {
        let matches =
            find_preferred_scientific_matches(transaction, input.target_rank, &input_name.name)?;
        if matches.is_empty() {
            continue;
        }
        let mut candidates = load_scientific_candidates(transaction, matches)?;
        if candidates.len() > 1 {
            candidates =
                disambiguate_candidates(transaction, candidates, input, input.target_rank)?;
        }
        if let [candidate] = candidates.as_slice() {
            return Ok(MatchResult::One(
                candidate.summary.clone(),
                MatchedName {
                    input_index,
                    name: input_name.name,
                    authority_year: input_name.authority_year,
                    existing_type: candidate.existing_type,
                },
            ));
        }
        if candidates.len() > 1 {
            return Ok(MatchResult::Many(
                candidates
                    .into_iter()
                    .map(|candidate| candidate.summary)
                    .collect(),
            ));
        }
    }
    Ok(MatchResult::None)
}

fn find_preferred_scientific_matches(
    transaction: &Transaction<'_>,
    rank: TaxonRank,
    name: &str,
) -> CoreResult<Vec<ScientificMatch>> {
    let mut matches =
        match_exact_taxonomy_name(transaction, name, rank, TaxonomyNameType::SciName)?;
    if matches.is_empty() {
        matches = match_exact_taxonomy_name(transaction, name, rank, TaxonomyNameType::Synonym)?;
    }
    Ok(matches
        .into_iter()
        .map(|matched| ScientificMatch {
            taxon_id: matched.taxon_id,
            existing_type: matched.name_type,
        })
        .collect())
}

fn load_scientific_candidates(
    transaction: &Transaction<'_>,
    matches: Vec<ScientificMatch>,
) -> CoreResult<Vec<ScientificCandidate>> {
    let taxon_ids = matches
        .iter()
        .map(|matched| matched.taxon_id)
        .collect::<Vec<_>>();
    let summaries = load_taxon_summaries(transaction, &taxon_ids)?;
    Ok(matches
        .into_iter()
        .zip(summaries)
        .map(|(matched, summary)| ScientificCandidate {
            summary,
            existing_type: matched.existing_type,
        })
        .collect())
}

fn disambiguate_candidates(
    transaction: &Transaction<'_>,
    mut candidates: Vec<ScientificCandidate>,
    input: &NormalizedInput,
    rank: TaxonRank,
) -> CoreResult<Vec<ScientificCandidate>> {
    for ancestor_rank in TaxonRank::ALL[..rank.index()].iter().rev().copied() {
        let Some(expected) = input.path[ancestor_rank.index()].as_deref() else {
            continue;
        };
        let allowed_ancestor_ids =
            find_preferred_scientific_matches(transaction, ancestor_rank, expected)?
                .into_iter()
                .map(|matched| matched.taxon_id)
                .collect::<HashSet<_>>();
        candidates.retain(|candidate| {
            candidate.summary.breadcrumb.iter().any(|ancestor| {
                ancestor.rank == ancestor_rank && allowed_ancestor_ids.contains(&ancestor.taxon_id)
            })
        });
        if candidates.len() <= 1 {
            break;
        }
    }
    Ok(candidates)
}

fn resolve_or_create_lineage(
    transaction: &Transaction<'_>,
    input: &NormalizedInput,
    rank: TaxonRank,
    child_rank: TaxonRank,
    changes: &mut Vec<TaxonChange>,
) -> CoreResult<Result<TaxonSummary, LineageFailure>> {
    let Some(name) = input.path[rank.index()].as_deref() else {
        return Ok(Err(LineageFailure::MissingParent {
            child_rank,
            parent_rank: rank,
        }));
    };
    let matches = find_preferred_scientific_matches(transaction, rank, name)?;
    let mut candidates = if matches.is_empty() {
        Vec::new()
    } else {
        load_scientific_candidates(transaction, matches)?
    };
    if candidates.len() > 1 {
        candidates = disambiguate_candidates(transaction, candidates, input, rank)?;
    }
    match candidates.as_slice() {
        [candidate] => return Ok(Ok(candidate.summary.clone())),
        [_, _, ..] => {
            return Ok(Err(LineageFailure::MultipleCandidates(
                candidates
                    .into_iter()
                    .map(|candidate| candidate.summary)
                    .collect(),
            )));
        }
        [] => {}
    }

    let parent = if let Some(parent_rank) = rank.parent() {
        if input.path[parent_rank.index()].is_none() {
            return Ok(Err(LineageFailure::MissingParent {
                child_rank: rank,
                parent_rank,
            }));
        }
        match resolve_or_create_lineage(transaction, input, parent_rank, rank, changes)? {
            Ok(parent) => Some(parent),
            Err(failure) => return Ok(Err(failure)),
        }
    } else {
        None
    };
    insert_taxon_with_scientific_name(
        transaction,
        rank,
        name,
        parent.as_ref(),
        None,
        None,
        None,
        changes,
    )
    .map(Ok)
}

#[allow(clippy::too_many_arguments)]
fn insert_taxon_with_scientific_name(
    transaction: &Transaction<'_>,
    rank: TaxonRank,
    name: &str,
    parent: Option<&TaxonSummary>,
    geological_range: Option<&str>,
    authority_year: Option<&str>,
    source: Option<&str>,
    changes: &mut Vec<TaxonChange>,
) -> CoreResult<TaxonSummary> {
    transaction.execute(
        "INSERT INTO taxa (parent_taxon_id, rank, geological_range) VALUES (?, ?, ?)",
        params![
            parent.map(|value| value.taxon_id),
            rank.code(),
            geological_range
        ],
    )?;
    let taxon_id = transaction.last_insert_rowid();
    transaction.execute(
        r#"
        INSERT INTO taxon_names (taxon_id, name_type, name, authority_year, source)
        VALUES (?, ?, ?, ?, ?)
        "#,
        params![
            taxon_id,
            TaxonomyNameType::SciName.code(),
            name,
            authority_year,
            source
        ],
    )?;
    changes.extend([
        TaxonChange {
            kind: TaxonChangeKind::CreateTaxon,
            field: "taxon".into(),
            old_value: None,
            new_value: Some(name.into()),
        },
        TaxonChange {
            kind: TaxonChangeKind::AppendName,
            field: "sci_name".into(),
            old_value: None,
            new_value: Some(name.into()),
        },
    ]);
    load_taxon_summary(transaction, taxon_id)?
        .ok_or_else(|| CoreError::NotFound(format!("new {} taxon {taxon_id}", rank.as_str())))
}

enum MatchResult {
    None,
    One(TaxonSummary, MatchedName),
    Many(Vec<TaxonSummary>),
}

#[derive(Debug)]
struct ScientificMatch {
    taxon_id: i64,
    existing_type: TaxonomyNameType,
}

#[derive(Debug)]
struct ScientificCandidate {
    summary: TaxonSummary,
    existing_type: TaxonomyNameType,
}

enum LineageFailure {
    MissingParent {
        child_rank: TaxonRank,
        parent_rank: TaxonRank,
    },
    MultipleCandidates(Vec<TaxonSummary>),
}

#[derive(Debug, Clone)]
struct ParsedSynonym {
    name: String,
    authority_year: Option<String>,
}

#[derive(Debug)]
struct MatchedName {
    input_index: usize,
    name: String,
    authority_year: Option<String>,
    existing_type: TaxonomyNameType,
}

#[derive(Debug)]
struct NormalizedInput {
    path: [Option<String>; 5],
    target_rank: TaxonRank,
    target_name: String,
    authority_year: Option<String>,
    synonyms: Vec<ParsedSynonym>,
    zh_names: Vec<String>,
    en_names: Vec<String>,
    geological_range: Option<String>,
    source: Option<String>,
}

impl NormalizedInput {
    fn from_row(
        row: &TaxonInputRow,
        synonym_parser: &SynonymAuthorityParser,
    ) -> Result<Self, String> {
        let mut path = [
            normalize_name(row.kingdom.as_deref()),
            normalize_name(row.order.as_deref()),
            normalize_name(row.family.as_deref()),
            normalize_name(row.genus.as_deref()),
            normalize_name(row.species.as_deref()),
        ];
        let target_index = path
            .iter()
            .rposition(Option::is_some)
            .ok_or_else(|| "at least one scientific rank field is required".to_string())?;
        let target_rank = TaxonRank::ALL[target_index];
        if target_rank == TaxonRank::Species && path[3].is_none() {
            path[3] = path[4]
                .as_deref()
                .and_then(|value| value.split_whitespace().next())
                .map(str::to_string);
        }
        let target_name = path[target_index].clone().unwrap_or_default();
        let mut seen_scientific_names = HashSet::from([target_name.clone()]);
        let mut synonyms = Vec::new();
        for raw in &row.synonyms {
            let parts = synonym_parser
                .split(raw)
                .map_err(|error| error.to_string())?;
            if seen_scientific_names.insert(parts.name.clone()) {
                synonyms.push(ParsedSynonym {
                    name: parts.name,
                    authority_year: normalize_text(parts.authority_year.as_deref()),
                });
            }
        }
        let zh_names = combined_names(row.zh_name.as_deref(), &row.zh_alias);
        let en_names = combined_names(row.en_name.as_deref(), &row.en_alias);
        Ok(Self {
            path,
            target_rank,
            target_name,
            authority_year: normalize_text(row.authority_year.as_deref()),
            synonyms,
            zh_names,
            en_names,
            geological_range: normalize_text(row.geological_range.as_deref()),
            source: normalize_text(row.source.as_deref()),
        })
    }

    fn scientific_names(&self) -> Vec<ParsedSynonym> {
        std::iter::once(ParsedSynonym {
            name: self.target_name.clone(),
            authority_year: self.authority_year.clone(),
        })
        .chain(self.synonyms.iter().cloned())
        .collect()
    }
}

fn combined_names(primary: Option<&str>, aliases: &[String]) -> Vec<String> {
    let mut values = Vec::with_capacity(aliases.len() + 1);
    if let Some(primary) = normalize_name(primary) {
        values.push(primary);
    }
    values.extend(unique_names(aliases));
    deduplicate(values)
}

fn unique_names(values: &[String]) -> Vec<String> {
    deduplicate(
        values
            .iter()
            .filter_map(|value| normalize_name(Some(value)))
            .collect(),
    )
}

fn deduplicate(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn normalize_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn classify_changes(changes: &[TaxonChange]) -> Vec<TaxonRowStatus> {
    if changes.is_empty() {
        return vec![TaxonRowStatus::NoChange];
    }
    let mut types = Vec::new();
    if changes
        .iter()
        .any(|change| change.kind == TaxonChangeKind::Supplement)
    {
        types.push(TaxonRowStatus::Supplement);
    }
    if changes
        .iter()
        .any(|change| change.kind == TaxonChangeKind::AppendName)
    {
        types.push(TaxonRowStatus::NewName);
    }
    if changes
        .iter()
        .any(|change| change.kind == TaxonChangeKind::Overwrite)
    {
        types.push(TaxonRowStatus::Overwrite);
    }
    types
}

fn failed_outcome(
    row_number: usize,
    operation_type: TaxonRowStatus,
    message: impl Into<String>,
) -> TaxonRowOutcome {
    TaxonRowOutcome {
        row_number,
        operation_types: vec![operation_type],
        message: message.into(),
        target: None,
        parent: None,
        candidates: Vec::new(),
        changes: Vec::new(),
    }
}

fn candidate_message(candidates: &[TaxonSummary]) -> String {
    let names = candidates
        .iter()
        .map(|candidate| {
            candidate
                .names
                .sci_name
                .clone()
                .unwrap_or_else(|| candidate.taxon_id.to_string())
        })
        .collect::<Vec<_>>()
        .join("/");
    format!("multiple candidates: {names}")
}

fn describe_changes(changes: &[TaxonChange]) -> String {
    changes
        .iter()
        .map(|change| match (&change.old_value, &change.new_value) {
            (None, Some(new)) => format!("{} added: {}", change.field, new),
            (Some(old), Some(new)) => format!("{}: {} -> {}", change.field, old, new),
            (Some(old), None) => format!("{} removed: {}", change.field, old),
            (None, None) => format!("{} changed", change.field),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn get_taxonomy_name_separator(database: &Database) -> CoreResult<String> {
    Ok(crate::metadata::get_raw(
        &database.connect_taxonomy_metadata_context()?,
        crate::metadata::MetadataKey::TaxonomyNameSeparator,
    )?
    .unwrap_or_else(|| ";".to_string()))
}

pub fn set_taxonomy_name_separator(database: &Database, separator: &str) -> CoreResult<()> {
    let mut characters = separator.chars();
    let Some(character) = characters.next() else {
        return Err(CoreError::InvalidArgument(
            "name separator is required".into(),
        ));
    };
    if characters.next().is_some() || character == '|' || character.is_whitespace() {
        return Err(CoreError::InvalidArgument(
            "name separator must be one non-whitespace character other than '|'".into(),
        ));
    }
    crate::metadata::set_raw(
        &database.connect_taxonomy_metadata_context()?,
        crate::metadata::MetadataKey::TaxonomyNameSeparator,
        separator,
    )
}

pub fn taxonomy_formatted_update_template(database: &Database) -> CoreResult<String> {
    let mut writer = WriterBuilder::new()
        .delimiter(crate::general::get_csv_delimiter_byte(database)?)
        .from_writer(Vec::new());
    writer.write_record(TAXONOMY_INPUT_COLUMNS)?;
    writer.flush()?;
    String::from_utf8(writer.into_inner().map_err(|error| error.into_error())?)
        .map_err(|error| CoreError::InvalidArgument(format!("invalid UTF-8 template: {error}")))
}

pub fn parse_taxonomy_input_csv(
    database: &Database,
    input: &str,
) -> CoreResult<Vec<TaxonInputRow>> {
    let separator = get_taxonomy_name_separator(database)?;
    let separator = separator.chars().next().unwrap_or(';');
    let mut reader = ReaderBuilder::new()
        .delimiter(crate::general::get_csv_delimiter_byte(database)?)
        .from_reader(input.as_bytes());
    let headers = reader.headers()?.clone();
    if headers.is_empty() {
        return Err(CoreError::InvalidArgument("CSV header is required".into()));
    }
    let allowed = TAXONOMY_INPUT_COLUMNS.into_iter().collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    for header in &headers {
        if !allowed.contains(header) {
            return Err(CoreError::InvalidArgument(format!(
                "unknown taxonomy input column: {header}"
            )));
        }
        if !seen.insert(header.to_string()) {
            return Err(CoreError::InvalidArgument(format!(
                "duplicate taxonomy input column: {header}"
            )));
        }
    }
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record?;
        let mut row = TaxonInputRow::default();
        for (header, value) in headers.iter().zip(record.iter()) {
            let scalar = || normalize_text(Some(value));
            let normalized_multiple = || {
                value
                    .split(separator)
                    .filter_map(|value| normalize_name(Some(value)))
                    .collect::<Vec<_>>()
            };
            let raw_multiple = || {
                if value.is_empty() {
                    Vec::new()
                } else {
                    value
                        .split(separator)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                }
            };
            match header {
                "kingdom" => row.kingdom = scalar(),
                "order" => row.order = scalar(),
                "family" => row.family = scalar(),
                "genus" => row.genus = scalar(),
                "species" => row.species = scalar(),
                "authority_year" => row.authority_year = scalar(),
                "synonyms" => row.synonyms = raw_multiple(),
                "zh_name" => row.zh_name = scalar(),
                "zh_alias" => row.zh_alias = normalized_multiple(),
                "en_name" => row.en_name = scalar(),
                "en_alias" => row.en_alias = normalized_multiple(),
                "geological_range" => row.geological_range = scalar(),
                "source" => row.source = scalar(),
                _ => {}
            }
        }
        rows.push(row);
    }
    Ok(rows)
}

pub fn taxonomy_log_csv(database: &Database, rows: &[TaxonRowOutcome]) -> CoreResult<String> {
    let mut writer = WriterBuilder::new()
        .delimiter(crate::general::get_csv_delimiter_byte(database)?)
        .from_writer(Vec::new());
    writer.write_record([
        "row_number",
        "operation_types",
        "summary",
        "parent",
        "changes",
        "message",
    ])?;
    for row in rows {
        writer.write_record([
            row.row_number.to_string(),
            serialize_json(&row.operation_types, "taxonomy row operation types")?,
            serialize_json(&row.target, "taxonomy row target")?,
            serialize_json(&row.parent, "taxonomy row parent")?,
            serialize_json(&row.changes, "taxonomy row changes")?,
            row.message.clone(),
        ])?;
    }
    writer.flush()?;
    String::from_utf8(writer.into_inner().map_err(|error| error.into_error())?)
        .map_err(|error| CoreError::InvalidArgument(format!("invalid UTF-8 log: {error}")))
}

pub fn list_operations(
    database: &Database,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<OperationPage<OperationSummary>> {
    operations::list_operations(
        &database.connect_taxonomy_metadata_context()?,
        cursor,
        limit,
    )
}

pub fn list_operation_audit(
    database: &Database,
    operation_id: i64,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<OperationPage<OperationAuditRow>> {
    operations::list_operation_audit(
        &database.connect_taxonomy_metadata_context()?,
        operation_id,
        cursor,
        limit,
    )
}

pub fn get_operation_input(
    database: &Database,
    operation_id: i64,
) -> CoreResult<Option<OperationInput>> {
    operations::get_operation_input(&database.connect_taxonomy_metadata_context()?, operation_id)
}

pub fn rollback_operation(database: &Database, operation_id: i64) -> CoreResult<()> {
    let _guard = database.try_taxonomy_mutation()?;
    let mut connection = database.connect_taxonomy_metadata_context()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let summary = operations::get_operation(&transaction, operation_id)?
        .ok_or_else(|| CoreError::NotFound(format!("operation {operation_id}")))?;
    if !summary.rollbackable {
        return Err(CoreError::InvalidArgument(format!(
            "operation {operation_id} cannot be rolled back"
        )));
    }
    let changeset_blob = transaction
        .query_row(
            "SELECT changeset_blob FROM operation_changesets WHERE operation_id = ?",
            [operation_id],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?
        .ok_or_else(|| {
            CoreError::Consistency(format!(
                "operation {operation_id} has no rollback changeset"
            ))
        })?;
    let affected_taxon_ids = affected_taxon_ids_from_changeset(&transaction, &changeset_blob)?;
    apply_inverse_taxonomy_changeset(&transaction, operation_id, &changeset_blob)?;
    validate_foreign_key_integrity(&transaction)?;
    validate_taxonomy(&transaction)?;
    super::sync::record_event(&transaction, None, affected_taxon_ids, false)?;
    operations::delete_operation(&transaction, operation_id)?;
    transaction.commit()?;
    Ok(())
}

pub(super) fn serialize_json<T: Serialize + ?Sized>(value: &T, label: &str) -> CoreResult<String> {
    serde_json::to_string(value)
        .map_err(|error| CoreError::InvalidArgument(format!("invalid {label}: {error}")))
}

#[cfg(test)]
#[path = "formatted/tests.rs"]
mod tests;
