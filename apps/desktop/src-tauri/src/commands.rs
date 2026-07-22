use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use phytoindex_core::mapping::{PhotoTaxonMapping, PhotoTaxonMatch};
use phytoindex_core::models::{
    DirectoryListingPage, MappingMetadata, MappingNode, OperationsStatus, Photo, PhotoLibrary,
    PhotoMetadata, PhotoSyncResult, TaxaMetadata, Taxon,
};
use phytoindex_core::taxonomy::{
    DeleteTaxonNameInput, TaxonChild, TaxonDetailNode, TaxonSearchResult, TaxonUpdateInput,
    TaxonUpdateOptions, TaxonomyActionResult, TaxonomyCustomSqlResult, TaxonomyCustomSqlTempTable,
    TaxonomyOperation, TaxonomyOperationBatch, TaxonomyPage, TaxonomyUpdateActionResult,
};
use phytoindex_core::{export, mapping, photos, taxa, taxonomy};
use serde_json::{Value, json};
use tauri::{AppHandle, State};

use crate::state::AppState;

type CommandResult<T> = Result<T, String>;

#[tauri::command]
pub fn get_photo_library(state: State<'_, AppState>) -> CommandResult<Option<PhotoLibrary>> {
    photos::get_library(&state.database).map_err(error)
}

#[tauri::command]
pub fn open_photo_library(state: State<'_, AppState>, root: String) -> CommandResult<PhotoLibrary> {
    photos::open_library(&state.database, &root).map_err(error)
}

#[tauri::command]
pub fn browse_photo_directory(
    state: State<'_, AppState>,
    directory_id: i64,
    cursor: Option<String>,
    limit: Option<usize>,
) -> CommandResult<DirectoryListingPage> {
    photos::browse_directory(
        &state.database,
        directory_id,
        cursor.as_deref(),
        limit.unwrap_or(160),
    )
    .map_err(error)
}

#[tauri::command]
pub fn refresh_photo_directory(
    state: State<'_, AppState>,
    directory_id: i64,
) -> CommandResult<PhotoSyncResult> {
    photos::refresh_directory(&state.database, directory_id).map_err(error)
}

#[tauri::command]
pub fn rename_photo(
    state: State<'_, AppState>,
    photo_id: i64,
    new_filename: String,
) -> CommandResult<Photo> {
    photos::rename_photo(&state.database, photo_id, &new_filename).map_err(error)
}

#[tauri::command]
pub fn get_all_photos(state: State<'_, AppState>) -> CommandResult<Vec<Photo>> {
    photos::list_photos(&state.database).map_err(error)
}

#[tauri::command]
pub fn get_photo(state: State<'_, AppState>, photo_id: i64) -> CommandResult<Photo> {
    photos::get_photo(&state.database, photo_id)
        .map_err(error)?
        .ok_or_else(|| format!("photo {photo_id} not found"))
}

#[tauri::command]
pub fn get_photo_availability(state: State<'_, AppState>, photo_id: i64) -> CommandResult<Value> {
    Ok(match photos::photo_file_path(&state.database, photo_id) {
        Ok(_) => json!({ "available": true, "error": null }),
        Err(error) => json!({ "available": false, "error": error.to_string() }),
    })
}

#[tauri::command]
pub fn get_photo_metadata(
    state: State<'_, AppState>,
    photo_id: i64,
) -> CommandResult<PhotoMetadata> {
    photos::get_photo_metadata(&state.database, photo_id).map_err(error)
}

#[tauri::command]
pub fn get_taxa_metadata(state: State<'_, AppState>) -> CommandResult<TaxaMetadata> {
    taxa::get_metadata(&state.database).map_err(error)
}

#[tauri::command]
pub fn save_knowledge_base_path(
    state: State<'_, AppState>,
    knowledge_base_path: Option<String>,
) -> CommandResult<TaxaMetadata> {
    taxa::save_knowledge_base_path(&state.database, knowledge_base_path.as_deref()).map_err(error)
}

