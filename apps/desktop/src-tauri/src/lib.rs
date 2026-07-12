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
            commands::get_photo_roots_metadata,
            commands::save_photo_roots,
            commands::browse_photos_page,
            commands::get_all_photos,
            commands::get_changed_photos,
            commands::get_photo,
            commands::get_photo_availability,
            commands::get_map_photos,
            commands::get_taxa_metadata,
            commands::save_knowledge_base_path,
            commands::get_mapping_metadata,
            commands::get_mapping_root,
            commands::get_mapping_taxon,
            commands::search_mapping_by_name,
            commands::search_mapping_by_binomial,
            commands::suggest_mapping_taxa,
            commands::get_operations_status,
            commands::start_photos_update,
            commands::start_photos_rebuild,
            commands::start_taxa_update,
            commands::start_taxa_rebuild,
            commands::start_mapping_update,
            commands::start_mapping_rebuild,
            commands::export_table,
        ])
        .run(tauri::generate_context!())
        .expect("error while running PhytoIndex");
}
