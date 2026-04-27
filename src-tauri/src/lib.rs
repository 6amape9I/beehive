mod bootstrap;
mod commands;
mod config;
mod dashboard;
mod database;
mod discovery;
mod domain;
mod executor;
mod file_open;
mod file_ops;
mod file_safety;
mod state_machine;
mod workdir;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::initialize_workdir,
            commands::open_workdir,
            commands::reload_workdir,
            commands::get_dashboard_overview,
            commands::scan_workspace,
            commands::ensure_stage_directories,
            commands::get_runtime_summary,
            commands::list_stages,
            commands::list_entities,
            commands::list_entity_files,
            commands::get_entity,
            commands::create_next_stage_copy,
            commands::run_due_tasks,
            commands::run_entity_stage,
            commands::list_stage_runs,
            commands::reconcile_stuck_tasks,
            commands::list_app_events,
            commands::get_workspace_explorer,
            commands::retry_entity_stage_now,
            commands::reset_entity_stage_to_pending,
            commands::skip_entity_stage,
            commands::open_entity_file,
            commands::open_entity_folder,
            commands::save_entity_file_business_json,
        ])
        .run(tauri::generate_context!())
        .expect("error while running beehive");
}
