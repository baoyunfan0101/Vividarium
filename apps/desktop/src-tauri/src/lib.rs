mod commands;
mod media;
mod paths;
mod state;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = AppState::new(paths::data_dir(app.handle())?)?;
            state::set_global(state.clone())
                .map_err(|_| std::io::Error::other("application state already initialized"))?;
            app.manage(state);
            Ok(())
        })
        .register_uri_scheme_protocol("phytoindex", |_context, request| media::handle(request))
        .invoke_handler(tauri::generate_handler![
            commands::get_photo_library,
            commands::get_photo_library_count,
            commands::open_photo_library,
            commands::browse_photo_directory,
            commands::get_photo_directory_counts,
            commands::refresh_photo_directory,
            commands::start_photo_mapping,
            commands::rename_photo,
            commands::rename_photo_from_taxon,
            commands::rename_photos_from_taxa,
            commands::get_all_photos,
            commands::get_photo,
            commands::get_photo_metadata,
            commands::get_photo_availability,
            commands::get_taxa_metadata,
            commands::save_knowledge_base_path,
            commands::search_taxa,
            commands::get_taxon_detail_node,
            commands::list_taxon_children,
            commands::delete_taxon_name,
            commands::update_taxon,
            commands::delete_taxon,
            commands::execute_custom_taxonomy_sql,
            commands::list_taxonomy_operation_batches,
            commands::list_taxonomy_operations,
            commands::list_taxonomy_operations_for_batch,
            commands::get_mapping_metadata,
            commands::get_photo_taxon_match,
            commands::select_photo_taxon,
            commands::get_photo_taxon_node,
            commands::browse_photo_taxon,
            commands::list_photos_by_mapping_status,
            commands::suggest_mapping_taxa,
            commands::get_operations_status,
            commands::start_taxa_update,
            commands::start_taxa_rebuild,
            commands::export_table,
        ])
        .run(tauri::generate_context!())
        .expect("error while running PhytoIndex");
}