#[tauri::command]
pub fn search_taxa(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> CommandResult<Vec<TaxonSearchResult>> {
    taxonomy::search_taxa(&state.database, &query, limit.unwrap_or(50)).map_err(error)
}

#[tauri::command]
pub fn get_taxon_detail_node(
    state: State<'_, AppState>,
    taxon_id: i64,
    children_cursor: Option<String>,
    children_limit: Option<usize>,
) -> CommandResult<TaxonDetailNode> {
    taxonomy::get_taxon_detail_node(
        &state.database,
        taxon_id,
        children_cursor.as_deref(),
        children_limit.unwrap_or(50),
    )
    .map_err(error)?
    .ok_or_else(|| format!("taxon {taxon_id} not found"))
}

#[tauri::command]
pub fn list_taxon_children(
    state: State<'_, AppState>,
    taxon_id: i64,
    cursor: Option<String>,
    limit: Option<usize>,
) -> CommandResult<TaxonomyPage<TaxonChild>> {
    taxonomy::list_taxon_children(
        &state.database,
        taxon_id,
        cursor.as_deref(),
        limit.unwrap_or(50),
    )
    .map_err(error)
}

#[tauri::command]
pub fn delete_taxon_name(
    state: State<'_, AppState>,
    input: DeleteTaxonNameInput,
) -> CommandResult<TaxonomyActionResult> {
    taxonomy::delete_taxon_name(&state.database, input).map_err(error)
}

#[tauri::command]
pub fn update_taxon(
    state: State<'_, AppState>,
    input: TaxonUpdateInput,
    options: Option<TaxonUpdateOptions>,
) -> CommandResult<TaxonomyUpdateActionResult> {
    taxonomy::update_taxon(&state.database, input, options.unwrap_or_default()).map_err(error)
}

#[tauri::command]
pub fn delete_taxon(
    state: State<'_, AppState>,
    taxon_id: i64,
) -> CommandResult<TaxonomyActionResult> {
    taxonomy::delete_taxon(&state.database, taxon_id).map_err(error)
}

#[tauri::command]
pub fn execute_custom_taxonomy_sql(
    state: State<'_, AppState>,
    sql: String,
    input: Option<TaxonomyCustomSqlTempTable>,
) -> CommandResult<TaxonomyCustomSqlResult> {
    taxonomy::execute_custom_taxonomy_sql(&state.database, &sql, input).map_err(error)
}

#[tauri::command]
pub fn list_taxonomy_operation_batches(
    state: State<'_, AppState>,
    cursor: Option<String>,
    limit: Option<usize>,
) -> CommandResult<TaxonomyPage<TaxonomyOperationBatch>> {
    taxonomy::list_taxonomy_operation_batches(
        &state.database,
        cursor.as_deref(),
        limit.unwrap_or(50),
    )
    .map_err(error)
}

#[tauri::command]
pub fn list_taxonomy_operations(
    state: State<'_, AppState>,
    cursor: Option<String>,
    limit: Option<usize>,
) -> CommandResult<TaxonomyPage<TaxonomyOperation>> {
    taxonomy::list_taxonomy_operations(&state.database, cursor.as_deref(), limit.unwrap_or(50))
        .map_err(error)
}

#[tauri::command]
pub fn list_taxonomy_operations_for_batch(
    state: State<'_, AppState>,
    batch_id: i64,
    cursor: Option<String>,
    limit: Option<usize>,
) -> CommandResult<TaxonomyPage<TaxonomyOperation>> {
    taxonomy::list_taxonomy_operations_for_batch(
        &state.database,
        batch_id,
        cursor.as_deref(),
        limit.unwrap_or(50),
    )
    .map_err(error)
}

#[tauri::command]
pub fn get_mapping_metadata(state: State<'_, AppState>) -> CommandResult<MappingMetadata> {
    mapping::get_metadata(&state.database).map_err(error)
}

#[tauri::command]
pub fn get_mapping_root(state: State<'_, AppState>) -> CommandResult<MappingNode> {
    mapping::get_root(&state.database).map_err(error)
}

#[tauri::command]
pub fn get_mapping_taxon(state: State<'_, AppState>, taxon_id: i64) -> CommandResult<MappingNode> {
    mapping::get_by_taxon_id(&state.database, Some(taxon_id)).map_err(error)
}

#[tauri::command]
pub fn get_photo_taxon_match(
    state: State<'_, AppState>,
    photo_id: i64,
) -> CommandResult<PhotoTaxonMatch> {
    mapping::get_photo_taxon_match(&state.database, photo_id).map_err(error)
}

#[tauri::command]
pub fn select_photo_taxon(
    state: State<'_, AppState>,
    photo_id: i64,
    taxon_id: i64,
) -> CommandResult<PhotoTaxonMapping> {
    mapping::select_photo_taxon(&state.database, photo_id, taxon_id).map_err(error)
}

#[tauri::command]
pub fn search_mapping_by_name(
    state: State<'_, AppState>,
    name: String,
) -> CommandResult<MappingNode> {
    mapping::get_by_name(&state.database, &name).map_err(error)
}

#[tauri::command]
pub fn search_mapping_by_binomial(
    state: State<'_, AppState>,
    binomial_name: String,
) -> CommandResult<MappingNode> {
    mapping::get_by_binomial(&state.database, &binomial_name).map_err(error)
}

#[tauri::command]
pub fn suggest_mapping_taxa(
    state: State<'_, AppState>,
    query: String,
    mode: String,
) -> CommandResult<Vec<Taxon>> {
    mapping::suggest(&state.database, &query, &mode, 10).map_err(error)
}

#[tauri::command]
pub fn get_operations_status(state: State<'_, AppState>) -> OperationsStatus {
    state.operations.status()
}

#[tauri::command]
pub fn start_taxa_update(
    app: AppHandle,
    state: State<'_, AppState>,
    knowledge_base_path: Option<String>,
    force: bool,
) -> CommandResult<Value> {
    start_taxa_operation(app, &state, knowledge_base_path, force, false)
}

#[tauri::command]
pub fn start_taxa_rebuild(
    app: AppHandle,
    state: State<'_, AppState>,
    knowledge_base_path: Option<String>,
    force: bool,
) -> CommandResult<Value> {
    start_taxa_operation(app, &state, knowledge_base_path, force, true)
}

#[tauri::command]
pub fn export_table(
    state: State<'_, AppState>,
    table_name: String,
    output_path: String,
) -> CommandResult<Value> {
    let exported = export::export_table(&state.database, &table_name, Path::new(&output_path))
        .map_err(error)?;
    Ok(json!({ "exported": exported, "output_path": output_path }))
}

fn start_taxa_operation(
    app: AppHandle,
    state: &AppState,
    knowledge_base_path: Option<String>,
    force: bool,
    rebuild: bool,
) -> CommandResult<Value> {
    if !force && taxa_input_unchanged(state, knowledge_base_path.as_deref())? {
        return Ok(confirmation(
            "knowledge_base_unchanged",
            "The selected knowledge-base file appears unchanged. Are you sure you want to continue this update/rebuild anyway?",
        ));
    }
    let database = state.database.clone();
    let operation_name = if rebuild { "rebuild" } else { "update" };
    let operation = state
        .operations
        .start(app, "taxa", operation_name, move |progress| {
            let result = if rebuild {
                taxa::rebuild_taxa(&database, knowledge_base_path.as_deref(), progress)
            } else {
                taxa::update_taxa(&database, knowledge_base_path.as_deref(), progress)
            }
            .map_err(error)?;
            serde_json::to_value(result).map_err(error)
        })?;
    Ok(json!({ "operation": operation }))
}

fn taxa_input_unchanged(state: &AppState, selected: Option<&str>) -> CommandResult<bool> {
    let metadata = taxa::get_metadata(&state.database).map_err(error)?;
    let selected = selected
        .map(PathBuf::from)
        .or_else(|| metadata.knowledge_base_path.as_deref().map(PathBuf::from));
    let Some(path) = selected else {
        return Ok(false);
    };
    let file = match fs::metadata(&path) {
        Ok(file) => file,
        Err(_) => return Ok(false),
    };
    let modified: DateTime<Local> = file.modified().map_err(error)?.into();
    Ok(metadata.knowledge_base_path.as_deref() == path.to_str()
        && metadata.knowledge_base_size == Some(file.len() as i64)
        && metadata.knowledge_base_modified_at.as_deref()
            == Some(modified.format("%Y-%m-%d %H:%M:%S").to_string().as_str()))
}

fn confirmation(reason: &str, message: &str) -> Value {
    json!({
        "needs_confirmation": true,
        "reason": reason,
        "message": message,
    })
}

fn error(error: impl ToString) -> String {
    error.to_string()
}
