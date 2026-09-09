use tempfile::TempDir;

use super::*;

fn database() -> (TempDir, Database) {
    let directory = TempDir::new().unwrap();
    let database = Database::open(directory.path().join("test.db")).unwrap();
    (directory, database)
}

#[test]
fn name_type_codes_follow_public_name_order() {
    for (index, name_type) in TaxonomyNameType::ALL.into_iter().enumerate() {
        let code = index as i64 + 1;
        assert_eq!(name_type.code(), code);
        assert_eq!(TaxonomyNameType::from_code(code).unwrap(), name_type);
    }
    assert!(TaxonomyNameType::from_code(0).is_err());
    assert!(TaxonomyNameType::from_code(7).is_err());
}

#[test]
fn formatted_update_uses_the_configured_synonym_hook() {
    let (_directory, database) = database();
    crate::naming::set_naming_hook(
        &database,
        crate::naming::NamingHookKind::SynonymAuthority,
        Some(
            r#"
                fn split_synonym_authority(value) {
                    if value == "  Raw_synonym  " {
                        #{ name: value, authority_year: "raw" }
                    } else if value == "   " {
                        #{ name: "Whitespace input", authority_year: "raw" }
                    } else {
                        #{ name: "Wrong input", authority_year: "normalized" }
                    }
                }
                "#,
        ),
    )
    .unwrap();
    let rows = parse_taxonomy_input_csv(
        &database,
        "kingdom,synonyms\nAnimalia,\"  Raw_synonym  ;   \"\n",
    )
    .unwrap();
    assert_eq!(rows[0].synonyms, vec!["  Raw_synonym  ", "   "]);
    apply_rows(&database, &rows).unwrap();
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    let mut statement = connection
        .prepare("SELECT name, authority_year FROM taxon_names WHERE name_type = 2 ORDER BY name")
        .unwrap();
    let names = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        names,
        vec![
            ("Raw synonym".into(), "raw".into()),
            ("Whitespace input".into(), "raw".into()),
        ]
    );
}

#[test]
fn empty_synonym_cell_is_an_empty_list() {
    let (_directory, database) = database();
    let rows = parse_taxonomy_input_csv(&database, "kingdom,synonyms\nAnimalia,\n").unwrap();

    assert!(rows[0].synonyms.is_empty());
}

#[test]
fn csv_accepts_subset_and_reordered_headers() {
    let (_directory, database) = database();
    let rows = parse_taxonomy_input_csv(
        &database,
        "synonyms,species,zh_alias\n\"Canis lycaon Linnaeus, 1758\",Canis lupus,wolf;dog\n",
    )
    .unwrap();
    assert_eq!(rows[0].species.as_deref(), Some("Canis lupus"));
    assert_eq!(rows[0].synonyms.len(), 1);
    assert_eq!(rows[0].zh_alias, vec!["wolf", "dog"]);
}

#[test]
fn csv_rejects_rows_with_a_different_column_count() {
    let (_directory, database) = database();
    let error = parse_taxonomy_input_csv(&database, "kingdom,order\nAnimalia\n").unwrap_err();
    assert!(error.to_string().contains("fields"));
}

#[test]
fn configured_csv_delimiter_controls_formatted_io() {
    let (_directory, database) = database();
    crate::general::update_general_settings(
        &database,
        &crate::general::GeneralSettings {
            csv_delimiter: "\t".into(),
            ..crate::general::GeneralSettings::default()
        },
    )
    .unwrap();

    let template = taxonomy_formatted_update_template(&database).unwrap();
    assert_eq!(
        template.lines().next().unwrap(),
        TAXONOMY_INPUT_COLUMNS.join("\t")
    );
    let rows = parse_taxonomy_input_csv(
        &database,
        "kingdom\tsynonyms\nAnimalia\tMetazoa;Metazoa sensu lato\n",
    )
    .unwrap();
    assert_eq!(rows[0].synonyms.len(), 2);
    let preview = preview_rows(&database, &rows).unwrap();
    assert_eq!(preview.delimiter, "\t");
    let applied = apply_rows(&database, &rows).unwrap();
    assert_eq!(applied.delimiter, "\t");
    assert!(
        taxonomy_log_csv(&database, &applied.rows)
            .unwrap()
            .starts_with("row_number\toperation_types\t")
    );
}

