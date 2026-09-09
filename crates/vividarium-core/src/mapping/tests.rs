use super::*;
use crate::naming::{NamingHookKind, set_naming_hook, take_hook_compile_count};
use crate::photos::{self, open_library, refresh_directory};
use crate::taxonomy::{
    CustomTaxonomySqlRequest, TaxonInputRow, apply_rows, execute_custom_taxonomy_sql,
};
use std::fs;

fn insert_test_photo(connection: &rusqlite::Connection, directory_id: i64, filename: &str) -> i64 {
    connection
        .execute(
            r#"
                INSERT INTO photos (
                    directory_id, filename, file_size, modified_at_ns
                ) VALUES (?, ?, 1, 1)
                "#,
            params![directory_id, filename],
        )
        .unwrap();
    connection.last_insert_rowid()
}

#[test]
fn one_mapping_run_compiles_the_hook_once_across_batches() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    let connection = database.connect().unwrap();
    for index in 0..PHOTO_MAPPING_BATCH_SIZE + 1 {
        let photo_id = insert_test_photo(
            &connection,
            library.root_directory_id,
            &format!("Unknown {index}.jpg"),
        );
        connection
            .execute(
                "INSERT INTO photo_mapping_queue (photo_id, reason) VALUES (?, 'refresh')",
                [photo_id],
            )
            .unwrap();
    }
    drop(connection);

    take_hook_compile_count();
    let mut progress = |_: OperationProgress| {};
    let result = process_pending_photo_matches(&database, &mut progress).unwrap();

    assert_eq!(result.processed, PHOTO_MAPPING_BATCH_SIZE + 1);
    assert_eq!(take_hook_compile_count(), 1);
}

#[test]
fn six_dimension_priority_controls_photo_mapping() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("input.jpg"), b"photo").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    apply_rows(
        &database,
        &[
            TaxonInputRow {
                kingdom: Some("Animalia".into()),
                ..Default::default()
            },
            TaxonInputRow {
                kingdom: Some("Animalia".into()),
                order: Some("Carnivora".into()),
                ..Default::default()
            },
            TaxonInputRow {
                kingdom: Some("Animalia".into()),
                order: Some("Carnivora".into()),
                family: Some("Canidae".into()),
                ..Default::default()
            },
            TaxonInputRow {
                kingdom: Some("Animalia".into()),
                order: Some("Carnivora".into()),
                family: Some("Canidae".into()),
                genus: Some("Canis".into()),
                ..Default::default()
            },
            TaxonInputRow {
                kingdom: Some("Animalia".into()),
                order: Some("Carnivora".into()),
                family: Some("Canidae".into()),
                genus: Some("Canis".into()),
                species: Some("Canis lupus".into()),
                zh_name: Some("wolf".into()),
                ..Default::default()
            },
        ],
    )
    .unwrap();
    set_naming_hook(
        &database,
        NamingHookKind::PhotoFilename,
        Some(
            r#"
                fn parse_photo_filename(filename) {
                    #{
                        info: #{
                            family_sci: "Canidae",
                            species_zh: "wolf"
                        },
                        suffix: ".jpg"
                    }
                }
                "#,
        ),
    )
    .unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = photos::list_photos(&database).unwrap().remove(0);
    let mut progress = |_: OperationProgress| {};
    process_pending_photo_matches(&database, &mut progress).unwrap();
    let species_mapping = get_photo_mapping(&database, photo.photo_id).unwrap();
    assert_eq!(species_mapping.status, PhotoTaxonStatus::Matched);
    assert!(
        get_photo_mapping_detail(&database, photo.photo_id)
            .unwrap()
            .candidates
            .is_empty()
    );
    let species_summary =
        crate::taxonomy::get_taxon_summary(&database, species_mapping.taxon_id.unwrap())
            .unwrap()
            .unwrap();
    assert_eq!(species_summary.names.zh_name.as_deref(), Some("wolf"));

    set_photo_name_match_settings(
        &database,
        &PhotoNameMatchSettings {
            priority: vec![
                PhotoNameField::FamilySci,
                PhotoNameField::SpeciesSci,
                PhotoNameField::SpeciesZh,
                PhotoNameField::GenusSci,
                PhotoNameField::GenusZh,
                PhotoNameField::FamilyZh,
            ],
        },
    )
    .unwrap();
    assert_eq!(
        database
            .connect()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM photo_mapping_queue", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
    let unchanged_mapping = get_photo_mapping(&database, photo.photo_id).unwrap();
    assert_eq!(unchanged_mapping.taxon_id, species_mapping.taxon_id);

    remap_photo(&database, photo.photo_id).unwrap();
    process_pending_photo_matches(&database, &mut progress).unwrap();
    let family_mapping = get_photo_mapping(&database, photo.photo_id).unwrap();
    assert_eq!(family_mapping.status, PhotoTaxonStatus::Matched);
    assert!(
        get_photo_mapping_detail(&database, photo.photo_id)
            .unwrap()
            .candidates
            .is_empty()
    );
    let family_summary =
        crate::taxonomy::get_taxon_summary(&database, family_mapping.taxon_id.unwrap())
            .unwrap()
            .unwrap();
    assert_eq!(family_summary.rank, TaxonRank::Family);
}

