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
pub mod http_api;
pub mod http_server;
mod pipeline_editor;
mod s3_client;
mod s3_control_envelope;
mod s3_manifest;
mod s3_reconciliation;
mod save_path;
mod services;
mod state_machine;
mod workdir;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::initialize_workdir,
            commands::list_registered_workspaces,
            commands::create_registered_workspace,
            commands::update_registered_workspace,
            commands::delete_registered_workspace,
            commands::restore_registered_workspace,
            commands::get_registered_workspace,
            commands::open_registered_workspace,
            commands::open_workdir,
            commands::reload_workdir,
            commands::get_dashboard_overview,
            commands::scan_workspace,
            commands::ensure_stage_directories,
            commands::get_runtime_summary,
            commands::list_stages,
            commands::get_pipeline_editor_state,
            commands::validate_pipeline_config_draft,
            commands::save_pipeline_config,
            commands::list_entities,
            commands::list_entity_files,
            commands::get_entity,
            commands::create_next_stage_copy,
            commands::run_due_tasks,
            commands::run_due_tasks_limited,
            commands::run_pipeline_waves,
            commands::run_entity_stage,
            commands::list_stage_runs,
            commands::reconcile_stuck_tasks,
            commands::list_app_events,
            commands::get_workspace_explorer,
            commands::get_workspace_explorer_by_id,
            commands::reconcile_s3_workspace,
            commands::reconcile_s3_workspace_by_id,
            commands::register_s3_source_artifact,
            commands::register_s3_source_artifact_by_id,
            commands::run_due_tasks_limited_by_id,
            commands::run_pipeline_waves_by_id,
            commands::run_selected_pipeline_waves_by_id,
            commands::create_s3_stage,
            commands::update_s3_stage,
            commands::delete_s3_stage,
            commands::restore_s3_stage,
            commands::update_stage_next_stage,
            commands::list_stage_run_outputs,
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
