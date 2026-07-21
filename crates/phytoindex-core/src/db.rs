use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, Row};

use crate::error::{CoreError, CoreResult};
use crate::models::{Photo, Taxon};

const SCHEMA_VERSION: i64 = 2;

#[derive(Debug, Clone)]
pub struct Database {
    path: PathBuf,
}

impl Database {
    pub fn open(path: impl Into<PathBuf>) -> CoreResult<Self> {
        let database = Self { path: path.into() };
        if let Some(parent) = database.path.parent() {
            fs::create_dir_all(parent)?;
        }
        database.initialize()?;
        Ok(database)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn connect(&self) -> CoreResult<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(Duration::from_secs(30))?;
        connection.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            "#,
        )?;
        Ok(connection)
    }

    fn initialize(&self) -> CoreResult<()> {
        let connection = self.connect()?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        match version {
            0 => connection.execute_batch(SCHEMA)?,
            SCHEMA_VERSION => {
                reset_prerelease_taxonomy_logs(&connection)?;
                connection.execute_batch(SCHEMA)?;
                migrate_legacy_taxon_name_tables(&connection)?;
            }
            1 => {
                return Err(CoreError::InvalidArgument(
                    "legacy database version 1 is not supported; open a new vividarium.db".into(),
                ));
            }
            _ => {
                return Err(CoreError::InvalidArgument(format!(
                    "unsupported database schema version: {version}"
                )));
            }
        }
        Ok(())
    }
}

fn migrate_legacy_taxon_name_tables(connection: &Connection) -> CoreResult<()> {
    for (table, kind, column) in [
        ("scientific", "scientific", "scientific_name"),
        ("english", "english", "english_name"),
        ("chinese", "chinese", "chinese_name"),
    ] {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
            [table],
            |row| row.get(0),
        )?;
        if exists {
            let sql = format!(
                r#"
                INSERT OR IGNORE INTO taxon_names (
                    taxon_id, name_kind, name, is_accepted, authority_year, category, source
                )
                SELECT taxon_id, ?1, {column}, is_accepted, authority_year, category, source
                FROM {table}
                "#
            );
            connection.execute(&sql, [kind])?;
            connection.execute(&format!("DROP TABLE {table}"), [])?;
        }
    }
    Ok(())
}