#[test]
fn matches_the_filename_stem_and_builds_sparse_usage() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("Canis lupus.jpg"), b"photo").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let rows = [
        TaxonInputRow {
            kingdom: Some("Animalia".into()),
            ..Default::default()
        },
        TaxonInputRow {
            kingdom: Some("Animalia".into()),
            order: Some("Carnivora".into()),
            ..Default::default()
        },
        TaxonInputRow {
            kingdom: Some("Animalia".into()),
            order: Some("Carnivora".into()),
            family: Some("Canidae".into()),
            ..Default::default()
        },
        TaxonInputRow {
            kingdom: Some("Animalia".into()),
            order: Some("Carnivora".into()),
            family: Some("Canidae".into()),
            genus: Some("Canis".into()),
            ..Default::default()
        },
        TaxonInputRow {
            kingdom: Some("Animalia".into()),
            order: Some("Carnivora".into()),
            family: Some("Canidae".into()),
            genus: Some("Canis".into()),
            species: Some("Canis lupus".into()),
            ..Default::default()
        },
    ];
    apply_rows(&database, &rows).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let mut progress = |_: OperationProgress| {};
    process_pending_photo_matches(&database, &mut progress).unwrap();
    let photo = photos::list_photos(&database).unwrap().remove(0);
    let mapping = get_photo_mapping(&database, photo.photo_id).unwrap();
    assert_eq!(mapping.status, PhotoTaxonStatus::Matched);
    let detail = get_photo_mapping_detail(&database, photo.photo_id).unwrap();
    assert!(detail.candidates.is_empty());
    assert_eq!(detail.matched_names.len(), 1);
    assert_eq!(detail.matched_names[0].name_type, TaxonomyNameType::SciName);
    assert_eq!(detail.matched_names[0].name, "Canis lupus");
    let species_id = mapping.taxon_id.unwrap();
    let species_summary = crate::taxonomy::get_taxon_summary(&database, species_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        species_summary.names.sci_name.as_deref(),
        Some("Canis lupus")
    );
    assert_eq!(mapping.taxon_id, Some(species_id));
    let node = get_photo_taxon_node(&database, mapping.taxon_id, false).unwrap();
    assert_eq!(node.taxon.as_ref().unwrap().direct_photo_count, 1);
    assert_eq!(node.subtree_photo_count, 1);
    let sparse_root = get_photo_taxon_node(&database, None, false).unwrap();
    assert_eq!(sparse_root.subtree_photo_count, 1);
    let root_page = browse_photo_taxon(&database, None, false, None, 20).unwrap();
    assert!(matches!(root_page.items[0], PhotoTaxonItem::Taxon { .. }));
    assert!(
        root_page
            .items
            .iter()
            .all(|item| matches!(item, PhotoTaxonItem::Taxon { .. }))
    );
    assert_eq!(
        get_photo_taxon_counts(&database, None).unwrap(),
        PhotoTaxonEntryCounts {
            taxon_count: 1,
            photo_count: 0
        }
    );
    let connection = database.connect().unwrap();
    let genus_id: i64 = connection
        .query_row(
            "SELECT parent_taxon_id FROM taxa WHERE taxon_id = ?",
            [species_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        get_photo_taxon_counts(&database, genus_id.into()).unwrap(),
        PhotoTaxonEntryCounts {
            taxon_count: 1,
            photo_count: 0
        }
    );
    assert_eq!(
        get_photo_taxon_counts(&database, species_id.into()).unwrap(),
        PhotoTaxonEntryCounts {
            taxon_count: 0,
            photo_count: 1
        }
    );
    let page = browse_photo_taxon(&database, mapping.taxon_id, false, None, 20).unwrap();
    assert_eq!(
        page.items,
        vec![PhotoTaxonItem::Photo {
            photo: photo.clone()
        }]
    );
    assert_eq!(page.next_cursor, None);
    execute_custom_taxonomy_sql(
        &database,
        &CustomTaxonomySqlRequest {
            sql: "UPDATE taxon_names SET name = 'Canis lycaon' WHERE name = 'Canis lupus'".into(),
            maximum_result_rows: None,
        },
    )
    .unwrap();
    crate::taxonomy::synchronize_pending_photo_libraries(&database).unwrap();
    process_pending_photo_matches(&database, &mut progress).unwrap();
    let old_taxon_id = mapping.taxon_id;
    let mapping = get_photo_mapping(&database, mapping.photo_id).unwrap();
    assert_eq!(mapping.status, PhotoTaxonStatus::Matched);
    assert_ne!(mapping.taxon_id, old_taxon_id);
    assert!(get_photo_taxon_node(&database, old_taxon_id, false).is_err());
}

#[test]
fn accepted_name_match_wins_and_alias_is_an_exact_fallback() {
    let data = tempfile::tempdir().unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    connection
        .execute_batch(
            r#"
            INSERT INTO taxa (taxon_id, rank) VALUES (1, 5), (2, 5);
            INSERT INTO taxon_names (taxon_id, name_type, name) VALUES
                (1, 1, 'Shared name'),
                (2, 1, 'Other name'),
                (2, 2, 'Shared name');
            "#,
        )
        .unwrap();

    let accepted =
        find_photo_name_candidates(&connection, PhotoNameField::SpeciesSci, "Shared name").unwrap();
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].summary.taxon_id, 1);
    assert_eq!(
        accepted[0].matched_names[0].name_type,
        TaxonomyNameType::SciName
    );

    connection
        .execute(
            "DELETE FROM taxon_names WHERE taxon_id = 1 AND name_type = 1",
            [],
        )
        .unwrap();
    let alias =
        find_photo_name_candidates(&connection, PhotoNameField::SpeciesSci, "Shared name").unwrap();
    assert_eq!(alias.len(), 1);
    assert_eq!(alias[0].summary.taxon_id, 2);
    assert_eq!(
        alias[0].matched_names[0].name_type,
        TaxonomyNameType::Synonym
    );
    assert!(
        find_photo_name_candidates(&connection, PhotoNameField::SpeciesSci, "shared name")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn stable_mapping_provenance_replaces_a_synonym_with_an_accepted_match() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("Felis leo.jpg"), b"photo").unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();
    let connection = database.connect().unwrap();
    connection
        .execute_batch(
            r#"
            INSERT INTO taxa (taxon_id, rank) VALUES (1, 5);
            INSERT INTO taxon_names (taxon_id, name_type, name) VALUES
                (1, 1, 'Panthera leo'),
                (1, 2, 'Felis leo');
            "#,
        )
        .unwrap();
    drop(connection);
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = photos::list_photos(&database).unwrap().remove(0);
    let mut progress = |_: OperationProgress| {};
    process_pending_photo_matches(&database, &mut progress).unwrap();

    let synonym_detail = get_photo_mapping_detail(&database, photo.photo_id).unwrap();
    assert_eq!(synonym_detail.mapping.status, PhotoTaxonStatus::Matched);
    assert_eq!(synonym_detail.matched_names.len(), 1);
    assert_eq!(
        synonym_detail.matched_names[0].name_type,
        TaxonomyNameType::Synonym
    );
    assert_eq!(synonym_detail.matched_names[0].name, "Felis leo");

    database
        .connect()
        .unwrap()
        .execute_batch(
            r#"
            DELETE FROM taxon_names WHERE taxon_id = 1 AND name_type = 2;
            UPDATE taxon_names
            SET name = 'Felis leo'
            WHERE taxon_id = 1 AND name_type = 1;
            "#,
        )
        .unwrap();
    remap_photo(&database, photo.photo_id).unwrap();

    let accepted_detail = get_photo_mapping_detail(&database, photo.photo_id).unwrap();
    assert_eq!(accepted_detail.matched_names.len(), 1);
    assert_eq!(
        accepted_detail.matched_names[0].name_type,
        TaxonomyNameType::SciName
    );
    assert_eq!(accepted_detail.matched_names[0].name, "Felis leo");
}

