use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::{
    ExistingTaxonUpdate, TaxonIdentifierLogRecord, TaxonLogRecord, TaxonNameInput,
    TaxonNameLogRecord, TaxonRowOutcome, TaxonUpdateOptions, TaxonomyBatchContext,
    TaxonomyLogChange, TaxonomyNameKind, apply_existing_taxon_update_with_log, hash_affected_taxa,
    insert_operation_batch, insert_operation_log,
};
use crate::{CoreError, CoreResult, Database};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonUpdateInput {
    pub taxon_id: i64,
    pub geological_range: Option<String>,
    pub scientific: Option<TaxonNameInput>,
    pub english: Option<TaxonNameInput>,
    pub chinese: Option<TaxonNameInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyUpdateActionResult {
    pub batch_id: Option<i64>,
    pub outcome: TaxonRowOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeleteTaxonNameInput {
    pub taxon_id: i64,
    pub name_kind: TaxonomyNameKind,
    pub name: String,
    pub replacement_accepted_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyActionResult {
    pub batch_id: i64,
    pub operation_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyCustomSqlResult {
    pub batch_id: i64,
}

#[derive(Debug, Serialize)]
struct DeleteTaxonInput {
    taxon_id: i64,
}

#[derive(Debug, Serialize)]
struct CustomSqlInput<'a> {
    sql: &'a str,
}

pub fn delete_taxon_name(
    database: &Database,
    input: DeleteTaxonNameInput,
) -> CoreResult<TaxonomyActionResult> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_taxon_exists(&transaction, input.taxon_id)?;
    let mut changes = Vec::new();
    let before = load_name_record(&transaction, input.taxon_id, input.name_kind, &input.name)?
        .ok_or_else(|| {
            CoreError::NotFound(format!(
                "{} name '{}' for taxon {}",
                input.name_kind.table(),
                input.name,
                input.taxon_id
            ))
        })?;
    let remaining_names =
        count_other_names(&transaction, input.taxon_id, input.name_kind, &input.name)?;

    if before.is_accepted && remaining_names > 0 {
        let replacement = input
            .replacement_accepted_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                CoreError::InvalidArgument(
                    "deleting an accepted name requires replacement_accepted_name".into(),
                )
            })?;
        if replacement == input.name {
            return Err(CoreError::InvalidArgument(
                "replacement_accepted_name must differ from the deleted name".into(),
            ));
        }
        let replacement_before =
            load_name_record(&transaction, input.taxon_id, input.name_kind, replacement)?
                .ok_or_else(|| {
                    CoreError::NotFound(format!(
                        "replacement {} name '{}' for taxon {}",
                        input.name_kind.table(),
                        replacement,
                        input.taxon_id
                    ))
                })?;
        let mut replacement_after = replacement_before.clone();
        replacement_after.is_accepted = true;
        promote_name(&transaction, input.taxon_id, input.name_kind, replacement)?;
        changes.push(TaxonomyLogChange::NameUpdated {
            name_kind: input.name_kind,
            before: replacement_before,
            after: replacement_after,
        });
    } else if input
        .replacement_accepted_name
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Err(CoreError::InvalidArgument(
            "replacement_accepted_name is only valid when deleting an accepted name".into(),
        ));
    }

    delete_name_record(&transaction, input.taxon_id, input.name_kind, &input.name)?;
    changes.push(TaxonomyLogChange::NameDeleted {
        name_kind: input.name_kind,
        before,
    });
    let after_hash = hash_affected_taxa(&transaction, &changes)?;
    let batch_id =
        insert_operation_batch(&transaction, &input, &TaxonomyBatchContext::QueryDeleteName)?;
    let operation_id = insert_operation_log(&transaction, batch_id, 1, &changes, &after_hash)?;
    transaction.commit()?;
    Ok(TaxonomyActionResult {
        batch_id,
        operation_id,
    })
}