fn reset_prerelease_taxonomy_logs(connection: &Connection) -> CoreResult<()> {
    let has_obsolete_schema: bool = connection.query_row(
        r#"
        SELECT
            (
                EXISTS (
                    SELECT 1 FROM sqlite_master
                    WHERE type = 'table' AND name = 'taxonomy_operations'
                )
                AND (
                    SELECT json_group_array(name)
                    FROM pragma_table_info('taxonomy_operations')
                ) != json_array(
                    'operation_id',
                    'batch_id',
                    'row_number',
                    'status',
                    'changes_json',
                    'after_hash',
                    'applied_at',
                    'reverted_at'
                )
            )
            OR (
                EXISTS (
                    SELECT 1 FROM sqlite_master
                    WHERE type = 'table' AND name = 'taxonomy_operation_batches'
                )
                AND NOT EXISTS (
                    SELECT 1
                    FROM pragma_table_info('taxonomy_operation_batches')
                    WHERE name = 'context_json'
                )
            )
        "#,
        [],
        |row| row.get(0),
    )?;
    if has_obsolete_schema {
        connection.execute_batch(
            r#"
            DROP TABLE IF EXISTS taxonomy_operations;
            DROP TABLE IF EXISTS taxonomy_operation_batches;
            "#,
        )?;
    }
    Ok(())
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS photos (
    photo_id INTEGER PRIMARY KEY AUTOINCREMENT,
    root TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    parent_dir TEXT NOT NULL,
    path_depth INTEGER NOT NULL,
    filename TEXT NOT NULL,
    binomial_name TEXT,
    captured_at TEXT,
    location TEXT,
    camera TEXT,
    width INTEGER,
    height INTEGER,
    file_size INTEGER,
    modified_at REAL,
    longitude REAL,
    latitude REAL,
    exif_json TEXT,
    thumbnail_path TEXT DEFAULT NULL,
    status TEXT NOT NULL,
    UNIQUE(root, relative_path)
);

CREATE TABLE IF NOT EXISTS photos_dir (
    root TEXT NOT NULL,
    relative_dir TEXT NOT NULL,
    parent_dir TEXT NOT NULL,
    name TEXT NOT NULL,
    path_depth INTEGER NOT NULL,
    PRIMARY KEY (root, relative_dir)
);

CREATE TABLE IF NOT EXISTS photos_metadata (
    root TEXT PRIMARY KEY,
    last_synced_at TEXT,
    sort_order INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS taxa (
    taxon_id INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_taxon_id INTEGER,
    rank TEXT NOT NULL,
    geological_range TEXT,
    CHECK (rank IN ('kingdom', 'order', 'family', 'genus', 'species')),
    FOREIGN KEY (parent_taxon_id) REFERENCES taxa(taxon_id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS taxon_names (
    taxon_id INTEGER NOT NULL,
    name_kind TEXT NOT NULL,
    name TEXT NOT NULL,
    is_accepted INTEGER NOT NULL DEFAULT 0,
    authority_year TEXT,
    category TEXT,
    source TEXT,
    PRIMARY KEY (taxon_id, name_kind, name),
    CHECK (name_kind IN ('scientific', 'english', 'chinese')),
    CHECK (is_accepted IN (0, 1)),
    CHECK (length(trim(name)) > 0),
    FOREIGN KEY (taxon_id) REFERENCES taxa(taxon_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS taxon_identifiers (
    taxon_id INTEGER NOT NULL,
    source TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY (source, external_id),
    FOREIGN KEY (taxon_id) REFERENCES taxa(taxon_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS taxonomy_operation_batches (
    batch_id INTEGER PRIMARY KEY AUTOINCREMENT,
    context_json TEXT NOT NULL,
    input_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS taxonomy_operations (
    operation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id INTEGER NOT NULL,
    row_number INTEGER NOT NULL,
    status TEXT NOT NULL,
    changes_json TEXT NOT NULL,
    after_hash TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    reverted_at TEXT,
    UNIQUE (batch_id, row_number),
    CHECK (status IN ('applied', 'reverted')),
    FOREIGN KEY (batch_id) REFERENCES taxonomy_operation_batches(batch_id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS taxa_metadata (
    knowledge_base_path TEXT,
    knowledge_base_size INTEGER,
    knowledge_base_modified_at TEXT,
    last_synced_at TEXT
);

CREATE TABLE IF NOT EXISTS photos_taxa_mapping_metadata (
    last_synced_at TEXT,
    photos_last_synced_at TEXT,
    taxa_last_synced_at TEXT
);

CREATE TABLE IF NOT EXISTS photos_taxa_mapping (
    photo_id INTEGER PRIMARY KEY,
    taxon_id INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS photos_taxa_mapping_taxa (
    taxon_id INTEGER PRIMARY KEY,
    rank TEXT NOT NULL,
    name TEXT NOT NULL,
    parent_id INTEGER,
    binomial_name TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_taxon_names_one_accepted
    ON taxon_names(taxon_id, name_kind) WHERE is_accepted = 1;
CREATE INDEX IF NOT EXISTS idx_taxa_parent ON taxa(parent_taxon_id);
CREATE INDEX IF NOT EXISTS idx_taxa_parent_rank_id ON taxa(parent_taxon_id, rank, taxon_id);
CREATE INDEX IF NOT EXISTS idx_taxa_rank ON taxa(rank);
CREATE INDEX IF NOT EXISTS idx_taxon_names_kind_name ON taxon_names(name_kind, name);
CREATE INDEX IF NOT EXISTS idx_taxon_names_kind_taxon ON taxon_names(name_kind, taxon_id);
CREATE INDEX IF NOT EXISTS idx_taxon_names_name ON taxon_names(name);
CREATE INDEX IF NOT EXISTS idx_taxon_identifiers_taxon ON taxon_identifiers(taxon_id);
CREATE INDEX IF NOT EXISTS idx_taxonomy_operations_batch
    ON taxonomy_operations(batch_id, row_number);
CREATE INDEX IF NOT EXISTS idx_taxonomy_operations_batch_page
    ON taxonomy_operations(batch_id, row_number, operation_id);
CREATE INDEX IF NOT EXISTS idx_taxonomy_operation_batches_created
    ON taxonomy_operation_batches(created_at DESC, batch_id DESC);
CREATE INDEX IF NOT EXISTS idx_photos_root_path ON photos(root, relative_path);
CREATE INDEX IF NOT EXISTS idx_photos_browse
    ON photos(root, parent_dir, status, filename);
CREATE INDEX IF NOT EXISTS idx_photos_browse_cursor
    ON photos(root, parent_dir, status, filename, photo_id);
CREATE INDEX IF NOT EXISTS idx_photos_status ON photos(status);
CREATE INDEX IF NOT EXISTS idx_photos_binomial_name ON photos(binomial_name);
CREATE UNIQUE INDEX IF NOT EXISTS idx_photos_dir_unique
    ON photos_dir(root, relative_dir);
CREATE INDEX IF NOT EXISTS idx_photos_dir_browse
    ON photos_dir(root, parent_dir, name);
CREATE INDEX IF NOT EXISTS idx_photos_taxa_mapping_taxon
    ON photos_taxa_mapping(taxon_id);
CREATE INDEX IF NOT EXISTS idx_photos_taxa_mapping_taxa_parent
    ON photos_taxa_mapping_taxa(parent_id);
CREATE INDEX IF NOT EXISTS idx_photos_taxa_mapping_taxa_binomial
    ON photos_taxa_mapping_taxa(binomial_name);
CREATE INDEX IF NOT EXISTS idx_photos_taxa_mapping_taxa_name
    ON photos_taxa_mapping_taxa(name);

DROP VIEW IF EXISTS taxa_display;
CREATE VIEW IF NOT EXISTS taxa_display AS
SELECT
    taxa.taxon_id,
    taxa.rank,
    COALESCE(
        (SELECT name FROM taxon_names
         WHERE taxon_names.taxon_id = taxa.taxon_id AND name_kind = 'chinese' AND is_accepted = 1),
        (SELECT name FROM taxon_names
         WHERE taxon_names.taxon_id = taxa.taxon_id AND name_kind = 'english' AND is_accepted = 1),
        (SELECT name FROM taxon_names
         WHERE taxon_names.taxon_id = taxa.taxon_id AND name_kind = 'scientific' AND is_accepted = 1),
        ''
    ) AS name,
    taxa.parent_taxon_id AS parent_id,
    (SELECT name FROM taxon_names
     WHERE taxon_names.taxon_id = taxa.taxon_id AND name_kind = 'scientific' AND is_accepted = 1) AS binomial_name
FROM taxa;

PRAGMA user_version = 2;
"#;

pub(crate) fn photo_from_row(row: &Row<'_>) -> rusqlite::Result<Photo> {
    Ok(Photo {
        photo_id: row.get("photo_id")?,
        root: row.get("root")?,
        relative_path: row.get("relative_path")?,
        parent_dir: row.get("parent_dir")?,
        path_depth: row.get("path_depth")?,
        filename: row.get("filename")?,
        binomial_name: row.get("binomial_name")?,
        captured_at: row.get("captured_at")?,
        location: row.get("location")?,
        camera: row.get("camera")?,
        width: row.get("width")?,
        height: row.get("height")?,
        file_size: row.get("file_size")?,
        modified_at: row.get("modified_at")?,
        longitude: row.get("longitude")?,
        latitude: row.get("latitude")?,
        exif_json: row.get("exif_json")?,
        thumbnail_path: row.get("thumbnail_path")?,
        status: row.get("status")?,
    })
}

pub(crate) fn taxon_from_row(row: &Row<'_>) -> rusqlite::Result<Taxon> {
    Ok(Taxon {
        taxon_id: row.get("taxon_id")?,
        rank: row.get("rank")?,
        name: row.get("name")?,
        parent_id: row.get("parent_id")?,
        binomial_name: row.get("binomial_name")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_the_version_two_schema() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(directory.path().join("vividarium.db")).unwrap();
        let connection = database.connect().unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        for table in [
            "taxa",
            "taxon_names",
            "taxon_identifiers",
            "taxonomy_operation_batches",
            "taxonomy_operations",
        ] {
            let exists: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists, "missing table {table}");
        }
        let name_columns = table_columns(&connection, "taxon_names");
        assert_eq!(
            name_columns,
            [
                "taxon_id",
                "name_kind",
                "name",
                "is_accepted",
                "authority_year",
                "category",
                "source"
            ]
        );
        let batch_columns = table_columns(&connection, "taxonomy_operation_batches");
        assert_eq!(
            batch_columns,
            ["batch_id", "context_json", "input_json", "created_at"]
        );
        let operation_columns = table_columns(&connection, "taxonomy_operations");
        assert_eq!(
            operation_columns,
            [
                "operation_id",
                "batch_id",
                "row_number",
                "status",
                "changes_json",
                "after_hash",
                "applied_at",
                "reverted_at",
            ]
        );
    }

    #[test]
    fn resets_prerelease_taxonomy_logs_without_changing_the_schema_version() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vividarium.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE taxonomy_operation_batches (
                    batch_id INTEGER PRIMARY KEY,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE taxonomy_operations (
                    operation_id INTEGER PRIMARY KEY,
                    batch_id INTEGER NOT NULL,
                    row_number INTEGER NOT NULL,
                    taxon_id INTEGER NOT NULL,
                    status TEXT NOT NULL,
                    input_json TEXT NOT NULL,
                    legacy_options TEXT NOT NULL,
                    changes_json TEXT NOT NULL,
                    before_json TEXT,
                    after_json TEXT NOT NULL,
                    applied_at TEXT NOT NULL,
                    reverted_at TEXT
                );
                INSERT INTO taxonomy_operation_batches VALUES (1, CURRENT_TIMESTAMP);
                INSERT INTO taxonomy_operations VALUES (
                    1, 1, 1, 1, 'applied', '{}', '{}', '[]', NULL, '{}',
                    CURRENT_TIMESTAMP, NULL
                );
                PRAGMA user_version = 2;
                "#,
            )
            .unwrap();
        drop(connection);

        let database = Database::open(&path).unwrap();
        let connection = database.connect().unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(
            table_columns(&connection, "taxonomy_operation_batches"),
            ["batch_id", "context_json", "input_json", "created_at"]
        );
        let operation_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM taxonomy_operations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(operation_count, 0);
    }

    #[test]
    fn migrates_legacy_taxon_name_tables_into_one_table() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vividarium.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE taxa (
                    taxon_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    parent_taxon_id INTEGER,
                    rank TEXT NOT NULL,
                    geological_range TEXT
                );
                CREATE TABLE scientific (
                    taxon_id INTEGER NOT NULL,
                    scientific_name TEXT NOT NULL,
                    is_accepted INTEGER NOT NULL DEFAULT 0,
                    authority_year TEXT,
                    category TEXT,
                    source TEXT,
                    PRIMARY KEY (taxon_id, scientific_name)
                );
                CREATE TABLE english (
                    taxon_id INTEGER NOT NULL,
                    english_name TEXT NOT NULL,
                    is_accepted INTEGER NOT NULL DEFAULT 0,
                    authority_year TEXT,
                    category TEXT,
                    source TEXT,
                    PRIMARY KEY (taxon_id, english_name)
                );
                CREATE TABLE chinese (
                    taxon_id INTEGER NOT NULL,
                    chinese_name TEXT NOT NULL,
                    is_accepted INTEGER NOT NULL DEFAULT 0,
                    authority_year TEXT,
                    category TEXT,
                    source TEXT,
                    PRIMARY KEY (taxon_id, chinese_name)
                );
                INSERT INTO taxa (taxon_id, rank) VALUES (1, 'species');
                INSERT INTO scientific VALUES (1, 'Canis lupus', 1, '1758', 'valid', 'local');
                INSERT INTO english VALUES (1, 'gray wolf', 1, NULL, NULL, NULL);
                INSERT INTO chinese VALUES (1, 'wolf', 1, NULL, NULL, NULL);
                PRAGMA user_version = 2;
                "#,
            )
            .unwrap();
        drop(connection);

        let database = Database::open(&path).unwrap();
        let connection = database.connect().unwrap();
        assert_eq!(
            table_columns(&connection, "scientific"),
            Vec::<String>::new()
        );
        let names: Vec<(String, String, i64)> = {
            let mut statement = connection
                .prepare(
                    r#"
                    SELECT name_kind, name, is_accepted
                    FROM taxon_names
                    WHERE taxon_id = 1
                    ORDER BY name_kind, name
                    "#,
                )
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(
            names,
            [
                ("chinese".into(), "wolf".into(), 1),
                ("english".into(), "gray wolf".into(), 1),
                ("scientific".into(), "Canis lupus".into(), 1),
            ]
        );
    }

    #[test]
    fn rejects_a_second_accepted_name_of_the_same_kind() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(directory.path().join("vividarium.db")).unwrap();
        let connection = database.connect().unwrap();
        connection
            .execute("INSERT INTO taxa (rank) VALUES ('species')", [])
            .unwrap();
        let taxon_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO taxon_names (taxon_id, name_kind, name, is_accepted) VALUES (?, 'scientific', 'A a', 1)",
                [taxon_id],
            )
            .unwrap();
        let result = connection.execute(
            "INSERT INTO taxon_names (taxon_id, name_kind, name, is_accepted) VALUES (?, 'scientific', 'A b', 1)",
            [taxon_id],
        );
        assert!(result.is_err());
    }

    #[test]
    fn refuses_to_open_the_abandoned_schema() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("phytoindex.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("PRAGMA user_version = 1;")
            .unwrap();
        drop(connection);
        let error = Database::open(path).unwrap_err();
        assert!(error.to_string().contains("legacy database version 1"));
    }

    fn table_columns(connection: &Connection, table: &str) -> Vec<String> {
        let mut statement = connection
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        statement
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }
}
