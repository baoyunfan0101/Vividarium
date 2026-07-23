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
                validate_photo_schema(&connection)?;
                migrate_taxon_name_index(&connection)?;
                connection.execute_batch(SCHEMA)?;
            }
            1 => {
                return Err(CoreError::InvalidArgument(
                    "legacy database schema is not supported; open a new vividarium.db".into(),
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

fn migrate_taxon_name_index(connection: &Connection) -> CoreResult<()> {
    let mut statement = connection.prepare("PRAGMA table_xinfo(taxon_names)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if columns.iter().any(|column| column == "name_search")
        && !columns.iter().any(|column| column == "normalized_name")
    {
        connection.execute_batch(
            r#"
            ALTER TABLE taxon_names RENAME COLUMN name_search TO normalized_name;
            "#,
        )?;
    }
    Ok(())
}

fn validate_photo_schema(connection: &Connection) -> CoreResult<()> {
    let mut statement = connection.prepare("PRAGMA table_info(photos)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.is_empty()
        && columns
            != [
                "photo_id",
                "directory_id",
                "filename",
                "file_size",
                "modified_at_ns",
                "thumbnail_path",
            ]
    {
        return Err(CoreError::InvalidArgument(
            "legacy photos schema is not supported; open a new vividarium.db".into(),
        ));
    }
    Ok(())
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS photo_library (
    library_id INTEGER PRIMARY KEY CHECK (library_id = 1),
    root_path TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS photo_directories (
    directory_id INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_directory_id INTEGER,
    name TEXT NOT NULL,
    relative_path TEXT NOT NULL UNIQUE,
    UNIQUE (parent_directory_id, name),
    FOREIGN KEY (parent_directory_id) REFERENCES photo_directories(directory_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS photos (
    photo_id INTEGER PRIMARY KEY AUTOINCREMENT,
    directory_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    modified_at_ns INTEGER NOT NULL,
    thumbnail_path TEXT,
    UNIQUE (directory_id, filename),
    CHECK (length(filename) > 0),
    CHECK (file_size >= 0),
    FOREIGN KEY (directory_id) REFERENCES photo_directories(directory_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS photo_metadata (
    photo_id INTEGER PRIMARY KEY,
    captured_at TEXT,
    camera TEXT,
    width INTEGER,
    height INTEGER,
    longitude REAL,
    latitude REAL,
    exif_json TEXT,
    FOREIGN KEY (photo_id) REFERENCES photos(photo_id) ON DELETE CASCADE
);

CREATE VIRTUAL TABLE IF NOT EXISTS photo_filenames_fts USING fts5(
    filename,
    content = 'photos',
    content_rowid = 'photo_id',
    tokenize = 'trigram'
);

CREATE TRIGGER IF NOT EXISTS photos_ai AFTER INSERT ON photos BEGIN
    INSERT INTO photo_filenames_fts(rowid, filename) VALUES (new.photo_id, new.filename);
END;

CREATE TRIGGER IF NOT EXISTS photos_ad AFTER DELETE ON photos BEGIN
    INSERT INTO photo_filenames_fts(photo_filenames_fts, rowid, filename)
    VALUES ('delete', old.photo_id, old.filename);
END;

CREATE TRIGGER IF NOT EXISTS photos_au AFTER UPDATE OF filename ON photos BEGIN
    INSERT INTO photo_filenames_fts(photo_filenames_fts, rowid, filename)
    VALUES ('delete', old.photo_id, old.filename);
    INSERT INTO photo_filenames_fts(rowid, filename) VALUES (new.photo_id, new.filename);
END;

CREATE TABLE IF NOT EXISTS photo_taxon_mapping (
    photo_id INTEGER PRIMARY KEY,
    taxon_id INTEGER,
    status TEXT NOT NULL,
    CHECK (status IN ('matched', 'unmatched', 'ambiguous', 'processing', 'stale')),
    CHECK ((status = 'matched' AND taxon_id IS NOT NULL)
        OR (status != 'matched' AND taxon_id IS NULL)),
    FOREIGN KEY (photo_id) REFERENCES photos(photo_id) ON DELETE CASCADE,
    FOREIGN KEY (taxon_id) REFERENCES taxa(taxon_id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS photo_taxon_usage (
    taxon_id INTEGER PRIMARY KEY,
    direct_photo_count INTEGER NOT NULL,
    subtree_photo_count INTEGER NOT NULL,
    CHECK (direct_photo_count >= 0),
    CHECK (subtree_photo_count >= direct_photo_count),
    FOREIGN KEY (taxon_id) REFERENCES taxa(taxon_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS photo_mapping_queue (
    photo_id INTEGER PRIMARY KEY,
    reason TEXT NOT NULL,
    CHECK (reason IN ('refresh', 'taxonomy')),
    FOREIGN KEY (photo_id) REFERENCES photos(photo_id) ON DELETE CASCADE
);

DROP TABLE IF EXISTS photo_mapping_state;

CREATE TABLE IF NOT EXISTS taxa (
    taxon_id INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_taxon_id INTEGER,
    rank INTEGER NOT NULL,
    geological_range TEXT,
    CHECK (rank IN (1, 2, 3, 4, 5)),
    FOREIGN KEY (parent_taxon_id) REFERENCES taxa(taxon_id) ON DELETE RESTRICT
);

CREATE TRIGGER IF NOT EXISTS taxa_bd_photo_mapping
BEFORE DELETE ON taxa BEGIN
    INSERT INTO photo_mapping_queue (photo_id, reason)
    SELECT photo_id, 'taxonomy'
    FROM photo_taxon_mapping
    WHERE taxon_id = old.taxon_id
    ON CONFLICT(photo_id) DO UPDATE SET reason = excluded.reason;
    UPDATE photo_taxon_mapping
    SET taxon_id = NULL, status = 'stale'
    WHERE taxon_id = old.taxon_id;
END;

CREATE TABLE IF NOT EXISTS taxon_names (
    name_id INTEGER PRIMARY KEY AUTOINCREMENT,
    taxon_id INTEGER NOT NULL,
    name_kind INTEGER NOT NULL,
    name TEXT NOT NULL,
    normalized_name TEXT GENERATED ALWAYS AS (lower(name)) STORED,
    is_accepted INTEGER NOT NULL DEFAULT 0,
    authority_year TEXT,
    category TEXT,
    source TEXT,
    UNIQUE (taxon_id, name_kind, name),
    CHECK (name_kind IN (1, 2, 3)),
    CHECK (is_accepted IN (0, 1)),
    CHECK (length(trim(name)) > 0),
    FOREIGN KEY (taxon_id) REFERENCES taxa(taxon_id) ON DELETE CASCADE
);

CREATE VIRTUAL TABLE IF NOT EXISTS taxon_names_fts USING fts5(
    name,
    content = 'taxon_names',
    content_rowid = 'name_id',
    tokenize = 'trigram'
);

CREATE TRIGGER IF NOT EXISTS taxon_names_ai AFTER INSERT ON taxon_names BEGIN
    INSERT INTO taxon_names_fts(rowid, name) VALUES (new.name_id, new.name);
END;

CREATE TRIGGER IF NOT EXISTS taxon_names_ad AFTER DELETE ON taxon_names BEGIN
    INSERT INTO taxon_names_fts(taxon_names_fts, rowid, name)
    VALUES ('delete', old.name_id, old.name);
END;

CREATE TRIGGER IF NOT EXISTS taxon_names_au AFTER UPDATE OF name ON taxon_names BEGIN
    INSERT INTO taxon_names_fts(taxon_names_fts, rowid, name)
    VALUES ('delete', old.name_id, old.name);
    INSERT INTO taxon_names_fts(rowid, name) VALUES (new.name_id, new.name);
END;

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
    changeset_blob BLOB NOT NULL,
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

CREATE TRIGGER IF NOT EXISTS taxa_photo_mapping_bd BEFORE DELETE ON taxa BEGIN
    UPDATE photo_taxon_mapping
    SET taxon_id = NULL, status = 'stale'
    WHERE taxon_id = old.taxon_id;
END;

CREATE UNIQUE INDEX IF NOT EXISTS idx_taxon_names_one_accepted
    ON taxon_names(taxon_id, name_kind) WHERE is_accepted = 1;
CREATE INDEX IF NOT EXISTS idx_taxa_parent ON taxa(parent_taxon_id);
CREATE INDEX IF NOT EXISTS idx_taxa_parent_rank_id ON taxa(parent_taxon_id, rank, taxon_id);
CREATE INDEX IF NOT EXISTS idx_taxa_rank ON taxa(rank);
CREATE INDEX IF NOT EXISTS idx_taxon_names_kind_name ON taxon_names(name_kind, name);
CREATE INDEX IF NOT EXISTS idx_taxon_names_kind_taxon ON taxon_names(name_kind, taxon_id);
CREATE INDEX IF NOT EXISTS idx_taxon_names_name ON taxon_names(name);
CREATE INDEX IF NOT EXISTS idx_taxon_names_name_search
    ON taxon_names(normalized_name, taxon_id);
CREATE INDEX IF NOT EXISTS idx_taxon_identifiers_taxon ON taxon_identifiers(taxon_id);
CREATE INDEX IF NOT EXISTS idx_taxonomy_operations_batch
    ON taxonomy_operations(batch_id, row_number);
CREATE INDEX IF NOT EXISTS idx_taxonomy_operations_batch_page
    ON taxonomy_operations(batch_id, row_number, operation_id);
CREATE INDEX IF NOT EXISTS idx_taxonomy_operation_batches_created
    ON taxonomy_operation_batches(created_at DESC, batch_id DESC);
CREATE INDEX IF NOT EXISTS idx_photo_directories_parent_name
    ON photo_directories(parent_directory_id, name, directory_id);
CREATE INDEX IF NOT EXISTS idx_photos_directory_filename
    ON photos(directory_id, filename, photo_id);
CREATE INDEX IF NOT EXISTS idx_photo_taxon_mapping_taxon
    ON photo_taxon_mapping(taxon_id, photo_id);
CREATE INDEX IF NOT EXISTS idx_photo_taxon_mapping_status
    ON photo_taxon_mapping(status, photo_id);
CREATE INDEX IF NOT EXISTS idx_photo_taxon_usage_subtree
    ON photo_taxon_usage(subtree_photo_count, taxon_id);
CREATE INDEX IF NOT EXISTS idx_photo_mapping_queue_reason
    ON photo_mapping_queue(reason, photo_id);

DROP VIEW IF EXISTS taxa_display;
CREATE VIEW IF NOT EXISTS taxa_display AS
SELECT
    taxa.taxon_id,
    CASE taxa.rank
        WHEN 1 THEN 'kingdom'
        WHEN 2 THEN 'order'
        WHEN 3 THEN 'family'
        WHEN 4 THEN 'genus'
        WHEN 5 THEN 'species'
    END AS rank,
    COALESCE(
        (SELECT name FROM taxon_names
         WHERE taxon_names.taxon_id = taxa.taxon_id AND name_kind = 3 AND is_accepted = 1),
        (SELECT name FROM taxon_names
         WHERE taxon_names.taxon_id = taxa.taxon_id AND name_kind = 2 AND is_accepted = 1),
        (SELECT name FROM taxon_names
         WHERE taxon_names.taxon_id = taxa.taxon_id AND name_kind = 1 AND is_accepted = 1),
        ''
    ) AS name,
    taxa.parent_taxon_id AS parent_id,
    (SELECT name FROM taxon_names
     WHERE taxon_names.taxon_id = taxa.taxon_id AND name_kind = 1 AND is_accepted = 1) AS binomial_name
FROM taxa;

PRAGMA user_version = 2;
"#;

pub(crate) fn photo_from_row(row: &Row<'_>) -> rusqlite::Result<Photo> {
    Ok(Photo {
        photo_id: row.get("photo_id")?,
        directory_id: row.get("directory_id")?,
        relative_path: row.get("relative_path")?,
        filename: row.get("filename")?,
        file_size: row.get("file_size")?,
        modified_at_ns: row.get("modified_at_ns")?,
        thumbnail_path: row.get("thumbnail_path")?,
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
            "photo_library",
            "photo_directories",
            "photos",
            "photo_metadata",
            "photo_filenames_fts",
            "photo_taxon_mapping",
            "photo_taxon_usage",
            "photo_mapping_queue",
            "taxa",
            "taxon_names",
            "taxon_names_fts",
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
                "name_id",
                "taxon_id",
                "name_kind",
                "name",
                "is_accepted",
                "authority_year",
                "category",
                "source"
            ]
        );
        let name_columns = table_xcolumns(&connection, "taxon_names");
        assert!(name_columns.contains(&"normalized_name".to_string()));
        let photo_columns = table_columns(&connection, "photos");
        assert_eq!(
            photo_columns,
            [
                "photo_id",
                "directory_id",
                "filename",
                "file_size",
                "modified_at_ns",
                "thumbnail_path",
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
                "changeset_blob",
                "applied_at",
                "reverted_at",
            ]
        );
    }

    #[test]
    fn rejects_a_second_accepted_name_of_the_same_kind() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(directory.path().join("vividarium.db")).unwrap();
        let connection = database.connect().unwrap();
        connection
            .execute("INSERT INTO taxa (rank) VALUES (5)", [])
            .unwrap();
        let taxon_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO taxon_names (taxon_id, name_kind, name, is_accepted) VALUES (?, 1, 'A a', 1)",
                [taxon_id],
            )
            .unwrap();
        let result = connection.execute(
            "INSERT INTO taxon_names (taxon_id, name_kind, name, is_accepted) VALUES (?, 1, 'A b', 1)",
            [taxon_id],
        );
        assert!(result.is_err());
    }

    #[test]
    fn refuses_to_open_legacy_schema_versions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("phytoindex.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("PRAGMA user_version = 1;")
            .unwrap();
        drop(connection);
        let error = Database::open(path).unwrap_err();
        assert!(error.to_string().contains("legacy database schema"));
    }

    #[test]
    fn refuses_the_legacy_photos_layout_at_version_two() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("phytoindex.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE photos (
                    photo_id INTEGER PRIMARY KEY,
                    root TEXT NOT NULL,
                    relative_path TEXT NOT NULL,
                    filename TEXT NOT NULL
                );
                PRAGMA user_version = 2;
                "#,
            )
            .unwrap();
        drop(connection);
        let error = Database::open(path).unwrap_err();
        assert!(error.to_string().contains("legacy photos schema"));
    }

    #[test]
    fn renames_the_version_two_taxon_name_index_in_place() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("phytoindex.db");
        let database = Database::open(&path).unwrap();
        let connection = database.connect().unwrap();
        connection
            .execute_batch(
                r#"
                DROP INDEX idx_taxon_names_name_search;
                ALTER TABLE taxon_names RENAME COLUMN normalized_name TO name_search;
                CREATE INDEX idx_taxon_names_name_search
                    ON taxon_names(name_search, taxon_id);
                "#,
            )
            .unwrap();
        drop(connection);
        drop(database);

        let database = Database::open(path).unwrap();
        let connection = database.connect().unwrap();
        let columns = table_xcolumns(&connection, "taxon_names");
        assert!(columns.contains(&"normalized_name".to_string()));
        assert!(!columns.contains(&"name_search".to_string()));
        let index_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = 'idx_taxon_names_name_search')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(index_exists);
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

    fn table_xcolumns(connection: &Connection, table: &str) -> Vec<String> {
        let mut statement = connection
            .prepare(&format!("PRAGMA table_xinfo({table})"))
            .unwrap();
        statement
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }
}