pub fn update_taxon(
    database: &Database,
    input: TaxonUpdateInput,
    options: TaxonUpdateOptions,
) -> CoreResult<TaxonomyUpdateActionResult> {
    let mut connection = database.connect()?;
    let update = ExistingTaxonUpdate::new(
        input.geological_range.as_deref(),
        input.scientific.as_ref(),
        input.english.as_ref(),
        input.chinese.as_ref(),
    );
    let mut batch_id = None;
    let outcome = apply_existing_taxon_update_with_log(
        &mut connection,
        1,
        input.taxon_id,
        update,
        options,
        &mut batch_id,
        &input,
        &TaxonomyBatchContext::QueryUpdate { options },
    )?;
    Ok(TaxonomyUpdateActionResult { batch_id, outcome })
}

pub fn delete_taxon(database: &Database, taxon_id: i64) -> CoreResult<TaxonomyActionResult> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let before = load_deleted_taxon(&transaction, taxon_id)?
        .ok_or_else(|| CoreError::NotFound(format!("taxon {taxon_id}")))?;
    let child_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM taxa WHERE parent_taxon_id = ?",
        [taxon_id],
        |row| row.get(0),
    )?;
    if child_count > 0 {
        return Err(CoreError::InvalidArgument(format!(
            "taxon {taxon_id} cannot be deleted because it has child taxa"
        )));
    }
    let mapped_photo_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM photos_taxa_mapping WHERE taxon_id = ?",
        [taxon_id],
        |row| row.get(0),
    )?;
    if mapped_photo_count > 0 {
        return Err(CoreError::InvalidArgument(format!(
            "taxon {taxon_id} cannot be deleted because it is used by photo mappings"
        )));
    }
    let changes = vec![TaxonomyLogChange::TaxonDeleted {
        before: before.taxon,
        scientific: before.scientific,
        english: before.english,
        chinese: before.chinese,
        identifiers: before.identifiers,
    }];
    transaction.execute("DELETE FROM taxa WHERE taxon_id = ?", [taxon_id])?;
    let after_hash = hash_affected_taxa(&transaction, &changes)?;
    let batch_id = insert_operation_batch(
        &transaction,
        &DeleteTaxonInput { taxon_id },
        &TaxonomyBatchContext::QueryDeleteTaxon,
    )?;
    let operation_id = insert_operation_log(&transaction, batch_id, 1, &changes, &after_hash)?;
    transaction.commit()?;
    Ok(TaxonomyActionResult {
        batch_id,
        operation_id,
    })
}

pub fn execute_custom_taxonomy_sql(
    database: &Database,
    sql: &str,
) -> CoreResult<TaxonomyCustomSqlResult> {
    let sql = sql.trim();
    if sql.is_empty() {
        return Err(CoreError::InvalidArgument("sql is required".into()));
    }
    let mut connection = database.connect()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(sql)?;
    let batch_id = insert_operation_batch(
        &transaction,
        &CustomSqlInput { sql },
        &TaxonomyBatchContext::CustomSql,
    )?;
    transaction.commit()?;
    Ok(TaxonomyCustomSqlResult { batch_id })
}

#[derive(Debug)]
struct DeletedTaxon {
    taxon: TaxonLogRecord,
    scientific: Vec<TaxonNameLogRecord>,
    english: Vec<TaxonNameLogRecord>,
    chinese: Vec<TaxonNameLogRecord>,
    identifiers: Vec<TaxonIdentifierLogRecord>,
}

fn load_deleted_taxon(
    transaction: &Transaction<'_>,
    taxon_id: i64,
) -> CoreResult<Option<DeletedTaxon>> {
    let taxon = transaction
        .query_row(
            r#"
            SELECT parent_taxon_id, rank, geological_range
            FROM taxa
            WHERE taxon_id = ?
            "#,
            [taxon_id],
            |row| {
                Ok(TaxonLogRecord {
                    taxon_id,
                    parent_taxon_id: row.get(0)?,
                    rank: row.get(1)?,
                    geological_range: row.get(2)?,
                })
            },
        )
        .optional()?;
    let Some(taxon) = taxon else {
        return Ok(None);
    };
    Ok(Some(DeletedTaxon {
        taxon,
        scientific: load_name_records(transaction, taxon_id, TaxonomyNameKind::Scientific)?,
        english: load_name_records(transaction, taxon_id, TaxonomyNameKind::English)?,
        chinese: load_name_records(transaction, taxon_id, TaxonomyNameKind::Chinese)?,
        identifiers: load_identifier_records(transaction, taxon_id)?,
    }))
}