#[test]
fn stable_mapping_persists_chinese_alias_provenance() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("input.jpg"), b"photo").unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();
    database
        .connect()
        .unwrap()
        .execute_batch(
            r#"
            INSERT INTO taxa (taxon_id, rank) VALUES (1, 5);
            INSERT INTO taxon_names (taxon_id, name_type, name) VALUES
                (1, 1, 'Panthera leo'),
                (1, 3, 'lion'),
                (1, 4, 'old lion');
            "#,
        )
        .unwrap();
    set_naming_hook(
        &database,
        NamingHookKind::PhotoFilename,
        Some(
            r#"
            fn parse_photo_filename(filename) {
                #{ info: #{ species_zh: "old lion" }, suffix: ".jpg" }
            }
            "#,
        ),
    )
    .unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = photos::list_photos(&database).unwrap().remove(0);
    let mut progress = |_: OperationProgress| {};
    process_pending_photo_matches(&database, &mut progress).unwrap();

    let detail = get_photo_mapping_detail(&database, photo.photo_id).unwrap();
    assert_eq!(detail.mapping.status, PhotoTaxonStatus::Matched);
    assert_eq!(detail.matched_names.len(), 1);
    assert_eq!(detail.matched_names[0].name_type, TaxonomyNameType::ZhAlias);
    assert_eq!(detail.matched_names[0].name, "old lion");
}

#[test]
fn candidate_limit_is_applied_before_loading_taxon_summaries() {
    let data = tempfile::tempdir().unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    connection
        .execute_batch(&format!(
            r#"
            PRAGMA foreign_keys = OFF;
            WITH RECURSIVE ids(value) AS (
                SELECT 1
                UNION ALL
                SELECT value + 1 FROM ids WHERE value < {limit}
            )
            INSERT INTO taxa (taxon_id, rank)
                SELECT value, 5 FROM ids;
            INSERT INTO taxa (taxon_id, parent_taxon_id, rank)
                VALUES ({orphan_id}, 999999, 5);
            WITH RECURSIVE ids(value) AS (
                SELECT 1
                UNION ALL
                SELECT value + 1 FROM ids WHERE value < {orphan_id}
            )
            INSERT INTO taxon_names (taxon_id, name_type, name)
                SELECT value, 1, 'Limited name' FROM ids;
            "#,
            limit = PHOTO_TAXON_CANDIDATE_LIMIT,
            orphan_id = PHOTO_TAXON_CANDIDATE_LIMIT + 1,
        ))
        .unwrap();

    let candidates =
        find_photo_name_candidates(&connection, PhotoNameField::SpeciesSci, "Limited name")
            .unwrap();

    assert_eq!(candidates.len(), PHOTO_TAXON_CANDIDATE_LIMIT);
    assert_eq!(
        candidates
            .last()
            .map(|candidate| candidate.summary.taxon_id),
        Some(PHOTO_TAXON_CANDIDATE_LIMIT as i64)
    );
}

