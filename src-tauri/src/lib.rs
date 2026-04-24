mod bootstrap;
mod commands;
mod config;
mod database;
mod discovery;
mod domain;
mod workdir;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::initialize_workdir,
            commands::open_workdir,
            commands::reload_workdir,
            commands::scan_workspace,
            commands::get_runtime_summary,
            commands::list_stages,
            commands::list_entities,
            commands::get_entity,
            commands::list_app_events,
            commands::get_workspace_explorer
        ])
        .run(tauri::generate_context!())
        .expect("error while running beehive");
}
