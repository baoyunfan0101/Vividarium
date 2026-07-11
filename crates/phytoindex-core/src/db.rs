use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, Row};

use crate::error::CoreResult;
use crate::models::{Photo, Taxon};

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
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(connection)
    }

    fn initialize(&self) -> CoreResult<()> {
        let connection = self.connect()?;
        connection.execute_batch(
            r#"
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
                rank TEXT NOT NULL,
                name TEXT NOT NULL,
                parent_id INTEGER REFERENCES taxa(taxon_id) ON DELETE CASCADE,
                binomial_name TEXT
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

            CREATE INDEX IF NOT EXISTS idx_photos_root_path
                ON photos(root, relative_path);
            CREATE INDEX IF NOT EXISTS idx_photos_browse
                ON photos(root, parent_dir, status, filename);
            CREATE INDEX IF NOT EXISTS idx_photos_browse_cursor
                ON photos(root, parent_dir, status, filename, photo_id);
            CREATE INDEX IF NOT EXISTS idx_photos_status ON photos(status);
            CREATE INDEX IF NOT EXISTS idx_photos_binomial_name
                ON photos(binomial_name);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_photos_dir_unique
                ON photos_dir(root, relative_dir);
            CREATE INDEX IF NOT EXISTS idx_photos_dir_browse
                ON photos_dir(root, parent_dir, name);
            CREATE INDEX IF NOT EXISTS idx_taxa_parent ON taxa(parent_id);
            CREATE INDEX IF NOT EXISTS idx_taxa_binomial_name
                ON taxa(binomial_name);
            CREATE INDEX IF NOT EXISTS idx_photos_taxa_mapping_taxon
                ON photos_taxa_mapping(taxon_id);
            CREATE INDEX IF NOT EXISTS idx_photos_taxa_mapping_taxa_parent
                ON photos_taxa_mapping_taxa(parent_id);
            CREATE INDEX IF NOT EXISTS idx_photos_taxa_mapping_taxa_binomial
                ON photos_taxa_mapping_taxa(binomial_name);
            CREATE INDEX IF NOT EXISTS idx_photos_taxa_mapping_taxa_name
                ON photos_taxa_mapping_taxa(name);

            PRAGMA user_version = 1;
            "#,
        )?;
        Ok(())
    }
}

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
    fn initializes_the_version_one_schema() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(directory.path().join("phytoindex.db")).unwrap();
        let connection = database.connect().unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);
        for table in [
            "photos",
            "photos_dir",
            "photos_metadata",
            "taxa",
            "taxa_metadata",
            "photos_taxa_mapping",
            "photos_taxa_mapping_metadata",
            "photos_taxa_mapping_taxa",
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
}