#[test]
fn persists_ambiguous_candidates_and_accepts_a_forced_mapping() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("Shared name.jpg"), b"photo").unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();
    let connection = database.connect().unwrap();
    for _ in 0..2 {
        connection
            .execute("INSERT INTO taxa (rank) VALUES (5)", [])
            .unwrap();
        let taxon_id = connection.last_insert_rowid();
        connection
            .execute(
                r#"
                    INSERT INTO taxon_names (taxon_id, name_type, name)
                    VALUES (?, 1, ?)
                    "#,
                params![taxon_id, "Shared name"],
            )
            .unwrap();
    }
    connection
        .execute("INSERT INTO taxa (rank) VALUES (5)", [])
        .unwrap();
    let alias_taxon_id = connection.last_insert_rowid();
    connection
        .execute(
            r#"
            INSERT INTO taxon_names (taxon_id, name_type, name) VALUES
                (?, 1, 'Different name'),
                (?, 2, 'Shared name')
            "#,
            params![alias_taxon_id, alias_taxon_id],
        )
        .unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = photos::list_photos(&database).unwrap().remove(0);
    assert_eq!(
        get_photo_mapping(&database, photo.photo_id).unwrap().status,
        PhotoTaxonStatus::Processing
    );
    let mut progress = |_: OperationProgress| {};
    process_pending_photo_matches(&database, &mut progress).unwrap();
    let mapping = get_photo_mapping(&database, photo.photo_id).unwrap();
    let ambiguous_detail = get_photo_mapping_detail(&database, photo.photo_id).unwrap();
    assert!(ambiguous_detail.matched_names.is_empty());
    let candidates = ambiguous_detail.candidates;
    assert_eq!(mapping.status, PhotoTaxonStatus::Ambiguous);
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].matched_names.len(), 1);
    assert_eq!(candidates[1].matched_names.len(), 1);
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.summary.taxon_id != alias_taxon_id)
    );
    let connection = database.connect().unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM photo_taxon_candidates WHERE photo_id = ?",
                [photo.photo_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        2
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM photo_taxon_candidate_names WHERE photo_id = ?",
                [photo.photo_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        2
    );
    drop(connection);
    let selected_taxon_id = candidates[0].summary.taxon_id;
    let mut taxonomy = database.connect_taxonomy().unwrap();
    let transaction = taxonomy.transaction().unwrap();
    crate::taxonomy::sync::record_event(&transaction, None, [selected_taxon_id], false).unwrap();
    transaction.commit().unwrap();
    crate::taxonomy::sync::synchronize_pending_photo_libraries(&database).unwrap();
    let processing = get_photo_mapping(&database, photo.photo_id).unwrap();
    assert_eq!(processing.status, PhotoTaxonStatus::Processing);
    assert_eq!(processing.taxon_id, None);
    assert!(
        get_photo_mapping_detail(&database, photo.photo_id)
            .unwrap()
            .candidates
            .is_empty()
    );
    process_pending_photo_matches(&database, &mut progress).unwrap();
    let mapping = get_photo_mapping(&database, photo.photo_id).unwrap();
    let candidates = get_photo_mapping_detail(&database, photo.photo_id)
        .unwrap()
        .candidates;
    assert_eq!(mapping.status, PhotoTaxonStatus::Ambiguous);
    assert_eq!(candidates.len(), 2);
    let selected = set_photo_mapping(&database, photo.photo_id, selected_taxon_id).unwrap();
    assert_eq!(selected.status, PhotoTaxonStatus::Matched);
    assert_eq!(selected.taxon_id, Some(selected_taxon_id));
    let selected_detail = get_photo_mapping_detail(&database, photo.photo_id).unwrap();
    assert_eq!(selected_detail.matched_names.len(), 1);
    assert_eq!(selected_detail.matched_names, candidates[0].matched_names);
    let preserved = remap_photo(&database, photo.photo_id).unwrap();
    assert_eq!(preserved.status, PhotoTaxonStatus::Matched);
    assert_eq!(preserved.taxon_id, Some(selected_taxon_id));
    assert_eq!(
        get_photo_mapping_detail(&database, photo.photo_id)
            .unwrap()
            .matched_names,
        candidates[0].matched_names
    );
    assert_eq!(
        database
            .connect()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM photo_taxon_candidates WHERE photo_id = ?",
                [photo.photo_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    let error = set_photo_mapping(&database, photo.photo_id, i64::MAX).unwrap_err();
    assert!(error.to_string().contains("taxon"));
}

#[test]
fn clears_forces_and_automatically_recomputes_one_mapping() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("Canis lupus.jpg"), b"photo").unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();
    let connection = database.connect().unwrap();
    connection
        .execute_batch(
            r#"
                INSERT INTO taxa (taxon_id, rank) VALUES (1, 5), (2, 5);
                INSERT INTO taxon_names (taxon_id, name_type, name) VALUES
                    (1, 1, 'Canis lupus'),
                    (2, 1, 'Forced taxon');
                "#,
        )
        .unwrap();
    drop(connection);
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = photos::list_photos(&database).unwrap().remove(0);
    let mut progress = |_: OperationProgress| {};
    process_pending_photo_matches(&database, &mut progress).unwrap();
    assert_eq!(
        get_photo_mapping(&database, photo.photo_id)
            .unwrap()
            .taxon_id,
        Some(1)
    );

    let forced = set_photo_mapping(&database, photo.photo_id, 2).unwrap();
    assert_eq!(forced.status, PhotoTaxonStatus::Matched);
    assert_eq!(forced.taxon_id, Some(2));
    assert!(
        get_photo_mapping_detail(&database, photo.photo_id)
            .unwrap()
            .matched_names
            .is_empty()
    );
    assert!(get_photo_taxon_node(&database, Some(1), false).is_err());
    assert_eq!(
        get_photo_taxon_node(&database, Some(2), false)
            .unwrap()
            .subtree_photo_count,
        1
    );

    let cleared = clear_photo_mapping(&database, photo.photo_id).unwrap();
    assert_eq!(cleared.status, PhotoTaxonStatus::Unmatched);
    assert_eq!(cleared.taxon_id, None);
    assert!(get_photo_taxon_node(&database, Some(2), false).is_err());

    let remapped = remap_photo(&database, photo.photo_id).unwrap();
    assert_eq!(remapped.status, PhotoTaxonStatus::Matched);
    assert_eq!(remapped.taxon_id, Some(1));
    assert_eq!(
        get_photo_mapping_detail(&database, photo.photo_id)
            .unwrap()
            .matched_names[0]
            .name_type,
        TaxonomyNameType::SciName
    );
    assert!(
        get_photo_mapping_detail(&database, photo.photo_id)
            .unwrap()
            .candidates
            .is_empty()
    );
    assert!(set_photo_mapping(&database, photo.photo_id, i64::MAX).is_err());
    assert!(clear_photo_mapping(&database, i64::MAX).is_err());
    assert!(remap_photo(&database, i64::MAX).is_err());
}

#[test]
fn does_not_synthesize_processing_for_a_missing_photo() {
    let data = tempfile::tempdir().unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();

    assert!(matches!(
        get_photo_mapping(&database, 404).unwrap_err(),
        CoreError::NotFound(_)
    ));
}