fn ensure_taxon_exists(transaction: &Transaction<'_>, taxon_id: i64) -> CoreResult<()> {
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM taxa WHERE taxon_id = ?)",
        [taxon_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(CoreError::NotFound(format!("taxon {taxon_id}")));
    }
    Ok(())
}

fn load_name_record(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: TaxonomyNameKind,
    name: &str,
) -> CoreResult<Option<TaxonNameLogRecord>> {
    let sql = format!(
        "SELECT is_accepted, authority_year, category, source FROM {} WHERE taxon_id = ? AND {} = ?",
        kind.table(),
        kind.column()
    );
    Ok(transaction
        .query_row(&sql, params![taxon_id, name], |row| {
            Ok(TaxonNameLogRecord {
                taxon_id,
                name: name.to_string(),
                is_accepted: row.get::<_, i64>(0)? != 0,
                authority_year: row.get(1)?,
                category: row.get(2)?,
                source: row.get(3)?,
            })
        })
        .optional()?)
}

fn load_name_records(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: TaxonomyNameKind,
) -> CoreResult<Vec<TaxonNameLogRecord>> {
    let sql = format!(
        "SELECT {}, is_accepted, authority_year, category, source FROM {} WHERE taxon_id = ? ORDER BY {}",
        kind.column(),
        kind.table(),
        kind.column()
    );
    let mut statement = transaction.prepare(&sql)?;
    let rows = statement.query_map([taxon_id], |row| {
        Ok(TaxonNameLogRecord {
            taxon_id,
            name: row.get(0)?,
            is_accepted: row.get::<_, i64>(1)? != 0,
            authority_year: row.get(2)?,
            category: row.get(3)?,
            source: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn load_identifier_records(
    transaction: &Transaction<'_>,
    taxon_id: i64,
) -> CoreResult<Vec<TaxonIdentifierLogRecord>> {
    let mut statement = transaction.prepare(
        r#"
        SELECT source, external_id
        FROM taxon_identifiers
        WHERE taxon_id = ?
        ORDER BY source, external_id
        "#,
    )?;
    let rows = statement.query_map([taxon_id], |row| {
        Ok(TaxonIdentifierLogRecord {
            taxon_id,
            source: row.get(0)?,
            external_id: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn count_other_names(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: TaxonomyNameKind,
    name: &str,
) -> CoreResult<i64> {
    let sql = format!(
        "SELECT COUNT(*) FROM {} WHERE taxon_id = ? AND {} != ?",
        kind.table(),
        kind.column()
    );
    Ok(transaction.query_row(&sql, params![taxon_id, name], |row| row.get(0))?)
}

fn promote_name(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: TaxonomyNameKind,
    name: &str,
) -> CoreResult<()> {
    let demote_sql = format!(
        "UPDATE {} SET is_accepted = 0 WHERE taxon_id = ?",
        kind.table()
    );
    transaction.execute(&demote_sql, [taxon_id])?;
    let promote_sql = format!(
        "UPDATE {} SET is_accepted = 1 WHERE taxon_id = ? AND {} = ?",
        kind.table(),
        kind.column()
    );
    transaction.execute(&promote_sql, params![taxon_id, name])?;
    Ok(())
}

fn delete_name_record(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: TaxonomyNameKind,
    name: &str,
) -> CoreResult<()> {
    let sql = format!(
        "DELETE FROM {} WHERE taxon_id = ? AND {} = ?",
        kind.table(),
        kind.column()
    );
    transaction.execute(&sql, params![taxon_id, name])?;
    Ok(())
}
