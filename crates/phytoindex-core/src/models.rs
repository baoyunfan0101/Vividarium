use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Photo {
    pub photo_id: i64,
    pub root: String,
    pub relative_path: String,
    pub parent_dir: String,
    pub path_depth: i64,
    pub filename: String,
    pub binomial_name: Option<String>,
    pub captured_at: Option<String>,
    pub location: Option<String>,
    pub camera: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub file_size: Option<i64>,
    pub modified_at: Option<f64>,
    pub longitude: Option<f64>,
    pub latitude: Option<f64>,
    pub exif_json: Option<String>,
    pub thumbnail_path: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct NewPhoto {
    pub root: String,
    pub relative_path: String,
    pub parent_dir: String,
    pub path_depth: i64,
    pub filename: String,
    pub binomial_name: Option<String>,
    pub captured_at: Option<String>,
    pub location: Option<String>,
    pub camera: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub file_size: Option<i64>,
    pub modified_at: Option<f64>,
    pub longitude: Option<f64>,
    pub latitude: Option<f64>,
    pub exif_json: Option<String>,
    pub thumbnail_path: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PhotoRootMetadata {
    pub root: String,
    pub last_synced_at: Option<String>,
    pub sort_order: i64,
    pub photo_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DirectoryListingPage {
    pub root: String,
    pub relative_dir: String,
    pub directories: Vec<String>,
    pub files: Vec<Photo>,
    pub next_cursor: Option<String>,
    pub directory_count: i64,
    pub file_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Taxon {
    pub taxon_id: i64,
    pub rank: String,
    pub name: String,
    pub parent_id: Option<i64>,
    pub binomial_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaxaMetadata {
    pub knowledge_base_path: Option<String>,
    pub knowledge_base_size: Option<i64>,
    pub knowledge_base_modified_at: Option<String>,
    pub last_synced_at: Option<String>,
    pub taxa_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MappingMetadata {
    pub last_synced_at: Option<String>,
    pub photos_last_synced_at: Option<String>,
    pub taxa_last_synced_at: Option<String>,
    pub mapped_photo_count: i64,
    pub mapping_taxa_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MappingNode {
    pub taxon: Option<Taxon>,
    pub photo_ids: Vec<i64>,
    pub children: Vec<Taxon>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperationState {
    pub module: String,
    pub task_id: Option<String>,
    pub operation: Option<String>,
    pub running: bool,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub message: String,
    pub processed: u64,
    pub total: Option<u64>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

pub type OperationsStatus = BTreeMap<String, OperationState>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationResponse {
    pub needs_confirmation: bool,
    pub reason: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressUpdate {
    pub module: String,
    pub task_id: String,
    pub processed: u64,
    pub total: Option<u64>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoSyncResult {
    pub roots: Option<usize>,
    pub inserted: usize,
    pub unchanged: usize,
    pub updated: usize,
    pub new: usize,
    pub deleted: usize,
    pub other_roots_unchanged: usize,
    pub thumbnails_cleared: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxaSyncResult {
    pub knowledge_base_path: String,
    pub knowledge_base_size: i64,
    pub knowledge_base_modified_at: String,
    pub sheet: String,
    pub rows_read: usize,
    pub taxa_changed: usize,
    pub total_taxa: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingSyncResult {
    pub processed: usize,
    pub mapped: usize,
    pub unmapped: usize,
    pub unmapped_photos: Vec<Photo>,
    pub orphan_mappings_deleted: usize,
}