#[test]
fn photo_display_summary_requires_a_unique_current_mapping() {
    let data = tempfile::tempdir().unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();
    let connection = database.connect().unwrap();
    connection
        .execute_batch(
            r#"
            INSERT INTO taxa (taxon_id, parent_taxon_id, rank) VALUES
                (1, NULL, 1),
                (2, 1, 2),
                (3, 2, 3),
                (4, 3, 4),
                (5, 4, 5);
            INSERT INTO taxon_names (taxon_id, name_type, name) VALUES
                (1, 1, 'Animalia'),
                (2, 1, 'Carnivora'),
                (3, 1, 'Felidae'),
                (4, 1, 'Panthera'),
                (5, 1, 'Panthera leo'),
                (5, 2, 'Felis leo'),
                (5, 5, 'Lion');
            INSERT INTO photo_directories (
                parent_directory_id, name, relative_path
            ) VALUES (NULL, '', '');
            "#,
        )
        .unwrap();
    let directory_id = connection.last_insert_rowid();
    let species_photo_id = insert_test_photo(&connection, directory_id, "species.jpg");
    let order_photo_id = insert_test_photo(&connection, directory_id, "order.jpg");
    let unmatched_photo_id = insert_test_photo(&connection, directory_id, "unmatched.jpg");
    let ambiguous_photo_id = insert_test_photo(&connection, directory_id, "ambiguous.jpg");
    for (photo_id, taxon_id, status) in [
        (species_photo_id, Some(5), "matched"),
        (order_photo_id, Some(2), "matched"),
        (unmatched_photo_id, None, "unmatched"),
        (ambiguous_photo_id, None, "ambiguous"),
    ] {
        connection
            .execute(
                r#"
                INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
                VALUES (?, ?, ?)
                "#,
                params![photo_id, taxon_id, status],
            )
            .unwrap();
    }

    let species = get_photo_taxon_display_summary(&database, species_photo_id)
        .unwrap()
        .unwrap();
    assert_eq!(species.current_rank, TaxonRank::Species);
    assert_eq!(
        species
            .items
            .iter()
            .map(|item| (item.rank, item.names.sci_name.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            (TaxonRank::Family, Some("Felidae")),
            (TaxonRank::Genus, Some("Panthera")),
            (TaxonRank::Species, Some("Panthera leo")),
        ]
    );
    assert_eq!(species.items[2].names.en_name.as_deref(), Some("Lion"));

    let order = get_photo_taxon_display_summary(&database, order_photo_id)
        .unwrap()
        .unwrap();
    assert_eq!(order.items.len(), 1);
    assert_eq!(order.items[0].rank, TaxonRank::Order);
    assert!(
        get_photo_taxon_display_summary(&database, unmatched_photo_id)
            .unwrap()
            .is_none()
    );
    assert!(
        get_photo_taxon_display_summary(&database, ambiguous_photo_id)
            .unwrap()
            .is_none()
    );

    connection
        .execute(
            "INSERT INTO photo_mapping_queue (photo_id, reason) VALUES (?, 'refresh')",
            [species_photo_id],
        )
        .unwrap();
    assert!(
        get_photo_taxon_display_summary(&database, species_photo_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn rejects_a_photo_without_mapping_state() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("Canis lupus.jpg"), b"photo").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = photos::list_photos(&database).unwrap().remove(0);
    database
        .connect()
        .unwrap()
        .execute(
            "DELETE FROM photo_mapping_queue WHERE photo_id = ?",
            [photo.photo_id],
        )
        .unwrap();

    let mapping_error = get_photo_mapping(&database, photo.photo_id).unwrap_err();
    assert!(matches!(mapping_error, CoreError::Consistency(_)));
    assert!(
        mapping_error
            .to_string()
            .contains("neither a mapping nor a mapping queue entry")
    );

    let candidates_error = get_photo_mapping_detail(&database, photo.photo_id).unwrap_err();
    assert!(matches!(candidates_error, CoreError::Consistency(_)));
}

#[test]
fn queues_a_photo_when_its_selected_taxon_is_deleted() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("Felis catus.jpg"), b"photo").unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();
    let connection = database.connect().unwrap();
    connection
        .execute("INSERT INTO taxa (rank) VALUES (4)", [])
        .unwrap();
    let parent_taxon_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, 5)",
            [parent_taxon_id],
        )
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
    drop(connection);
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = photos::list_photos(&database).unwrap().remove(0);
    let mut progress = |_: OperationProgress| {};
    process_pending_photo_matches(&database, &mut progress).unwrap();
    set_photo_mapping(&database, photo.photo_id, taxon_id).unwrap();
    assert_eq!(
        get_photo_taxon_node(&database, Some(parent_taxon_id), false)
            .unwrap()
            .subtree_photo_count,
        1
    );

    crate::taxonomy::delete_taxon(&database, taxon_id).unwrap();
    crate::taxonomy::synchronize_pending_photo_libraries(&database).unwrap();

    let connection = database.connect().unwrap();
    let stored_mapping_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM photo_taxon_mapping WHERE photo_id = ?",
            [photo.photo_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored_mapping_count, 0);
    drop(connection);
    assert!(get_photo_taxon_node(&database, Some(parent_taxon_id), false).is_err());
    assert_eq!(
        get_photo_taxon_node(&database, Some(parent_taxon_id), true)
            .unwrap()
            .subtree_photo_count,
        0
    );
    assert_eq!(
        get_photo_mapping(&database, photo.photo_id).unwrap().status,
        PhotoTaxonStatus::Processing
    );
    process_pending_photo_matches(&database, &mut progress).unwrap();
    assert_eq!(
        get_photo_mapping(&database, photo.photo_id).unwrap().status,
        PhotoTaxonStatus::Unmatched
    );
}

