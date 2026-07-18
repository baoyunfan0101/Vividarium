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
            0 | SCHEMA_VERSION => connection.execute_batch(SCHEMA)?,
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

CREATE TABLE IF NOT EXISTS scientific (
    taxon_id INTEGER NOT NULL,
    scientific_name TEXT NOT NULL,
    is_accepted INTEGER NOT NULL DEFAULT 0,
    authority_year TEXT,
    category TEXT,
    source TEXT,
    PRIMARY KEY (taxon_id, scientific_name),
    CHECK (is_accepted IN (0, 1)),
    CHECK (length(trim(scientific_name)) > 0),
    FOREIGN KEY (taxon_id) REFERENCES taxa(taxon_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS english (
    taxon_id INTEGER NOT NULL,
    english_name TEXT NOT NULL,
    is_accepted INTEGER NOT NULL DEFAULT 0,
    authority_year TEXT,
    category TEXT,
    source TEXT,
    PRIMARY KEY (taxon_id, english_name),
    CHECK (is_accepted IN (0, 1)),
    CHECK (length(trim(english_name)) > 0),
    FOREIGN KEY (taxon_id) REFERENCES taxa(taxon_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS chinese (
    taxon_id INTEGER NOT NULL,
    chinese_name TEXT NOT NULL,
    is_accepted INTEGER NOT NULL DEFAULT 0,
    authority_year TEXT,
    category TEXT,
    source TEXT,
    PRIMARY KEY (taxon_id, chinese_name),
    CHECK (is_accepted IN (0, 1)),
    CHECK (length(trim(chinese_name)) > 0),
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
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS taxonomy_operations (
    operation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id INTEGER NOT NULL,
    row_number INTEGER NOT NULL,
    taxon_id INTEGER NOT NULL,
    status TEXT NOT NULL,
    input_json TEXT NOT NULL,
    options_json TEXT NOT NULL,
    changes_json TEXT NOT NULL,
    before_json TEXT,
    after_json TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    reverted_at TEXT,
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

CREATE UNIQUE INDEX IF NOT EXISTS idx_scientific_one_accepted
    ON scientific(taxon_id) WHERE is_accepted = 1;
CREATE UNIQUE INDEX IF NOT EXISTS idx_english_one_accepted
    ON english(taxon_id) WHERE is_accepted = 1;
CREATE UNIQUE INDEX IF NOT EXISTS idx_chinese_one_accepted
    ON chinese(taxon_id) WHERE is_accepted = 1;
CREATE INDEX IF NOT EXISTS idx_taxa_parent ON taxa(parent_taxon_id);
CREATE INDEX IF NOT EXISTS idx_taxa_rank ON taxa(rank);
CREATE INDEX IF NOT EXISTS idx_scientific_name ON scientific(scientific_name);
CREATE INDEX IF NOT EXISTS idx_english_name ON english(english_name);
CREATE INDEX IF NOT EXISTS idx_chinese_name ON chinese(chinese_name);
CREATE INDEX IF NOT EXISTS idx_taxon_identifiers_taxon ON taxon_identifiers(taxon_id);
CREATE INDEX IF NOT EXISTS idx_taxonomy_operations_batch
    ON taxonomy_operations(batch_id, row_number);
CREATE INDEX IF NOT EXISTS idx_taxonomy_operations_taxon
    ON taxonomy_operations(taxon_id, operation_id);
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

CREATE VIEW IF NOT EXISTS taxa_display AS
SELECT
    taxa.taxon_id,
    taxa.rank,
    COALESCE(
        (SELECT chinese_name FROM chinese
         WHERE chinese.taxon_id = taxa.taxon_id AND is_accepted = 1),
        (SELECT english_name FROM english
         WHERE english.taxon_id = taxa.taxon_id AND is_accepted = 1),
        (SELECT scientific_name FROM scientific
         WHERE scientific.taxon_id = taxa.taxon_id AND is_accepted = 1),
        ''
    ) AS name,
    taxa.parent_taxon_id AS parent_id,
    (SELECT scientific_name FROM scientific
     WHERE scientific.taxon_id = taxa.taxon_id AND is_accepted = 1) AS binomial_name
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
            "scientific",
            "english",
            "chinese",
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
                "INSERT INTO scientific (taxon_id, scientific_name, is_accepted) VALUES (?, 'A a', 1)",
                [taxon_id],
            )
            .unwrap();
        let result = connection.execute(
            "INSERT INTO scientific (taxon_id, scientific_name, is_accepted) VALUES (?, 'A b', 1)",
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
}
