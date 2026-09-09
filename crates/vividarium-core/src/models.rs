//! Shared serializable data-transfer objects used across backend modules.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Photo {
    pub photo_id: i64,
    pub directory_id: i64,
    pub relative_path: String,
    pub filename: String,
    pub file_size: i64,
    pub modified_at_ns: i64,
    pub thumbnail_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PhotoMetadata {
    pub photo_id: i64,
    pub captured_at: Option<String>,
    pub camera: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub longitude: Option<f64>,
    pub latitude: Option<f64>,
    pub exif_json: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct NewPhoto {
    pub directory_id: i64,
    pub filename: String,
    pub file_size: i64,
    pub modified_at_ns: i64,
    pub thumbnail_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PhotoLibrary {
    pub root_path: String,
    pub root_directory_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoLibraryRegistration {
    pub library_uuid: String,
    pub display_name: String,
    pub root_path: String,
    pub db_path: String,
    pub last_opened_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoLibraryLocation {
    pub library_uuid: String,
    pub root_path: String,
    pub database_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseLocations {
    pub metadata_database: String,
    pub taxonomy_database: String,
    pub default_taxonomy_directory: String,
    pub default_photo_library_directory: String,
    pub active_photo_library_uuid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PhotoDirectory {
    pub directory_id: i64,
    pub parent_directory_id: Option<i64>,
    pub name: String,
    pub relative_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PhotoPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PhotoDirectoryItem {
    Directory { directory: PhotoDirectory },
    Photo { photo: Photo },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DirectoryEntryCounts {
    pub directory_count: i64,
    pub file_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MappingMetadata {
    pub mapped_photo_count: i64,
    pub unmatched_photo_count: i64,
    pub ambiguous_photo_count: i64,
    pub processing_photo_count: i64,
    pub mapping_taxa_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperationProgress {
    pub stage: String,
    pub current: Option<u64>,
    pub total: Option<u64>,
    pub unit: Option<OperationProgressUnit>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationProgressUnit {
    Items,
    Files,
    Photos,
    Names,
    Taxa,
    Bytes,
    Statements,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskState {
    Queued,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperationState {
    pub module: String,
    pub task_id: Option<String>,
    pub task_kind: Option<String>,
    pub task_scope: Option<String>,
    pub state: BackgroundTaskState,
    pub operation: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub progress: Option<OperationProgress>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

impl OperationState {
    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            BackgroundTaskState::Queued | BackgroundTaskState::Running
        )
    }
}

pub type OperationsStatus = BTreeMap<String, OperationState>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoSyncResult {
    pub directory_id: i64,
    pub inserted: usize,
    pub unchanged: usize,
    pub updated: usize,
    pub deleted: usize,
    pub directories_inserted: usize,
    pub directories_deleted: usize,
}