#[test]
fn taxonomy_update_queues_only_affected_photos() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("Canis lupus.jpg"), b"photo").unwrap();
    fs::write(root.path().join("Felis catus.jpg"), b"photo").unwrap();
    fs::write(root.path().join("domestic cat.jpg"), b"photo").unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();
    let connection = database.connect().unwrap();
    let insert_taxon = |parent_taxon_id: Option<i64>, rank: i64, name: &str| {
        connection
            .execute(
                "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, ?)",
                params![parent_taxon_id, rank],
            )
            .unwrap();
        let taxon_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO taxon_names (taxon_id, name_type, name) VALUES (?, 1, ?)",
                params![taxon_id, name],
            )
            .unwrap();
        taxon_id
    };
    let animalia_taxon_id = insert_taxon(None, 1, "Animalia");
    let carnivora_taxon_id = insert_taxon(Some(animalia_taxon_id), 2, "Carnivora");
    let canidae_taxon_id = insert_taxon(Some(carnivora_taxon_id), 3, "Canidae");
    let canis_genus_taxon_id = insert_taxon(Some(canidae_taxon_id), 4, "Canis");
    let canis_taxon_id = insert_taxon(Some(canis_genus_taxon_id), 5, "Canis lupus");
    let felidae_taxon_id = insert_taxon(Some(carnivora_taxon_id), 3, "Felidae");
    let felis_genus_taxon_id = insert_taxon(Some(felidae_taxon_id), 4, "Felis");
    let felis_taxon_id = insert_taxon(Some(felis_genus_taxon_id), 5, "Felis catus");
    drop(connection);
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let mut progress = |_: OperationProgress| {};
    process_pending_photo_matches(&database, &mut progress).unwrap();
    let photos = photos::list_photos(&database).unwrap();
    let canis_photo = photos
        .iter()
        .find(|photo| photo.filename == "Canis lupus.jpg")
        .unwrap();
    let felis_photo = photos
        .iter()
        .find(|photo| photo.filename == "Felis catus.jpg")
        .unwrap();
    let domestic_cat_photo = photos
        .iter()
        .find(|photo| photo.filename == "domestic cat.jpg")
        .unwrap();
    let connection = database.connect().unwrap();
    connection
        .execute(
            r#"
                INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
                VALUES (?, ?, 'matched')
                ON CONFLICT(photo_id) DO UPDATE
                SET taxon_id = excluded.taxon_id, status = excluded.status
                "#,
            params![canis_photo.photo_id, canis_taxon_id],
        )
        .unwrap();
    connection
        .execute(
            r#"
                INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
                VALUES (?, ?, 'matched')
                ON CONFLICT(photo_id) DO UPDATE
                SET taxon_id = excluded.taxon_id, status = excluded.status
                "#,
            params![felis_photo.photo_id, felis_taxon_id],
        )
        .unwrap();
    connection
        .execute("DELETE FROM photo_mapping_queue", [])
        .unwrap();
    drop(connection);

    apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Animalia".into()),
            order: Some("Carnivora".into()),
            family: Some("Felidae".into()),
            genus: Some("Felis".into()),
            species: Some("Felis catus".into()),
            en_name: Some("domestic cat".into()),
            ..Default::default()
        }],
    )
    .unwrap();
    crate::taxonomy::synchronize_pending_photo_libraries(&database).unwrap();

    assert_eq!(
        get_photo_mapping(&database, felis_photo.photo_id)
            .unwrap()
            .status,
        PhotoTaxonStatus::Processing
    );
    assert_eq!(
        get_photo_mapping(&database, canis_photo.photo_id)
            .unwrap()
            .status,
        PhotoTaxonStatus::Matched
    );
    assert_eq!(
        get_photo_mapping(&database, domestic_cat_photo.photo_id)
            .unwrap()
            .status,
        PhotoTaxonStatus::Unmatched
    );
    assert_eq!(get_metadata(&database).unwrap().processing_photo_count, 1);
}

#[test]
fn taxon_browse_cursor_spans_children_and_photos() {
    let data = tempfile::tempdir().unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();
    let connection = database.connect().unwrap();
    connection
        .execute("INSERT INTO taxa (rank) VALUES (1)", [])
        .unwrap();
    let parent_taxon_id = connection.last_insert_rowid();
    connection
        .execute(
            r#"
                INSERT INTO taxon_names (taxon_id, name_type, name)
                VALUES (?, 1, 'Parent')
                "#,
            [parent_taxon_id],
        )
        .unwrap();
    let mut child_taxon_ids = Vec::new();
    for name in ["First child", "Second child"] {
        connection
            .execute(
                "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, 2)",
                [parent_taxon_id],
            )
            .unwrap();
        let child_taxon_id = connection.last_insert_rowid();
        connection
            .execute(
                r#"
                    INSERT INTO taxon_names (taxon_id, name_type, name)
                    VALUES (?, 1, ?)
                    "#,
                params![child_taxon_id, name],
            )
            .unwrap();
        child_taxon_ids.push(child_taxon_id);
    }
    connection
        .execute(
            r#"
                INSERT INTO photo_directories (
                    parent_directory_id, name, relative_path
                ) VALUES (NULL, '', '')
                "#,
            [],
        )
        .unwrap();
    let directory_id = connection.last_insert_rowid();
    let first_photo_id = insert_test_photo(&connection, directory_id, "first.jpg");
    let second_photo_id = insert_test_photo(&connection, directory_id, "second.jpg");
    for photo_id in [first_photo_id, second_photo_id] {
        connection
            .execute(
                r#"
                    INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
                    VALUES (?, ?, 'matched')
                    "#,
                params![photo_id, parent_taxon_id],
            )
            .unwrap();
    }
    connection
        .execute(
            r#"
                INSERT INTO photo_taxon_usage (
                    taxon_id, direct_photo_count, subtree_photo_count
                ) VALUES (?, 2, 2)
                "#,
            [parent_taxon_id],
        )
        .unwrap();
    drop(connection);

    let first = browse_photo_taxon(&database, Some(parent_taxon_id), true, None, 2).unwrap();
    assert_eq!(first.items.len(), 2);
    assert!(
        first
            .items
            .iter()
            .all(|item| matches!(item, PhotoTaxonItem::Taxon { .. }))
    );
    assert!(first.next_cursor.is_some());
    database
        .connect()
        .unwrap()
        .execute(
            "INSERT INTO photo_mapping_queue (photo_id, reason) VALUES (?, 'refresh')",
            [first_photo_id],
        )
        .unwrap();
    let current_node = get_photo_taxon_node(&database, Some(parent_taxon_id), false).unwrap();
    assert_eq!(current_node.taxon.as_ref().unwrap().direct_photo_count, 1);
    assert_eq!(current_node.subtree_photo_count, 1);
    assert_eq!(
        get_photo_taxon_node(&database, None, false)
            .unwrap()
            .subtree_photo_count,
        1
    );
    let error = browse_photo_taxon(
        &database,
        Some(child_taxon_ids[0]),
        true,
        first.next_cursor.as_deref(),
        2,
    )
    .unwrap_err();
    assert!(error.to_string().contains("invalid photo cursor"));

    let second = browse_photo_taxon(
        &database,
        Some(parent_taxon_id),
        true,
        first.next_cursor.as_deref(),
        2,
    )
    .unwrap();
    assert_eq!(
        second.items,
        vec![PhotoTaxonItem::Photo {
            photo: photos::get_photo(&database, second_photo_id)
                .unwrap()
                .unwrap()
        }]
    );
    assert_eq!(second.next_cursor, None);
}

