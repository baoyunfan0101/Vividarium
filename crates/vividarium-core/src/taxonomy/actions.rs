use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::changeset::start_taxonomy_session;
use super::validation::validate_taxonomy;
use super::{TaxonRank, TaxonomyNameType};
use crate::naming::normalize_taxonomy_name;
use crate::operations::{self, NewAuditRow, NewOperation, OperationInput};
use crate::{CoreError, CoreResult, Database};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeleteTaxonNameInput {
    pub taxon_id: i64,
    pub name_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromoteTaxonNameInput {
    pub taxon_id: i64,
    pub name_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonNameMetadataInput {
    pub name_id: i64,
    pub authority_year: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewTaxonNameInput {
    pub name: String,
    pub authority_year: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SaveTaxonNameGroupInput {
    pub taxon_id: i64,
    pub name_type: TaxonomyNameType,
    pub updates: Vec<TaxonNameMetadataInput>,
    pub additions: Vec<NewTaxonNameInput>,
}

pub fn promote_taxon_name(database: &Database, input: PromoteTaxonNameInput) -> CoreResult<()> {
    let action_input = taxonomy_action_input("promote_name", &input)?;
    let _guard = database.try_taxonomy_mutation()?;
    let mut connection = database.connect_taxonomy_metadata_context()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (current_type, name) = transaction
        .query_row(
            r#"
            SELECT taxon_names.name_type, taxon_names.name
            FROM taxa JOIN taxon_names USING (taxon_id)
            WHERE taxa.taxon_id = ? AND taxon_names.name_id = ?
            "#,
            params![input.taxon_id, input.name_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            CoreError::NotFound(format!(
                "name {} for taxon {}",
                input.name_id, input.taxon_id
            ))
        })?;
    let current_type = TaxonomyNameType::from_code(current_type)?;
    if current_type.is_primary() {
        return Err(CoreError::InvalidArgument(
            "the selected name is already accepted".into(),
        ));
    }
    let accepted_type = current_type.accepted_type();
    let (accepted_name_id, accepted_name) = transaction
        .query_row(
            "SELECT name_id, name FROM taxon_names WHERE taxon_id = ? AND name_type = ?",
            params![input.taxon_id, accepted_type.code()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            CoreError::InvalidArgument(format!(
                "taxon {} has no {} to exchange",
                input.taxon_id,
                accepted_type.as_str()
            ))
        })?;
    let mut session = start_taxonomy_session(&transaction)?;
    transaction.execute(
        "UPDATE taxon_names SET name_type = ? WHERE taxon_id = ? AND name_id = ?",
        params![current_type.code(), input.taxon_id, accepted_name_id],
    )?;
    transaction.execute(
        "UPDATE taxon_names SET name_type = ? WHERE taxon_id = ? AND name_id = ?",
        params![accepted_type.code(), input.taxon_id, input.name_id],
    )?;
    validate_taxonomy(&transaction)?;
    let mut changeset_blob = Vec::new();
    session.changeset_strm(&mut changeset_blob)?;
    drop(session);
    let operation_id = operations::insert_operation(
        &transaction,
        NewOperation {
            kind: "taxonomy_name_promote",
            source: "ui_promote",
            total_items: 1,
            succeeded_items: 1,
            failed_items: 0,
            rollbackable: true,
            has_formatted_input: false,
        },
    )?;
    operations::insert_operation_input(&transaction, operation_id, &action_input)?;
    transaction.execute(
        r#"
        INSERT INTO operation_changesets (operation_id, changeset_blob)
        VALUES (?, ?)
        "#,
        params![operation_id, changeset_blob],
    )?;
    operations::insert_audit_row(
        &transaction,
        operation_id,
        NewAuditRow {
            sequence: 1,
            entity_type: "taxon_name",
            entity_id: Some(input.name_id.to_string()),
            action: "promote",
            before_json: Some(serde_json::json!({
                "taxon_id": input.taxon_id,
                "accepted_name_id": accepted_name_id,
                "accepted_name": accepted_name.clone(),
                "promoted_name_id": input.name_id,
                "promoted_name": name.clone(),
                "accepted_name_type": accepted_type.code(),
                "alias_name_type": current_type.code(),
            })),
            after_json: Some(serde_json::json!({
                "taxon_id": input.taxon_id,
                "accepted_name_id": input.name_id,
                "accepted_name": name,
                "alias_name_id": accepted_name_id,
                "alias_name": accepted_name,
                "accepted_name_type": accepted_type.code(),
                "alias_name_type": current_type.code(),
            })),
            succeeded: true,
            message: "exchanged accepted and alias taxonomy names",
        },
    )?;
    super::sync::record_event(&transaction, Some(operation_id), [input.taxon_id], false)?;
    transaction.commit()?;
    Ok(())
}

pub fn save_taxon_name_group(
    database: &Database,
    input: SaveTaxonNameGroupInput,
) -> CoreResult<()> {
    let action_input = taxonomy_action_input("edit_name_group", &input)?;
    let _guard = database.try_taxonomy_mutation()?;
    let mut connection = database.connect_taxonomy_metadata_context()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (rank_code, parent_taxon_id) = transaction
        .query_row(
            "SELECT rank, parent_taxon_id FROM taxa WHERE taxon_id = ?",
            [input.taxon_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound(format!("taxon {}", input.taxon_id)))?;
    let rank = TaxonRank::from_code(rank_code)?;

    validate_name_group_additions(
        &transaction,
        input.taxon_id,
        rank,
        parent_taxon_id,
        input.name_type,
        &input.additions,
    )?;

    let mut session = start_taxonomy_session(&transaction)?;
    let mut seen_name_ids = HashSet::new();
    let mut before_records = Vec::new();
    let mut after_records = Vec::new();
    let mut changed_count = 0usize;

    for update in input.updates {
        if !seen_name_ids.insert(update.name_id) {
            return Err(CoreError::InvalidArgument(format!(
                "name {} is included more than once",
                update.name_id
            )));
        }
        let current = transaction
            .query_row(
                r#"
                SELECT name_type, name, authority_year, source
                FROM taxon_names
                WHERE taxon_id = ? AND name_id = ?
                "#,
                params![input.taxon_id, update.name_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| {
                CoreError::NotFound(format!(
                    "name {} for taxon {}",
                    update.name_id, input.taxon_id
                ))
            })?;
        let current_name_type = TaxonomyNameType::from_code(current.0)?;
        if current_name_type != input.name_type {
            return Err(CoreError::InvalidArgument(format!(
                "name {} is {}, not {}",
                update.name_id,
                current_name_type.as_str(),
                input.name_type.as_str()
            )));
        }
        let authority_year = normalized_optional(update.authority_year.as_deref());
        let source = normalized_optional(update.source.as_deref());
        if current.2 == authority_year && current.3 == source {
            continue;
        }
        transaction.execute(
            r#"
            UPDATE taxon_names
            SET authority_year = ?, source = ?
            WHERE taxon_id = ? AND name_id = ?
            "#,
            params![authority_year, source, input.taxon_id, update.name_id],
        )?;
        before_records.push(serde_json::json!({
            "name_id": update.name_id,
            "name": current.1.clone(),
            "name_type": input.name_type.as_str(),
            "authority_year": current.2,
            "source": current.3,
        }));
        after_records.push(serde_json::json!({
            "name_id": update.name_id,
            "name": current.1,
            "name_type": input.name_type.as_str(),
            "authority_year": authority_year,
            "source": source,
        }));
        changed_count += 1;
    }

    for addition in input.additions {
        let name = normalize_taxonomy_name(&addition.name)
            .ok_or_else(|| CoreError::InvalidArgument("taxonomy name must not be blank".into()))?;
        let authority_year = normalized_optional(addition.authority_year.as_deref());
        let source = normalized_optional(addition.source.as_deref());
        transaction.execute(
            r#"
            INSERT INTO taxon_names (taxon_id, name_type, name, authority_year, source)
            VALUES (?, ?, ?, ?, ?)
            "#,
            params![
                input.taxon_id,
                input.name_type.code(),
                name,
                authority_year,
                source
            ],
        )?;
        let name_id = transaction.last_insert_rowid();
        after_records.push(serde_json::json!({
            "name_id": name_id,
            "name": name,
            "name_type": input.name_type.as_str(),
            "authority_year": authority_year,
            "source": source,
        }));
        changed_count += 1;
    }

    if changed_count == 0 {
        drop(session);
        transaction.commit()?;
        return Ok(());
    }

    validate_taxonomy(&transaction)?;
    let mut changeset_blob = Vec::new();
    session.changeset_strm(&mut changeset_blob)?;
    drop(session);
    let operation_id = operations::insert_operation(
        &transaction,
        NewOperation {
            kind: "taxonomy_name_group_save",
            source: "ui_name_group",
            total_items: 1,
            succeeded_items: 1,
            failed_items: 0,
            rollbackable: true,
            has_formatted_input: false,
        },
    )?;
    operations::insert_operation_input(&transaction, operation_id, &action_input)?;
    transaction.execute(
        r#"
        INSERT INTO operation_changesets (operation_id, changeset_blob)
        VALUES (?, ?)
        "#,
        params![operation_id, changeset_blob],
    )?;
    operations::insert_audit_row(
        &transaction,
        operation_id,
        NewAuditRow {
            sequence: 1,
            entity_type: "taxon_name_group",
            entity_id: Some(input.taxon_id.to_string()),
            action: "save",
            before_json: Some(serde_json::json!({
                "taxon_id": input.taxon_id,
                "name_type": input.name_type.as_str(),
                "records": before_records,
            })),
            after_json: Some(serde_json::json!({
                "taxon_id": input.taxon_id,
                "name_type": input.name_type.as_str(),
                "records": after_records,
            })),
            succeeded: true,
            message: "saved taxonomy name group",
        },
    )?;
    super::sync::record_event(&transaction, Some(operation_id), [input.taxon_id], false)?;
    transaction.commit()?;
    Ok(())
}

fn validate_name_group_additions(
    transaction: &rusqlite::Transaction<'_>,
    taxon_id: i64,
    rank: TaxonRank,
    parent_taxon_id: Option<i64>,
    name_type: TaxonomyNameType,
    additions: &[NewTaxonNameInput],
) -> CoreResult<()> {
    if additions.is_empty() {
        return Ok(());
    }
    if name_type.is_primary() && additions.len() > 1 {
        return Err(CoreError::InvalidArgument(format!(
            "{} accepts only one name",
            name_type.as_str()
        )));
    }
    let accepted_exists: bool = transaction.query_row(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM taxon_names
            WHERE taxon_id = ? AND name_type = ?
        )
        "#,
        params![taxon_id, name_type.accepted_type().code()],
        |row| row.get(0),
    )?;
    if name_type.is_primary() && accepted_exists {
        return Err(CoreError::InvalidArgument(format!(
            "taxon {taxon_id} already has {}",
            name_type.as_str()
        )));
    }
    if !name_type.is_primary() && !accepted_exists {
        return Err(CoreError::InvalidArgument(format!(
            "add {} before adding {} records",
            name_type.accepted_type().as_str(),
            name_type.as_str()
        )));
    }

    let parent_scientific_name = if rank == TaxonRank::Species
        && matches!(
            name_type,
            TaxonomyNameType::SciName | TaxonomyNameType::Synonym
        ) {
        let parent_taxon_id = parent_taxon_id.ok_or_else(|| {
            CoreError::InvalidArgument(format!("species taxon {taxon_id} has no parent genus"))
        })?;
        Some(
            transaction
                .query_row(
                    r#"
                    SELECT name FROM taxon_names
                    WHERE taxon_id = ? AND name_type = ?
                    "#,
                    params![parent_taxon_id, TaxonomyNameType::SciName.code()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .ok_or_else(|| {
                    CoreError::InvalidArgument(format!(
                        "parent genus {parent_taxon_id} has no sci_name"
                    ))
                })?,
        )
    } else {
        None
    };

    let accepted_type = name_type.accepted_type();
    let alias_type = name_type.alias_type();
    let mut seen_names = HashSet::new();
    for addition in additions {
        let name = normalize_taxonomy_name(&addition.name)
            .ok_or_else(|| CoreError::InvalidArgument("taxonomy name must not be blank".into()))?;
        if !seen_names.insert(name.clone()) {
            return Err(CoreError::InvalidArgument(format!(
                "taxonomy name '{name}' is included more than once"
            )));
        }
        if let Some(parent_name) = parent_scientific_name.as_deref()
            && name.split_whitespace().next() != Some(parent_name)
        {
            return Err(CoreError::InvalidArgument(format!(
                "species scientific name '{name}' does not start with parent genus '{parent_name}'"
            )));
        }
        let duplicate: bool = transaction.query_row(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM taxon_names
                WHERE taxon_id = ?
                  AND name_type IN (?, ?)
                  AND name = ? COLLATE BINARY
            )
            "#,
            params![taxon_id, accepted_type.code(), alias_type.code(), name],
            |row| row.get(0),
        )?;
        if duplicate {
            return Err(CoreError::InvalidArgument(format!(
                "taxonomy name '{name}' already exists in this name group"
            )));
        }
    }
    Ok(())
}

fn normalized_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn validate_taxon_name_deletion(
    connection: &Connection,
    taxon_id: i64,
    name_type: TaxonomyNameType,
) -> CoreResult<()> {
    if name_type == TaxonomyNameType::SciName {
        return Err(CoreError::InvalidArgument(
            "the unique sci_name cannot be deleted".into(),
        ));
    }
    let (alias_type, language) = match name_type {
        TaxonomyNameType::ZhName => (TaxonomyNameType::ZhAlias, "Chinese"),
        TaxonomyNameType::EnName => (TaxonomyNameType::EnAlias, "English"),
        _ => return Ok(()),
    };
    let aliases_exist = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM taxon_names WHERE taxon_id = ? AND name_type = ?)",
        params![taxon_id, alias_type.code()],
        |row| row.get::<_, bool>(0),
    )?;
    if aliases_exist {
        return Err(CoreError::InvalidArgument(format!(
            "{language} accepted name cannot be deleted while {language} aliases exist"
        )));
    }
    Ok(())
}

pub fn delete_taxon_name(database: &Database, input: DeleteTaxonNameInput) -> CoreResult<()> {
    let action_input = taxonomy_action_input("delete_taxonomy_name", &input)?;
    let _guard = database.try_taxonomy_mutation()?;
    let mut connection = database.connect_taxonomy_metadata_context()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (name_type, name) = transaction
        .query_row(
            "SELECT name_type, name FROM taxon_names WHERE taxon_id = ? AND name_id = ?",
            params![input.taxon_id, input.name_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            CoreError::NotFound(format!(
                "name {} for taxon {}",
                input.name_id, input.taxon_id
            ))
        })?;
    validate_taxon_name_deletion(
        &transaction,
        input.taxon_id,
        TaxonomyNameType::from_code(name_type)?,
    )?;
    let mut session = start_taxonomy_session(&transaction)?;
    let deleted = transaction.execute(
        "DELETE FROM taxon_names WHERE taxon_id = ? AND name_id = ?",
        params![input.taxon_id, input.name_id],
    )?;
    if deleted == 0 {
        return Err(CoreError::NotFound(format!(
            "name {} ('{}') for taxon {}",
            input.name_id, name, input.taxon_id
        )));
    }
    validate_taxonomy(&transaction)?;
    let mut changeset_blob = Vec::new();
    session.changeset_strm(&mut changeset_blob)?;
    drop(session);
    let operation_id = operations::insert_operation(
        &transaction,
        NewOperation {
            kind: "taxonomy_delete",
            source: "ui_delete",
            total_items: 1,
            succeeded_items: 1,
            failed_items: 0,
            rollbackable: true,
            has_formatted_input: false,
        },
    )?;
    operations::insert_operation_input(&transaction, operation_id, &action_input)?;
    transaction.execute(
        r#"
        INSERT INTO operation_changesets (operation_id, changeset_blob)
        VALUES (?, ?)
        "#,
        params![operation_id, changeset_blob],
    )?;
    operations::insert_audit_row(
        &transaction,
        operation_id,
        NewAuditRow {
            sequence: 1,
            entity_type: "taxon_name",
            entity_id: Some(input.name_id.to_string()),
            action: "delete",
            before_json: Some(serde_json::json!({
                "taxon_id": input.taxon_id,
                "name_id": input.name_id,
                "name_type": name_type,
                "name": name,
            })),
            after_json: None,
            succeeded: true,
            message: "deleted taxonomy name",
        },
    )?;
    super::sync::record_event(&transaction, Some(operation_id), [input.taxon_id], false)?;
    transaction.commit()?;
    Ok(())
}

pub fn delete_taxon(database: &Database, taxon_id: i64) -> CoreResult<()> {
    let action_input = taxonomy_action_input(
        "delete_taxon",
        &serde_json::json!({
            "taxon_id": taxon_id,
        }),
    )?;
    let _guard = database.try_taxonomy_mutation()?;
    let mut connection = database.connect_taxonomy_metadata_context()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let taxon = transaction
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
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?
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
    let mut session = start_taxonomy_session(&transaction)?;
    transaction.execute("DELETE FROM taxa WHERE taxon_id = ?", [taxon_id])?;
    let mut changeset_blob = Vec::new();
    session.changeset_strm(&mut changeset_blob)?;
    drop(session);
    let operation_id = operations::insert_operation(
        &transaction,
        NewOperation {
            kind: "taxonomy_delete",
            source: "ui_delete",
            total_items: 1,
            succeeded_items: 1,
            failed_items: 0,
            rollbackable: true,
            has_formatted_input: false,
        },
    )?;
    operations::insert_operation_input(&transaction, operation_id, &action_input)?;
    transaction.execute(
        r#"
        INSERT INTO operation_changesets (operation_id, changeset_blob)
        VALUES (?, ?)
        "#,
        params![operation_id, changeset_blob],
    )?;
    operations::insert_audit_row(
        &transaction,
        operation_id,
        NewAuditRow {
            sequence: 1,
            entity_type: "taxon",
            entity_id: Some(taxon_id.to_string()),
            action: "delete",
            before_json: Some(serde_json::json!({
                "taxon_id": taxon_id,
                "parent_taxon_id": taxon.0,
                "rank": taxon.1,
                "geological_range": taxon.2,
            })),
            after_json: None,
            succeeded: true,
            message: "deleted taxon",
        },
    )?;
    super::sync::record_event(&transaction, Some(operation_id), [taxon_id], false)?;
    transaction.commit()?;
    Ok(())
}

fn taxonomy_action_input<T: Serialize>(action: &str, input: &T) -> CoreResult<OperationInput> {
    Ok(OperationInput::TaxonomyAction {
        action: action.into(),
        input: serde_json::to_value(input).map_err(|error| {
            CoreError::InvalidArgument(format!("invalid taxonomy action input: {error}"))
        })?,
    })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::taxonomy::{TaxonInputRow, apply_rows};

    fn database() -> (TempDir, Database) {
        let directory = TempDir::new().unwrap();
        let database = Database::open_test(directory.path().join("test.db")).unwrap();
        (directory, database)
    }

    fn canis_species(database: &Database) -> (i64, i64, i64) {
        apply_rows(
            database,
            &[
                TaxonInputRow {
                    kingdom: Some("Animalia".into()),
                    ..TaxonInputRow::default()
                },
                TaxonInputRow {
                    kingdom: Some("Animalia".into()),
                    order: Some("Carnivora".into()),
                    ..TaxonInputRow::default()
                },
                TaxonInputRow {
                    kingdom: Some("Animalia".into()),
                    order: Some("Carnivora".into()),
                    family: Some("Canidae".into()),
                    ..TaxonInputRow::default()
                },
                TaxonInputRow {
                    kingdom: Some("Animalia".into()),
                    order: Some("Carnivora".into()),
                    family: Some("Canidae".into()),
                    genus: Some("Canis".into()),
                    ..TaxonInputRow::default()
                },
                TaxonInputRow {
                    kingdom: Some("Animalia".into()),
                    order: Some("Carnivora".into()),
                    family: Some("Canidae".into()),
                    genus: Some("Canis".into()),
                    species: Some("Canis lupus".into()),
                    synonyms: vec!["Canis lycaon".into()],
                    ..TaxonInputRow::default()
                },
            ],
        )
        .unwrap();
        database
            .connect_taxonomy_metadata_context()
            .unwrap()
            .query_row(
                r#"
                SELECT sci.taxon_id, sci.name_id, synonym.name_id
                FROM taxon_names AS sci
                JOIN taxon_names AS synonym USING (taxon_id)
                WHERE sci.name = 'Canis lupus' AND synonym.name = 'Canis lycaon'
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
    }

    #[test]
    fn saving_a_name_group_updates_metadata_adds_names_and_rolls_back() {
        let (_directory, database) = database();
        let (taxon_id, _sci_name_id, synonym_id) = canis_species(&database);

        save_taxon_name_group(
            &database,
            SaveTaxonNameGroupInput {
                taxon_id,
                name_type: TaxonomyNameType::Synonym,
                updates: vec![TaxonNameMetadataInput {
                    name_id: synonym_id,
                    authority_year: Some("  Schreber, 1775 ".into()),
                    source: Some(" Revised checklist ".into()),
                }],
                additions: vec![NewTaxonNameInput {
                    name: " Canis familiaris ".into(),
                    authority_year: Some("Linnaeus, 1758".into()),
                    source: Some("Catalogue".into()),
                }],
            },
        )
        .unwrap();

        let connection = database.connect_taxonomy_metadata_context().unwrap();
        let updated: (Option<String>, Option<String>) = connection
            .query_row(
                "SELECT authority_year, source FROM taxon_names WHERE name_id = ?",
                [synonym_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(updated.0.as_deref(), Some("Schreber, 1775"));
        assert_eq!(updated.1.as_deref(), Some("Revised checklist"));
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM taxon_names WHERE taxon_id = ? AND name = 'Canis familiaris' AND authority_year = 'Linnaeus, 1758' AND source = 'Catalogue'",
                    [taxon_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        drop(connection);

        let operation = crate::taxonomy::list_operations(&database, None, 1)
            .unwrap()
            .items
            .remove(0);
        assert_eq!(operation.kind, "taxonomy_name_group_save");
        assert!(operation.rollbackable);
        match crate::taxonomy::get_operation_input(&database, operation.operation_id).unwrap() {
            Some(OperationInput::TaxonomyAction { action, input }) => {
                assert_eq!(action, "edit_name_group");
                assert_eq!(input["taxon_id"], taxon_id);
                assert_eq!(input["name_type"], "synonym");
            }
            input => panic!("unexpected taxonomy action input: {input:?}"),
        }
        crate::taxonomy::rollback_operation(&database, operation.operation_id).unwrap();
        let connection = database.connect_taxonomy_metadata_context().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM taxon_names WHERE taxon_id = ? AND name = 'Canis familiaris'",
                    [taxon_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        let restored: (Option<String>, Option<String>) = connection
            .query_row(
                "SELECT authority_year, source FROM taxon_names WHERE name_id = ?",
                [synonym_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(restored, (None, None));
    }

    #[test]
    fn saving_a_name_group_validates_type_primary_and_species_names() {
        let (_directory, database) = database();
        let (taxon_id, sci_name_id, _synonym_id) = canis_species(&database);

        let exact_duplicate = save_taxon_name_group(
            &database,
            SaveTaxonNameGroupInput {
                taxon_id,
                name_type: TaxonomyNameType::Synonym,
                updates: vec![],
                additions: vec![NewTaxonNameInput {
                    name: "Canis lycaon".into(),
                    authority_year: None,
                    source: None,
                }],
            },
        )
        .unwrap_err();
        assert!(exact_duplicate.to_string().contains("already exists"));

        save_taxon_name_group(
            &database,
            SaveTaxonNameGroupInput {
                taxon_id,
                name_type: TaxonomyNameType::Synonym,
                updates: vec![],
                additions: vec![NewTaxonNameInput {
                    name: "Canis Lycaon".into(),
                    authority_year: None,
                    source: None,
                }],
            },
        )
        .unwrap();

        let invalid_species = save_taxon_name_group(
            &database,
            SaveTaxonNameGroupInput {
                taxon_id,
                name_type: TaxonomyNameType::Synonym,
                updates: vec![],
                additions: vec![NewTaxonNameInput {
                    name: "Felis lupus".into(),
                    authority_year: None,
                    source: None,
                }],
            },
        )
        .unwrap_err();
        assert!(invalid_species.to_string().contains("parent genus"));

        let missing_primary = save_taxon_name_group(
            &database,
            SaveTaxonNameGroupInput {
                taxon_id,
                name_type: TaxonomyNameType::ZhAlias,
                updates: vec![],
                additions: vec![NewTaxonNameInput {
                    name: "wolf".into(),
                    authority_year: None,
                    source: None,
                }],
            },
        )
        .unwrap_err();
        assert!(missing_primary.to_string().contains("add zh_name"));

        let wrong_group = save_taxon_name_group(
            &database,
            SaveTaxonNameGroupInput {
                taxon_id,
                name_type: TaxonomyNameType::Synonym,
                updates: vec![TaxonNameMetadataInput {
                    name_id: sci_name_id,
                    authority_year: None,
                    source: None,
                }],
                additions: vec![],
            },
        )
        .unwrap_err();
        assert!(wrong_group.to_string().contains("not synonym"));

        let duplicate_primary = save_taxon_name_group(
            &database,
            SaveTaxonNameGroupInput {
                taxon_id,
                name_type: TaxonomyNameType::SciName,
                updates: vec![],
                additions: vec![NewTaxonNameInput {
                    name: "Canis familiaris".into(),
                    authority_year: None,
                    source: None,
                }],
            },
        )
        .unwrap_err();
        assert!(
            duplicate_primary
                .to_string()
                .contains("already has sci_name")
        );
    }

    #[test]
    fn promoting_a_species_synonym_does_not_require_the_parent_genus_prefix() {
        let (_directory, database) = database();
        apply_rows(
            &database,
            &[
                TaxonInputRow {
                    kingdom: Some("Animalia".into()),
                    ..TaxonInputRow::default()
                },
                TaxonInputRow {
                    kingdom: Some("Animalia".into()),
                    order: Some("Carnivora".into()),
                    ..TaxonInputRow::default()
                },
                TaxonInputRow {
                    kingdom: Some("Animalia".into()),
                    order: Some("Carnivora".into()),
                    family: Some("Canidae".into()),
                    ..TaxonInputRow::default()
                },
                TaxonInputRow {
                    kingdom: Some("Animalia".into()),
                    order: Some("Carnivora".into()),
                    family: Some("Canidae".into()),
                    genus: Some("Canis".into()),
                    ..TaxonInputRow::default()
                },
                TaxonInputRow {
                    kingdom: Some("Animalia".into()),
                    order: Some("Carnivora".into()),
                    family: Some("Canidae".into()),
                    genus: Some("Canis".into()),
                    species: Some("Canis lupus".into()),
                    synonyms: vec!["Canis lycaon".into(), "Felis lupus".into()],
                    ..TaxonInputRow::default()
                },
            ],
        )
        .unwrap();
        let connection = database.connect_taxonomy_metadata_context().unwrap();
        let (taxon_id, promoted_name_id, invalid_name_id): (i64, i64, i64) = connection
            .query_row(
                r#"
                SELECT accepted.taxon_id, promoted.name_id, invalid.name_id
                FROM taxon_names AS accepted
                JOIN taxon_names AS promoted
                  ON promoted.taxon_id = accepted.taxon_id
                 AND promoted.name = 'Canis lycaon'
                JOIN taxon_names AS invalid
                  ON invalid.taxon_id = accepted.taxon_id
                 AND invalid.name = 'Felis lupus'
                WHERE accepted.name = 'Canis lupus'
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        drop(connection);

        promote_taxon_name(
            &database,
            PromoteTaxonNameInput {
                taxon_id,
                name_id: promoted_name_id,
            },
        )
        .unwrap();
        let connection = database.connect_taxonomy_metadata_context().unwrap();
        let promoted: i64 = connection
            .query_row(
                "SELECT name_type FROM taxon_names WHERE name = 'Canis lycaon'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let previous: i64 = connection
            .query_row(
                "SELECT name_type FROM taxon_names WHERE name = 'Canis lupus'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(promoted, TaxonomyNameType::SciName.code());
        assert_eq!(previous, TaxonomyNameType::Synonym.code());
        drop(connection);
        let operation = crate::taxonomy::list_operations(&database, None, 1)
            .unwrap()
            .items
            .remove(0);
        assert_eq!(operation.kind, "taxonomy_name_promote");
        assert!(operation.rollbackable);
        assert!(!operation.has_formatted_input);

        crate::taxonomy::rollback_operation(&database, operation.operation_id).unwrap();
        promote_taxon_name(
            &database,
            PromoteTaxonNameInput {
                taxon_id,
                name_id: invalid_name_id,
            },
        )
        .unwrap();
        let connection = database.connect_taxonomy_metadata_context().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT name_type FROM taxon_names WHERE name = 'Felis lupus'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            TaxonomyNameType::SciName.code()
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT name_type FROM taxon_names WHERE name = 'Canis lupus'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            TaxonomyNameType::Synonym.code()
        );
    }

    #[test]
    fn deleting_localized_accepted_names_without_aliases_succeeds() {
        let (_directory, database) = database();
        apply_rows(
            &database,
            &[TaxonInputRow {
                kingdom: Some("Animalia".into()),
                zh_name: Some("Animal".into()),
                en_name: Some("Animal kingdom".into()),
                ..TaxonInputRow::default()
            }],
        )
        .unwrap();
        let connection = database.connect_taxonomy_metadata_context().unwrap();
        let taxon_id: i64 = connection
            .query_row(
                "SELECT taxon_id FROM taxon_names WHERE name = 'Animalia'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let zh_name_id: i64 = connection
            .query_row(
                "SELECT name_id FROM taxon_names WHERE taxon_id = ? AND name_type = ?",
                params![taxon_id, TaxonomyNameType::ZhName.code()],
                |row| row.get(0),
            )
            .unwrap();
        let en_name_id: i64 = connection
            .query_row(
                "SELECT name_id FROM taxon_names WHERE taxon_id = ? AND name_type = ?",
                params![taxon_id, TaxonomyNameType::EnName.code()],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);

        delete_taxon_name(
            &database,
            DeleteTaxonNameInput {
                taxon_id,
                name_id: zh_name_id,
            },
        )
        .unwrap();
        delete_taxon_name(
            &database,
            DeleteTaxonNameInput {
                taxon_id,
                name_id: en_name_id,
            },
        )
        .unwrap();

        let connection = database.connect_taxonomy_metadata_context().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM taxon_names WHERE taxon_id = ? AND name_type IN (?, ?)",
                    params![
                        taxon_id,
                        TaxonomyNameType::ZhName.code(),
                        TaxonomyNameType::EnName.code()
                    ],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        validate_taxonomy(&connection).unwrap();
    }

    #[test]
    fn localized_aliases_block_accepted_name_deletion_until_removed() {
        let (_directory, database) = database();
        apply_rows(
            &database,
            &[TaxonInputRow {
                kingdom: Some("Animalia".into()),
                zh_name: Some("Animal".into()),
                zh_alias: vec!["Creature".into()],
                en_name: Some("Animal kingdom".into()),
                en_alias: vec!["Animals".into()],
                ..TaxonInputRow::default()
            }],
        )
        .unwrap();
        let connection = database.connect_taxonomy_metadata_context().unwrap();
        let taxon_id: i64 = connection
            .query_row(
                "SELECT taxon_id FROM taxon_names WHERE name = 'Animalia'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let name_id = |name_type: TaxonomyNameType| {
            connection
                .query_row(
                    "SELECT name_id FROM taxon_names WHERE taxon_id = ? AND name_type = ?",
                    params![taxon_id, name_type.code()],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap()
        };
        let zh_name_id = name_id(TaxonomyNameType::ZhName);
        let zh_alias_id = name_id(TaxonomyNameType::ZhAlias);
        let en_name_id = name_id(TaxonomyNameType::EnName);
        let en_alias_id = name_id(TaxonomyNameType::EnAlias);
        drop(connection);

        let zh_error = delete_taxon_name(
            &database,
            DeleteTaxonNameInput {
                taxon_id,
                name_id: zh_name_id,
            },
        )
        .unwrap_err();
        assert_eq!(
            zh_error.to_string(),
            "invalid argument: Chinese accepted name cannot be deleted while Chinese aliases exist"
        );
        let en_error = delete_taxon_name(
            &database,
            DeleteTaxonNameInput {
                taxon_id,
                name_id: en_name_id,
            },
        )
        .unwrap_err();
        assert_eq!(
            en_error.to_string(),
            "invalid argument: English accepted name cannot be deleted while English aliases exist"
        );

        for name_id in [zh_alias_id, zh_name_id, en_alias_id, en_name_id] {
            delete_taxon_name(&database, DeleteTaxonNameInput { taxon_id, name_id }).unwrap();
        }
        let connection = database.connect_taxonomy_metadata_context().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM taxon_names WHERE taxon_id = ? AND name_type IN (?, ?, ?, ?)",
                    params![
                        taxon_id,
                        TaxonomyNameType::ZhName.code(),
                        TaxonomyNameType::ZhAlias.code(),
                        TaxonomyNameType::EnName.code(),
                        TaxonomyNameType::EnAlias.code()
                    ],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn name_actions_require_matching_taxon_and_name_ids() {
        let (_directory, database) = database();
        apply_rows(
            &database,
            &[
                TaxonInputRow {
                    kingdom: Some("Animalia".into()),
                    synonyms: vec!["Metazoa".into()],
                    ..TaxonInputRow::default()
                },
                TaxonInputRow {
                    kingdom: Some("Plantae".into()),
                    ..TaxonInputRow::default()
                },
            ],
        )
        .unwrap();
        let connection = database.connect_taxonomy_metadata_context().unwrap();
        let (animalia_id, sci_name_id, synonym_id): (i64, i64, i64) = connection
            .query_row(
                r#"
                SELECT sci.taxon_id, sci.name_id, synonym.name_id
                FROM taxon_names AS sci
                JOIN taxon_names AS synonym USING (taxon_id)
                WHERE sci.name = 'Animalia' AND synonym.name = 'Metazoa'
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let plantae_id: i64 = connection
            .query_row(
                "SELECT taxon_id FROM taxon_names WHERE name = 'Plantae'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);
        let detail = crate::taxonomy::get_taxon_detail(&database, animalia_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            detail.names.sci_name.as_ref().map(|name| name.name_id),
            Some(sci_name_id)
        );
        assert_eq!(detail.names.synonyms[0].name_id, synonym_id);

        let mismatch = delete_taxon_name(
            &database,
            DeleteTaxonNameInput {
                taxon_id: plantae_id,
                name_id: synonym_id,
            },
        )
        .unwrap_err();
        assert!(mismatch.to_string().contains("not found"));
        let sci_error = delete_taxon_name(
            &database,
            DeleteTaxonNameInput {
                taxon_id: animalia_id,
                name_id: sci_name_id,
            },
        )
        .unwrap_err();
        assert!(sci_error.to_string().contains("cannot be deleted"));

        delete_taxon_name(
            &database,
            DeleteTaxonNameInput {
                taxon_id: animalia_id,
                name_id: synonym_id,
            },
        )
        .unwrap();
        crate::taxonomy::synchronize_pending_photo_libraries(&database).unwrap();
        assert!(
            database
                .connect_taxonomy_metadata_context()
                .unwrap()
                .query_row(
                    "SELECT NOT EXISTS(SELECT 1 FROM taxon_names WHERE name_id = ?)",
                    [synonym_id],
                    |row| row.get::<_, bool>(0),
                )
                .unwrap()
        );
        let operation = crate::taxonomy::list_operations(&database, None, 1)
            .unwrap()
            .items
            .remove(0);
        assert_eq!(operation.kind, "taxonomy_delete");
        assert!(!operation.has_formatted_input);
        crate::taxonomy::rollback_operation(&database, operation.operation_id).unwrap();
        assert!(
            database
                .connect_taxonomy_metadata_context()
                .unwrap()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM taxon_names WHERE name_id = ?)",
                    [synonym_id],
                    |row| row.get::<_, bool>(0),
                )
                .unwrap()
        );
    }

    #[test]
    fn custom_taxon_delete_queues_old_matches() {
        let (_directory, database) = database();
        let connection = database.connect().unwrap();
        connection
            .execute("INSERT INTO taxa (rank) VALUES (5)", [])
            .unwrap();
        let taxon_id = connection.last_insert_rowid();
        connection
            .execute(
                r#"
                INSERT INTO taxon_names (taxon_id, name_type, name)
                VALUES (?, 1, 'Felis catus')
                "#,
                [taxon_id],
            )
            .unwrap();
        let name_id = connection.last_insert_rowid();
        connection
            .execute_batch(
                r#"
                INSERT INTO photo_directories (
                    directory_id, parent_directory_id, name, relative_path
                ) VALUES (1, NULL, '', '');
                INSERT INTO photos (
                    photo_id, directory_id, filename, file_size, modified_at_ns
                ) VALUES
                    (1, 1, 'Felis catus.jpg', 1, 1),
                    (2, 1, 'Ambiguous cat.jpg', 1, 1);
                "#,
            )
            .unwrap();
        connection
            .execute(
                r#"
                INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
                VALUES (1, ?, 'matched')
                "#,
                [taxon_id],
            )
            .unwrap();
        connection
            .execute(
                r#"
                INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
                VALUES (2, NULL, 'ambiguous')
                "#,
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO photo_taxon_candidates (photo_id, taxon_id) VALUES (2, ?)",
                [taxon_id],
            )
            .unwrap();
        connection
            .execute(
                r#"
                INSERT INTO photo_taxon_candidate_names (
                    photo_id, taxon_id, name_id, name_type, name
                ) VALUES (2, ?, ?, 1, 'Felis catus')
                "#,
                params![taxon_id, name_id],
            )
            .unwrap();
        connection
            .execute(
                r#"
                INSERT INTO photo_taxon_usage (
                    taxon_id, direct_photo_count, subtree_photo_count
                ) VALUES (?, 1, 1)
                "#,
                [taxon_id],
            )
            .unwrap();
        drop(connection);

        let result = crate::taxonomy::execute_custom_taxonomy_sql(
            &database,
            &crate::taxonomy::CustomTaxonomySqlRequest {
                sql: format!("DELETE FROM taxa WHERE taxon_id = {taxon_id}"),
                maximum_result_rows: None,
            },
        )
        .unwrap();
        crate::taxonomy::synchronize_pending_photo_libraries(&database).unwrap();
        assert!(
            crate::taxonomy::export_operation_input(&database, result.operation_id.unwrap())
                .unwrap_err()
                .to_string()
                .contains("formatted input")
        );

        let connection = database.connect().unwrap();
        let queued: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM photo_mapping_queue WHERE photo_id IN (1, 2)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(queued, 2);
    }
}
