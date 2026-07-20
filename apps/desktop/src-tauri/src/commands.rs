use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use phytoindex_core::models::{
    DirectoryListingPage, MappingMetadata, MappingNode, OperationsStatus, Photo, PhotoRootMetadata,
    TaxaMetadata, Taxon,
};
use phytoindex_core::taxonomy::{
    DeleteTaxonNameInput, TaxonDetailNode, TaxonSearchResult, TaxonUpdateInput, TaxonUpdateOptions,
    TaxonomyActionResult, TaxonomyCustomSqlResult, TaxonomyOperation, TaxonomyOperationBatch,
    TaxonomyUpdateActionResult,
};
use phytoindex_core::{export, mapping, photos, taxa, taxonomy};
use serde_json::{Value, json};
use tauri::{AppHandle, State};

use crate::state::AppState;

type CommandResult<T> = Result<T, String>;

#[tauri::command]
pub fn get_photo_roots_metadata(
    state: State<'_, AppState>,
) -> CommandResult<Vec<PhotoRootMetadata>> {
    photos::get_roots_metadata(&state.database).map_err(error)
}

#[tauri::command]
pub fn save_photo_roots(
    state: State<'_, AppState>,
    roots: Vec<String>,
) -> CommandResult<Vec<PhotoRootMetadata>> {
    photos::save_roots(&state.database, &roots).map_err(error)
}

#[tauri::command]
pub fn browse_photos_page(
    state: State<'_, AppState>,
    root: String,
    relative_dir: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
) -> CommandResult<DirectoryListingPage> {
    photos::browse_photos_page(
        &state.database,
        &root,
        relative_dir.as_deref().unwrap_or_default(),
        cursor.as_deref(),
        limit.unwrap_or(160),
    )
    .map_err(error)
}

#[tauri::command]
pub fn get_all_photos(state: State<'_, AppState>) -> CommandResult<Vec<Photo>> {
    photos::list_photos(&state.database).map_err(error)
}

#[tauri::command]
pub fn get_changed_photos(state: State<'_, AppState>) -> CommandResult<Vec<Photo>> {
    photos::list_changed_photos(&state.database).map_err(error)
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
pub fn get_map_photos(state: State<'_, AppState>) -> CommandResult<Vec<Photo>> {
    photos::list_map_photos(&state.database, None).map_err(error)
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
) -> CommandResult<TaxonDetailNode> {
    taxonomy::get_taxon_detail_node(&state.database, taxon_id)
        .map_err(error)?
        .ok_or_else(|| format!("taxon {taxon_id} not found"))
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
) -> CommandResult<TaxonomyCustomSqlResult> {
    taxonomy::execute_custom_taxonomy_sql(&state.database, &sql).map_err(error)
}

#[tauri::command]
pub fn list_taxonomy_operation_batches(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> CommandResult<Vec<TaxonomyOperationBatch>> {
    taxonomy::list_taxonomy_operation_batches(&state.database, limit.unwrap_or(50)).map_err(error)
}

#[tauri::command]
pub fn list_taxonomy_operations_for_batch(
    state: State<'_, AppState>,
    batch_id: i64,
) -> CommandResult<Vec<TaxonomyOperation>> {
    taxonomy::list_taxonomy_operations_for_batch(&state.database, batch_id).map_err(error)
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
pub fn start_photos_update(
    app: AppHandle,
    state: State<'_, AppState>,
    roots: Vec<String>,
) -> CommandResult<Value> {
    if roots.is_empty() {
        return Err("at least one photo root is required".into());
    }
    let database = state.database.clone();
    let operation = state
        .operations
        .start(app, "photos", "update", move |progress| {
            let results = photos::update_photos_many(&database, &roots, progress).map_err(error)?;
            Ok(json!({ "roots": roots.len(), "results": results }))
        })?;
    Ok(json!({ "operation": operation }))
}

#[tauri::command]
pub fn start_photos_rebuild(
    app: AppHandle,
    state: State<'_, AppState>,
    roots: Vec<String>,
    force: bool,
) -> CommandResult<Value> {
    if !force {
        return Ok(confirmation(
            "photos_rebuild_clears_thumbnails",
            "Rebuilding photos will clear all cached thumbnails and rebuild the photos table. Are you sure you want to continue?",
        ));
    }
    if roots.is_empty() {
        return Err("at least one photo root is required".into());
    }
    let database = state.database.clone();
    let thumbnail_dir = state.thumbnail_dir.clone();
    let operation = state
        .operations
        .start(app, "photos", "rebuild", move |progress| {
            serde_json::to_value(
                photos::rebuild_photos(&database, &roots, &thumbnail_dir, progress)
                    .map_err(error)?,
            )
            .map_err(error)
        })?;
    Ok(json!({ "operation": operation }))
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
pub fn start_mapping_update(
    app: AppHandle,
    state: State<'_, AppState>,
    force: bool,
) -> CommandResult<Value> {
    start_mapping_operation(app, &state, force, false)
}

#[tauri::command]
pub fn start_mapping_rebuild(
    app: AppHandle,
    state: State<'_, AppState>,
    force: bool,
) -> CommandResult<Value> {
    start_mapping_operation(app, &state, force, true)
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

fn start_mapping_operation(
    app: AppHandle,
    state: &AppState,
    force: bool,
    rebuild: bool,
) -> CommandResult<Value> {
    if !force && let Some(value) = mapping_confirmation(state)? {
        return Ok(value);
    }
    let database = state.database.clone();
    let operation_name = if rebuild { "rebuild" } else { "update" };
    let operation = state
        .operations
        .start(app, "mapping", operation_name, move |progress| {
            let result = if rebuild {
                mapping::rebuild_mapping(&database, progress)
            } else {
                mapping::update_mapping(&database, progress)
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

fn mapping_confirmation(state: &AppState) -> CommandResult<Option<Value>> {
    let photos_metadata = photos::get_roots_metadata(&state.database).map_err(error)?;
    let photos_latest = photos_metadata
        .iter()
        .filter_map(|value| value.last_synced_at.as_deref())
        .max()
        .map(str::to_string);
    let taxa_latest = taxa::get_metadata(&state.database)
        .map_err(error)?
        .last_synced_at;
    let mapping = mapping::get_metadata(&state.database).map_err(error)?;
    let unchanged = mapping.last_synced_at.is_some()
        && mapping.photos_last_synced_at == photos_latest
        && mapping.taxa_last_synced_at == taxa_latest;
    if unchanged {
        return Ok(Some(confirmation(
            "mapping_inputs_unchanged",
            "Photos and taxa appear unchanged since the last mapping sync. Are you sure you want to continue this update/rebuild anyway?",
        )));
    }
    if matches!((&taxa_latest, &photos_latest), (Some(taxa), Some(photos)) if taxa > photos) {
        return Ok(Some(confirmation(
            "taxa_newer_than_photos",
            "Taxa were synced later than photos. Confirm before updating mapping.",
        )));
    }
    Ok(None)
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