#[test]
fn taxon_browse_lists_direct_photos_and_subtree_photos_are_separate() {
    let data = tempfile::tempdir().unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();
    let connection = database.connect().unwrap();
    connection
        .execute(
            "INSERT INTO taxa (parent_taxon_id, rank) VALUES (NULL, 1)",
            [],
        )
        .unwrap();
    let parent_taxon_id = connection.last_insert_rowid();
    connection
        .execute(
            r#"
                INSERT INTO taxon_names (taxon_id, name_type, name)
                VALUES (?, 1, 'Parent taxon')
                "#,
            [parent_taxon_id],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, 2)",
            [parent_taxon_id],
        )
        .unwrap();
    let child_taxon_id = connection.last_insert_rowid();
    connection
        .execute(
            r#"
                INSERT INTO taxon_names (taxon_id, name_type, name)
                VALUES (?, 1, 'Child taxon')
                "#,
            [child_taxon_id],
        )
        .unwrap();
    connection
        .execute(
            r#"
                INSERT INTO photo_directories (
                    parent_directory_id, name, relative_path
                ) VALUES (NULL, '', '')
                "#,
            [],
        )
        .unwrap();
    let directory_id = connection.last_insert_rowid();
    let parent_photo_id = insert_test_photo(&connection, directory_id, "parent.jpg");
    let child_photo_id = insert_test_photo(&connection, directory_id, "child.jpg");
    for (photo_id, taxon_id) in [
        (parent_photo_id, parent_taxon_id),
        (child_photo_id, child_taxon_id),
    ] {
        connection
            .execute(
                r#"
                    INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
                    VALUES (?, ?, 'matched')
                    "#,
                params![photo_id, taxon_id],
            )
            .unwrap();
    }
    for (taxon_id, direct_photo_count, subtree_photo_count) in
        [(parent_taxon_id, 1, 2), (child_taxon_id, 1, 1)]
    {
        connection
            .execute(
                r#"
                    INSERT INTO photo_taxon_usage (
                        taxon_id, direct_photo_count, subtree_photo_count
                    ) VALUES (?, ?, ?)
                    "#,
                params![taxon_id, direct_photo_count, subtree_photo_count],
            )
            .unwrap();
    }
    drop(connection);

    let root_page = browse_photo_taxon(&database, None, false, None, 20).unwrap();
    assert!(
        root_page
            .items
            .iter()
            .all(|item| matches!(item, PhotoTaxonItem::Taxon { .. }))
    );

    let browse_page =
        browse_photo_taxon(&database, Some(parent_taxon_id), false, None, 20).unwrap();
    let taxon_ids = browse_page
        .items
        .iter()
        .filter_map(|item| match item {
            PhotoTaxonItem::Taxon { taxon } => Some(taxon.taxon_id),
            PhotoTaxonItem::Photo { .. } => None,
        })
        .collect::<Vec<_>>();
    let photo_ids = browse_page
        .items
        .iter()
        .filter_map(|item| match item {
            PhotoTaxonItem::Taxon { .. } => None,
            PhotoTaxonItem::Photo { photo } => Some(photo.photo_id),
        })
        .collect::<Vec<_>>();
    assert_eq!(taxon_ids, vec![child_taxon_id]);
    assert_eq!(photo_ids, vec![parent_photo_id]);

    let subtree_page = list_taxon_photos(&database, parent_taxon_id, None, 20).unwrap();
    assert_eq!(
        subtree_page
            .items
            .into_iter()
            .map(|photo| photo.photo_id)
            .collect::<Vec<_>>(),
        vec![parent_photo_id, child_photo_id],
    );
}

