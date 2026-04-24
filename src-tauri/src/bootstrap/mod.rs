use std::path::Path;

use crate::config;
use crate::database;
use crate::domain::{
    AppInitializationPhase, AppInitializationState, BootstrapErrorInfo, BootstrapResult,
    ConfigValidationResult, PipelineConfig, WorkdirHealthSeverity, WorkdirState,
};
use crate::workdir;

pub fn initialize_workdir(path: &str) -> BootstrapResult {
    let workdir_path = workdir::normalize_path(path);
    match workdir::initialize(&workdir_path) {
        Ok(workdir_state) => bootstrap_from_workdir(workdir_state),
        Err(message) => failed_state(&workdir_path, "workdir_initialization_failed", message),
    }
}

pub fn open_workdir(path: &str) -> BootstrapResult {
    let workdir_path = workdir::normalize_path(path);
    let workdir_state = workdir::inspect(&workdir_path, false);
    bootstrap_from_workdir(workdir_state)
}

pub fn reload_workdir(path: &str) -> BootstrapResult {
    open_workdir(path)
}

fn bootstrap_from_workdir(workdir_state: WorkdirState) -> BootstrapResult {
    if !workdir_state.exists {
        return failed_with_workdir(
            workdir_state,
            "workdir_missing",
            "The selected workdir does not exist.",
        );
    }

    if !workdir_state.pipeline_config_exists {
        return state_from_parts(
            AppInitializationPhase::BootstrapFailed,
            "pipeline.yaml is missing.",
            Some(workdir_state),
            None,
            None,
            ConfigValidationResult::valid(),
            None,
            vec![BootstrapErrorInfo {
                code: "pipeline_config_missing".to_string(),
                message: "pipeline.yaml is required to open this workdir.".to_string(),
                path: None,
            }],
        );
    }

    let loaded_config =
        config::load_pipeline_config(Path::new(&workdir_state.pipeline_config_path));
    if !loaded_config.validation.is_valid {
        return state_from_parts(
            AppInitializationPhase::ConfigInvalid,
            "pipeline.yaml was loaded but failed validation.",
            Some(workdir_state),
            loaded_config.config,
            None,
            loaded_config.validation,
            Some(loaded_config.loaded_at),
            Vec::new(),
        );
    }

    let Some(config) = loaded_config.config else {
        return failed_with_workdir(
            workdir_state,
            "config_unavailable",
            "Configuration could not be built from pipeline.yaml.",
        );
    };

    match database::bootstrap_database(Path::new(&workdir_state.database_path), &config) {
        Ok(database_state) => {
            let refreshed_workdir_state =
                workdir::inspect(Path::new(&workdir_state.workdir_path), false);
            state_from_parts(
                AppInitializationPhase::FullyInitialized,
                "Workdir, config, SQLite schema, and stage sync are ready.",
                Some(refreshed_workdir_state),
                Some(config),
                Some(database_state),
                loaded_config.validation,
                Some(loaded_config.loaded_at),
                Vec::new(),
            )
        }
        Err(message) => state_from_parts(
            AppInitializationPhase::BootstrapFailed,
            "SQLite bootstrap failed.",
            Some(workdir_state),
            Some(config),
            None,
            loaded_config.validation,
            Some(loaded_config.loaded_at),
            vec![BootstrapErrorInfo {
                code: "database_bootstrap_failed".to_string(),
                message,
                path: None,
            }],
        ),
    }
}

fn state_from_parts(
    phase: AppInitializationPhase,
    message: impl Into<String>,
    workdir_state: Option<WorkdirState>,
    config: Option<PipelineConfig>,
    database_state: Option<crate::domain::DatabaseState>,
    validation: ConfigValidationResult,
    last_config_load_at: Option<String>,
    errors: Vec<BootstrapErrorInfo>,
) -> BootstrapResult {
    let selected_workdir_path = workdir_state
        .as_ref()
        .map(|state| state.workdir_path.clone());
    let config_path = workdir_state
        .as_ref()
        .map(|state| state.pipeline_config_path.clone());
    let database_path = workdir_state
        .as_ref()
        .map(|state| state.database_path.clone());
    let project_name = config.as_ref().map(|config| config.project.name.clone());
    let stage_ids: Vec<String> = config
    .as_ref()
    .map(|config| {
        config
            .stages
            .iter()
            .map(|stage| stage.id.clone())
            .collect()
    })
    .unwrap_or_default();

    let stage_count = stage_ids.len() as u64;
    let config_loaded = config.is_some() || !validation.is_valid;
    let database_status = if database_state.as_ref().is_some_and(|state| state.is_ready) {
        "ready"
    } else {
        "not_ready"
    };
    let config_status = if validation.is_valid && config.is_some() {
        "valid"
    } else if !validation.is_valid {
        "invalid"
    } else {
        "not_loaded"
    };

    BootstrapResult {
        state: AppInitializationState {
            phase,
            message: message.into(),
            selected_workdir_path,
            project_name,
            config_path,
            database_path,
            config_loaded,
            config_status: config_status.to_string(),
            database_status: database_status.to_string(),
            stage_count,
            stage_ids,
            last_config_load_at,
            validation,
            workdir_state,
            database_state,
            config,
            errors,
        },
    }
}

fn failed_state(path: &Path, code: &str, message: String) -> BootstrapResult {
    let workdir_state = workdir::inspect(path, false);
    state_from_parts(
        AppInitializationPhase::BootstrapFailed,
        message.clone(),
        Some(workdir_state),
        None,
        None,
        ConfigValidationResult::valid(),
        None,
        vec![BootstrapErrorInfo {
            code: code.to_string(),
            message,
            path: Some(workdir::path_string(path)),
        }],
    )
}

fn failed_with_workdir(workdir_state: WorkdirState, code: &str, message: &str) -> BootstrapResult {
    let path = Some(workdir_state.workdir_path.clone());
    state_from_parts(
        AppInitializationPhase::BootstrapFailed,
        message,
        Some(workdir_state),
        None,
        None,
        ConfigValidationResult::valid(),
        None,
        vec![BootstrapErrorInfo {
            code: code.to_string(),
            message: message.to_string(),
            path,
        }],
    )
}

#[allow(dead_code)]
fn has_blocking_workdir_issue(workdir_state: &WorkdirState) -> bool {
    workdir_state
        .health_issues
        .iter()
        .any(|issue| issue.severity == WorkdirHealthSeverity::Error)
}