#[test]
fn preview_rolls_back_and_apply_is_revertible() {
    let (_directory, database) = database();
    let input = TaxonInputRow {
        kingdom: Some("Animalia".into()),
        ..TaxonInputRow::default()
    };
    assert_eq!(
        preview_rows(&database, std::slice::from_ref(&input))
            .unwrap()
            .rows[0]
            .operation_types,
        vec![TaxonRowStatus::NewTaxon, TaxonRowStatus::NewName]
    );
    assert!(
        database
            .connect_taxonomy_metadata_context()
            .unwrap()
            .query_row("SELECT NOT EXISTS(SELECT 1 FROM taxa)", [], |row| row
                .get::<_, bool>(0),)
            .unwrap()
    );
    let result = apply_rows(&database, &[input]).unwrap();
    rollback_operation(&database, result.operation_id).unwrap();
    assert!(
        operations::get_operation(
            &database.connect_taxonomy_metadata_context().unwrap(),
            result.operation_id,
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn formatted_operation_input_preserves_submitted_row_order() {
    let (_directory, database) = database();
    let rows = vec![
        TaxonInputRow {
            kingdom: Some("Animalia".into()),
            source: Some("first".into()),
            ..TaxonInputRow::default()
        },
        TaxonInputRow {
            kingdom: Some("Plantae".into()),
            source: Some("second".into()),
            ..TaxonInputRow::default()
        },
    ];
    let result = apply_rows(&database, &rows).unwrap();

    assert_eq!(
        get_operation_input(&database, result.operation_id).unwrap(),
        Some(OperationInput::FormattedUpdate { rows })
    );
}

#[test]
fn prepared_preview_applies_the_cached_changeset() {
    let (_directory, database) = database();
    let input = TaxonInputRow {
        kingdom: Some("Animalia".into()),
        synonyms: vec!["Metazoa".into()],
        ..TaxonInputRow::default()
    };
    let prepared = prepare_rows(&database, std::slice::from_ref(&input)).unwrap();
    let preview = prepared.preview_result().clone();
    assert_eq!(
        preview.rows[0].operation_types,
        vec![TaxonRowStatus::NewTaxon, TaxonRowStatus::NewName]
    );
    assert!(
        database
            .connect_taxonomy_metadata_context()
            .unwrap()
            .query_row("SELECT NOT EXISTS(SELECT 1 FROM taxa)", [], |row| row
                .get::<_, bool>(0))
            .unwrap()
    );

    let applied = apply_prepared_rows(&database, prepared).unwrap();
    assert_eq!(applied.rows, preview.rows);
    assert_eq!(applied.succeeded_rows, 1);
    assert_eq!(
        database
            .connect_taxonomy_metadata_context()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM taxon_names", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn prepared_preview_rejects_a_changed_taxonomy_revision() {
    let (_directory, database) = database();
    let prepared = prepare_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Animalia".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();
    apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Plantae".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    let error = apply_prepared_rows(&database, prepared).unwrap_err();
    assert!(error.to_string().contains("preview is stale"));
    assert_eq!(
        database
            .connect_taxonomy_metadata_context()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM taxon_names WHERE name = 'Animalia'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn one_row_reports_every_change_type() {
    let (_directory, database) = database();
    apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Animalia".into()),
            authority_year: Some("old".into()),
            geological_range: Some("old".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    connection
        .execute(
            "UPDATE taxon_names SET source = NULL WHERE name_type = 1",
            [],
        )
        .unwrap();
    drop(connection);

    let preview = preview_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Animalia".into()),
            authority_year: Some("new".into()),
            synonyms: vec!["Metazoa Linnaeus, 1758".into()],
            geological_range: Some("new".into()),
            source: Some("catalog".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    assert_eq!(
        preview.rows[0].operation_types,
        vec![
            TaxonRowStatus::Supplement,
            TaxonRowStatus::NewName,
            TaxonRowStatus::Overwrite
        ]
    );
}

#[test]
fn source_only_fills_an_empty_existing_value() {
    let (_directory, database) = database();
    apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Animalia".into()),
            source: Some("first".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();
    let result = apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Animalia".into()),
            source: Some("second".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();
    assert_eq!(
        result.rows[0].operation_types,
        vec![TaxonRowStatus::NoChange]
    );
    let source: String = database
        .connect_taxonomy_metadata_context()
        .unwrap()
        .query_row(
            "SELECT source FROM taxon_names WHERE name_type = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(source, "first");
}

#[test]
fn row_source_applies_only_to_target_taxon_names() {
    let (_directory, database) = database();
    apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Animalia".into()),
            order: Some("Carnivora".into()),
            family: Some("Canidae".into()),
            genus: Some("Canis".into()),
            species: Some("Canis lupus".into()),
            source: Some("catalog".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    let mut statement = connection
        .prepare(
            r#"
            SELECT taxon_names.name, taxon_names.source
            FROM taxa JOIN taxon_names USING (taxon_id)
            WHERE taxon_names.name_type = 1
            ORDER BY taxa.rank
            "#,
        )
        .unwrap();
    let sources = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        sources,
        vec![
            ("Animalia".into(), None),
            ("Carnivora".into(), None),
            ("Canidae".into(), None),
            ("Canis".into(), None),
            ("Canis lupus".into(), Some("catalog".into())),
        ]
    );
}

#[test]
fn matched_synonym_receives_its_authority_and_other_names_become_synonyms() {
    let (_directory, database) = database();
    apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Accepted name".into()),
            synonyms: vec!["Matched synonym".into()],
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    let result = apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Proposed name".into()),
            authority_year: Some("Proposed authority".into()),
            synonyms: vec![
                "Matched synonym Matched authority".into(),
                "Other synonym Other authority".into(),
            ],
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    assert_eq!(
        result.rows[0].operation_types,
        vec![TaxonRowStatus::Supplement, TaxonRowStatus::NewName]
    );
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    let names = connection
        .prepare(
            r#"
                SELECT name_type, name, authority_year
                FROM taxon_names
                ORDER BY name_id
                "#,
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        names,
        vec![
            (
                TaxonomyNameType::SciName.code(),
                "Accepted name".into(),
                None
            ),
            (
                TaxonomyNameType::Synonym.code(),
                "Matched synonym".into(),
                Some("Matched authority".into())
            ),
            (
                TaxonomyNameType::Synonym.code(),
                "Proposed name".into(),
                Some("Proposed authority".into())
            ),
            (
                TaxonomyNameType::Synonym.code(),
                "Other synonym".into(),
                Some("Other authority".into())
            ),
        ]
    );
}

#[test]
fn input_priority_precedes_database_name_type_priority() {
    let (_directory, database) = database();
    apply_rows(
        &database,
        &[
            TaxonInputRow {
                kingdom: Some("First taxon".into()),
                synonyms: vec!["First input".into()],
                ..TaxonInputRow::default()
            },
            TaxonInputRow {
                kingdom: Some("Second input".into()),
                ..TaxonInputRow::default()
            },
        ],
    )
    .unwrap();

    let result = apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("No match".into()),
            synonyms: vec!["First input".into(), "Second input".into()],
            geological_range: Some("selected".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();
    assert_eq!(
        result.rows[0]
            .target
            .as_ref()
            .and_then(|target| target.names.sci_name.as_deref()),
        Some("First taxon")
    );
}

#[test]
fn target_sci_name_wins_over_synonym() {
    let (_directory, database) = database();
    apply_rows(
        &database,
        &[
            TaxonInputRow {
                kingdom: Some("Shared".into()),
                ..TaxonInputRow::default()
            },
            TaxonInputRow {
                kingdom: Some("Other".into()),
                ..TaxonInputRow::default()
            },
        ],
    )
    .unwrap();
    apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Other".into()),
            synonyms: vec!["Shared".into()],
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    let preview = preview_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Shared".into()),
            geological_range: Some("selected".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();
    assert_eq!(
        preview.rows[0]
            .target
            .as_ref()
            .and_then(|target| target.names.sci_name.as_deref()),
        Some("Shared")
    );
    assert!(preview.rows[0].candidates.is_empty());
    assert_eq!(
        preview.rows[0].operation_types,
        vec![TaxonRowStatus::Supplement]
    );
}

#[test]
fn target_matching_is_case_sensitive() {
    let (_directory, database) = database();
    apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Shared".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    let preview = preview_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("shared".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    assert!(
        preview.rows[0]
            .operation_types
            .contains(&TaxonRowStatus::NewTaxon)
    );
    assert_eq!(
        preview.rows[0]
            .target
            .as_ref()
            .and_then(|target| target.names.sci_name.as_deref()),
        Some("shared")
    );
}

#[test]
fn sparse_lineage_fields_allow_unique_matches_and_direct_parent_creation() {
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
                ..TaxonInputRow::default()
            },
        ],
    )
    .unwrap();

    let sparse_match = preview_rows(
        &database,
        &[TaxonInputRow {
            family: Some("Canidae".into()),
            species: Some("Canis lupus".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();
    assert_eq!(
        sparse_match.rows[0]
            .target
            .as_ref()
            .and_then(|target| target.names.sci_name.as_deref()),
        Some("Canis lupus")
    );
    assert_eq!(
        sparse_match.rows[0].operation_types,
        vec![TaxonRowStatus::NoChange]
    );

    let direct_parent_only = apply_rows(
        &database,
        &[TaxonInputRow {
            order: Some("Carnivora".into()),
            family: Some("Felidae".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();
    assert!(
        direct_parent_only.rows[0]
            .operation_types
            .contains(&TaxonRowStatus::NewTaxon)
    );
    assert_eq!(
        direct_parent_only.rows[0]
            .parent
            .as_ref()
            .and_then(|parent| parent.names.sci_name.as_deref()),
        Some("Carnivora")
    );
}

#[test]
fn one_species_row_derives_genus_and_creates_the_strict_lineage() {
    let (_directory, database) = database();
    let result = apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Animalia".into()),
            order: Some("Carnivora".into()),
            family: Some("Canidae".into()),
            species: Some("Canis lupus".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    assert!(
        result.rows[0]
            .operation_types
            .contains(&TaxonRowStatus::NewTaxon)
    );
    assert_eq!(
        result.rows[0]
            .parent
            .as_ref()
            .and_then(|parent| parent.names.sci_name.as_deref()),
        Some("Canis")
    );
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM taxa", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        connection
            .query_row(
                r#"
                SELECT parent_name.name
                FROM taxon_names AS species_name
                JOIN taxa AS species USING (taxon_id)
                JOIN taxon_names AS parent_name
                  ON parent_name.taxon_id = species.parent_taxon_id
                 AND parent_name.name_type = 1
                WHERE species_name.name = 'Canis lupus'
                "#,
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "Canis"
    );
}

#[test]
fn a_unique_lowest_rank_match_ignores_supplied_ancestors() {
    let (_directory, database) = database();
    apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Animalia".into()),
            order: Some("Carnivora".into()),
            family: Some("Canidae".into()),
            genus: Some("Canis".into()),
            species: Some("Canis lupus".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    let result = apply_rows(
        &database,
        &[TaxonInputRow {
            genus: Some("Felis".into()),
            species: Some("Canis lupus".into()),
            geological_range: Some("updated".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    assert_eq!(
        result.rows[0]
            .target
            .as_ref()
            .and_then(|target| target.names.sci_name.as_deref()),
        Some("Canis lupus")
    );
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM taxon_names WHERE name = 'Felis'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn ancestor_matching_prefers_sci_name_and_falls_back_to_synonym() {
    let (_directory, database) = database();
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    connection
        .execute_batch(
            r#"
            INSERT INTO taxa (taxon_id, parent_taxon_id, rank) VALUES
                (1, NULL, 1),
                (2, 1, 2),
                (3, 2, 3),
                (4, 3, 4),
                (5, 4, 5),
                (6, NULL, 1),
                (7, 6, 2),
                (8, 7, 3),
                (9, 8, 4),
                (10, 9, 5),
                (11, 4, 5);
            INSERT INTO taxon_names (taxon_id, name_type, name) VALUES
                (1, 1, 'Animalia'),
                (2, 1, 'Carnivora'),
                (3, 1, 'Canidae'),
                (4, 1, 'Canis alpha'),
                (4, 2, 'Canis'),
                (5, 1, 'Shared species'),
                (6, 1, 'Other kingdom'),
                (7, 1, 'Other order'),
                (8, 1, 'Other family'),
                (9, 1, 'Other genus'),
                (10, 1, 'Shared species'),
                (11, 1, 'Different species'),
                (11, 2, 'Shared species');
            "#,
        )
        .unwrap();
    drop(connection);

    let result = preview_rows(
        &database,
        &[TaxonInputRow {
            family: Some("Canidae".into()),
            genus: Some("Canis".into()),
            species: Some("Shared species".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    assert_eq!(result.rows[0].candidates.len(), 0);
    assert_eq!(
        result.rows[0].target.as_ref().map(|target| target.taxon_id),
        Some(5)
    );

    database
        .connect_taxonomy_metadata_context()
        .unwrap()
        .execute(
            "UPDATE taxon_names SET name = 'Canis' WHERE taxon_id = 9 AND name_type = 1",
            [],
        )
        .unwrap();
    let accepted_ancestor = preview_rows(
        &database,
        &[TaxonInputRow {
            family: Some("Canidae".into()),
            genus: Some("Canis".into()),
            species: Some("Shared species".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();
    assert_eq!(
        accepted_ancestor.rows[0]
            .target
            .as_ref()
            .map(|target| target.taxon_id),
        Some(10)
    );
}

#[test]
fn a_missing_strict_parent_is_reported_before_creating_a_lineage() {
    let (_directory, database) = database();
    let result = preview_rows(
        &database,
        &[TaxonInputRow {
            species: Some("Canis lupus".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    assert_eq!(
        result.rows[0].operation_types,
        vec![TaxonRowStatus::NotMatched]
    );
    assert!(
        result.rows[0]
            .message
            .contains("new genus taxon requires a family")
    );
}

#[test]
fn a_new_taxon_reuses_a_unique_parent_synonym_without_checking_higher_ranks() {
    let (_directory, database) = database();
    apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Animalia".into()),
            order: Some("Carnivora".into()),
            family: Some("Canidae".into()),
            genus: Some("Canis".into()),
            synonyms: vec!["Canini".into()],
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    let result = apply_rows(
        &database,
        &[TaxonInputRow {
            family: Some("Wrong family".into()),
            genus: Some("Canini".into()),
            species: Some("Canis familiaris".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    assert_eq!(
        result.rows[0]
            .parent
            .as_ref()
            .and_then(|parent| parent.names.sci_name.as_deref()),
        Some("Canis")
    );
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM taxon_names WHERE name = 'Wrong family'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn a_new_taxon_prefers_an_accepted_parent_over_a_parent_synonym() {
    let (_directory, database) = database();
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    connection
        .execute_batch(
            r#"
            INSERT INTO taxa (taxon_id, parent_taxon_id, rank) VALUES
                (1, NULL, 1),
                (2, 1, 2),
                (3, 2, 3),
                (4, 3, 4),
                (5, 3, 4);
            INSERT INTO taxon_names (taxon_id, name_type, name) VALUES
                (1, 1, 'Animalia'),
                (2, 1, 'Carnivora'),
                (3, 1, 'Canidae'),
                (4, 1, 'Canis'),
                (4, 2, 'Canini'),
                (5, 1, 'Canini');
            "#,
        )
        .unwrap();

    let result = apply_rows(
        &database,
        &[TaxonInputRow {
            family: Some("Canidae".into()),
            genus: Some("Canini".into()),
            species: Some("Canini example".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    assert_eq!(
        result.rows[0].parent.as_ref().map(|parent| parent.taxon_id),
        Some(5)
    );
}

#[test]
fn ancestor_disambiguation_with_no_remaining_target_creates_the_target() {
    let (_directory, database) = database();
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    connection
        .execute_batch(
            r#"
            INSERT INTO taxa (taxon_id, parent_taxon_id, rank) VALUES
                (1, NULL, 1),
                (2, 1, 2),
                (3, 2, 3),
                (4, 3, 4),
                (5, 3, 4),
                (6, 3, 4),
                (7, 4, 5),
                (8, 5, 5);
            INSERT INTO taxon_names (taxon_id, name_type, name) VALUES
                (1, 1, 'Animalia'),
                (2, 1, 'Carnivora'),
                (3, 1, 'Canidae'),
                (4, 1, 'Canis'),
                (5, 1, 'Lycaon'),
                (6, 1, 'Cuon'),
                (7, 1, 'Shared species'),
                (8, 1, 'Shared species');
            "#,
        )
        .unwrap();
    drop(connection);

    let result = apply_rows(
        &database,
        &[TaxonInputRow {
            genus: Some("Cuon".into()),
            species: Some("Shared species".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();

    assert!(
        result.rows[0]
            .operation_types
            .contains(&TaxonRowStatus::NewTaxon)
    );
    assert_eq!(
        result.rows[0]
            .parent
            .as_ref()
            .and_then(|parent| parent.names.sci_name.as_deref()),
        Some("Cuon")
    );
}