#[test]
fn mapping_status_pages_are_logical_and_cursor_scoped() {
    let data = tempfile::tempdir().unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();
    let connection = database.connect().unwrap();
    connection
        .execute(
            r#"
                INSERT INTO photo_directories (
                    parent_directory_id, name, relative_path
                ) VALUES (NULL, '', '')
                "#,
            [],
        )
        .unwrap();
    let directory_id = connection.last_insert_rowid();
    let processing_photo_id = insert_test_photo(&connection, directory_id, "processing.jpg");
    let first_unmatched_id = insert_test_photo(&connection, directory_id, "unmatched-1.jpg");
    let second_unmatched_id =
        insert_test_photo(&connection, directory_id, "Canidae-unmatched-2.jpg");
    let matched_photo_id = insert_test_photo(&connection, directory_id, "plain-match.jpg");
    let ambiguous_photo_id = insert_test_photo(&connection, directory_id, "plain-ambiguous.jpg");
    for photo_id in [processing_photo_id, first_unmatched_id, second_unmatched_id] {
        connection
            .execute(
                r#"
                    INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
                    VALUES (?, NULL, 'unmatched')
                    "#,
                [photo_id],
            )
            .unwrap();
    }
    connection
        .execute_batch(&format!(
            r#"
                INSERT INTO taxa (taxon_id, rank) VALUES (1, 3), (2, 3);
                INSERT INTO taxon_names (name_id, taxon_id, name_type, name)
                VALUES
                    (1, 1, 1, 'Canidae'),
                    (2, 2, 1, 'Canidae');
                INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
                VALUES
                    ({matched_photo_id}, 1, 'matched'),
                    ({ambiguous_photo_id}, NULL, 'ambiguous');
                INSERT INTO photo_taxon_candidates (photo_id, taxon_id)
                VALUES
                    ({ambiguous_photo_id}, 1),
                    ({ambiguous_photo_id}, 2);
                INSERT INTO photo_taxon_candidate_names (
                    photo_id, taxon_id, name_id, name_type, name
                ) VALUES
                    ({ambiguous_photo_id}, 1, 1, 1, 'Canidae'),
                    ({ambiguous_photo_id}, 2, 2, 1, 'Canidae');
                INSERT INTO photo_taxon_usage (
                    taxon_id, direct_photo_count, subtree_photo_count
                ) VALUES (1, 1, 1);
                "#
        ))
        .unwrap();
    connection
        .execute(
            r#"
                INSERT INTO photo_mapping_queue (photo_id, reason)
                VALUES (?, 'refresh')
                "#,
            [processing_photo_id],
        )
        .unwrap();
    drop(connection);

    let first =
        list_photos_by_mapping_status(&database, PhotoMappingListStatus::Unmatched, None, 1)
            .unwrap();
    assert_eq!(first.items[0].photo.photo_id, first_unmatched_id);
    assert_eq!(first.items[0].mapping.status, PhotoTaxonStatus::Unmatched);
    assert!(first.next_cursor.is_some());
    let error = list_photos_by_mapping_status(
        &database,
        PhotoMappingListStatus::Processing,
        first.next_cursor.as_deref(),
        1,
    )
    .unwrap_err();
    assert!(error.to_string().contains("invalid photo cursor"));
    let second = list_photos_by_mapping_status(
        &database,
        PhotoMappingListStatus::Unmatched,
        first.next_cursor.as_deref(),
        1,
    )
    .unwrap();
    assert_eq!(second.items[0].photo.photo_id, second_unmatched_id);
    assert_eq!(second.next_cursor, None);

    let processing =
        list_photos_by_mapping_status(&database, PhotoMappingListStatus::Processing, None, 10)
            .unwrap();
    assert_eq!(processing.items.len(), 1);
    assert_eq!(processing.items[0].photo.photo_id, processing_photo_id);
    assert_eq!(
        processing.items[0].mapping.status,
        PhotoTaxonStatus::Processing
    );

    let matched_search = search_photos_by_mapping_status(
        &database,
        PhotoMappingListStatus::Matched,
        "Canidae",
        None,
        10,
    )
    .unwrap();
    assert_eq!(matched_search.items.len(), 1);
    assert_eq!(matched_search.items[0].photo.photo_id, matched_photo_id);
    let unmatched_search = search_photos_by_mapping_status(
        &database,
        PhotoMappingListStatus::Unmatched,
        "Canidae",
        None,
        10,
    )
    .unwrap();
    assert_eq!(unmatched_search.items.len(), 1);
    assert_eq!(
        unmatched_search.items[0].photo.photo_id,
        second_unmatched_id
    );
    let first_search = search_photos_by_mapping_status(
        &database,
        PhotoMappingListStatus::Unmatched,
        "unmatched",
        None,
        1,
    )
    .unwrap();
    assert_eq!(first_search.items[0].photo.photo_id, first_unmatched_id);
    assert!(first_search.next_cursor.is_some());
    let second_search = search_photos_by_mapping_status(
        &database,
        PhotoMappingListStatus::Unmatched,
        "unmatched",
        first_search.next_cursor.as_deref(),
        1,
    )
    .unwrap();
    assert_eq!(second_search.items[0].photo.photo_id, second_unmatched_id);
    assert!(second_search.next_cursor.is_none());
    let processing_search = search_photos_by_mapping_status(
        &database,
        PhotoMappingListStatus::Processing,
        "processing",
        None,
        10,
    )
    .unwrap();
    assert_eq!(processing_search.items.len(), 1);
    assert_eq!(
        processing_search.items[0].photo.photo_id,
        processing_photo_id
    );
    assert!(
        search_photos_by_mapping_status(
            &database,
            PhotoMappingListStatus::Ambiguous,
            "Canidae",
            None,
            10,
        )
        .unwrap()
        .items
        .is_empty()
    );
    let ambiguous_search = search_photos_by_mapping_status(
        &database,
        PhotoMappingListStatus::Ambiguous,
        "ambiguous",
        None,
        10,
    )
    .unwrap();
    assert_eq!(ambiguous_search.items.len(), 1);
    assert_eq!(ambiguous_search.items[0].photo.photo_id, ambiguous_photo_id);
    assert!(
        search_photos_by_mapping_status(
            &database,
            PhotoMappingListStatus::Matched,
            "Canidae",
            first.next_cursor.as_deref(),
            10,
        )
        .is_err()
    );

    let metadata = get_metadata(&database).unwrap();
    assert_eq!(metadata.mapped_photo_count, 1);
    assert_eq!(metadata.unmatched_photo_count, 2);
    assert_eq!(metadata.ambiguous_photo_count, 1);
    assert_eq!(metadata.processing_photo_count, 1);
}

#[test]
fn batches_usage_deltas_for_shared_ancestors() {
    let data = tempfile::tempdir().unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();
    let mut connection = database.connect().unwrap();
    connection
        .execute("INSERT INTO taxa (rank) VALUES (1)", [])
        .unwrap();
    let root_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, 2)",
            [root_id],
        )
        .unwrap();
    let first_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, 2)",
            [root_id],
        )
        .unwrap();
    let second_id = connection.last_insert_rowid();
    let transaction = connection.transaction().unwrap();
    let deltas = BTreeMap::from([(first_id, 1), (second_id, 1)]);

    apply_usage_deltas(&transaction, &deltas).unwrap();

    assert_eq!(
        transaction
            .query_row(
                "SELECT subtree_photo_count FROM photo_taxon_usage WHERE taxon_id = ?",
                [root_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        2
    );
    assert_eq!(
        transaction
            .query_row(
                "SELECT SUM(direct_photo_count) FROM photo_taxon_usage WHERE taxon_id IN (?, ?)",
                params![first_id, second_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        2
    );
}

#[test]
fn large_id_sets_use_a_temporary_table() {
    let data = tempfile::tempdir().unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();
    let mut connection = database.connect().unwrap();
    let transaction = connection.transaction().unwrap();
    let ids = (1..=501).collect::<Vec<_>>();
    let selection = id_selection(&transaction, &ids, "photo_id", "temp_mapping_photo_ids").unwrap();
    assert!(selection.values.is_empty());
    assert_eq!(
        transaction
            .query_row("SELECT COUNT(*) FROM temp_mapping_photo_ids", [], |row| row
                .get::<_, i64>(0),)
            .unwrap(),
        501
    );
    assert!(selection.predicate.contains("temp_mapping_photo_ids"));
}
