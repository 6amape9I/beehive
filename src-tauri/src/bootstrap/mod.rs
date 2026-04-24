use std::path::Path;

use crate::config;
use crate::database;
use crate::domain::{
    AppInitializationPhase, AppInitializationState, BootstrapErrorInfo, BootstrapResult,
    ConfigValidationResult, PipelineConfig, WorkdirHealthSeverity, WorkdirState,
};
use crate::workdir;

pub fn initialize_workdir(path: &str) -> BootstrapResult {
    let workdir_path = match resolve_workdir_path(path) {
        Ok(path) => path,
        Err(result) => return result,
    };
    match workdir::initialize(&workdir_path) {
        Ok(workdir_state) => bootstrap_from_workdir(workdir_state),
        Err(message) => failed_state(&workdir_path, "workdir_initialization_failed", message),
    }
}

pub fn open_workdir(path: &str) -> BootstrapResult {
    let workdir_path = match resolve_workdir_path(path) {
        Ok(path) => path,
        Err(result) => return result,
    };
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
    let stage_ids: Vec<String> = if let Some(database_state) = &database_state {
        database_state.synced_stage_ids.clone()
    } else {
        config
            .as_ref()
            .map(|config| config.stages.iter().map(|stage| stage.id.clone()).collect())
            .unwrap_or_default()
    };

    let stage_count = if let Some(database_state) = &database_state {
        database_state.stage_count
    } else {
        stage_ids.len() as u64
    };
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

fn resolve_workdir_path(path: &str) -> Result<std::path::PathBuf, BootstrapResult> {
    workdir::resolve_user_path(path).map_err(|message| {
        let code = if message.contains("outside the application directory") {
            "workdir_inside_application_directory"
        } else {
            "invalid_workdir_path"
        };
        invalid_path_state(path, code, message)
    })
}

fn invalid_path_state(path: &str, code: &str, message: String) -> BootstrapResult {
    let selected_workdir_path = Some(path.trim().to_string()).filter(|value| !value.is_empty());
    BootstrapResult {
        state: AppInitializationState {
            phase: AppInitializationPhase::BootstrapFailed,
            message: message.clone(),
            selected_workdir_path,
            project_name: None,
            config_path: None,
            database_path: None,
            config_status: "not_loaded".to_string(),
            database_status: "not_ready".to_string(),
            stage_count: 0,
            stage_ids: Vec::new(),
            last_config_load_at: None,
            validation: ConfigValidationResult::valid(),
            workdir_state: None,
            database_state: None,
            config: None,
            errors: vec![BootstrapErrorInfo {
                code: code.to_string(),
                message,
                path: None,
            }],
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::config::default_pipeline_yaml;

    fn write_pipeline(path: &Path, content: &str) {
        fs::write(path.join("pipeline.yaml"), content).expect("write pipeline");
    }

    #[test]
    fn initialize_workdir_bootstraps_to_fully_initialized() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("fresh-workdir");

        let result = initialize_workdir(&workdir.to_string_lossy());

        assert_eq!(result.state.phase, AppInitializationPhase::FullyInitialized);
        assert_eq!(result.state.config_status, "valid");
        assert_eq!(result.state.database_status, "ready");
        assert!(workdir.join("pipeline.yaml").exists());
        assert!(workdir.join("app.db").exists());
        assert!(workdir.join("stages").is_dir());
        assert!(workdir.join("logs").is_dir());
    }

    #[test]
    fn open_existing_workdir_returns_fully_initialized_state() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("existing-workdir");
        fs::create_dir_all(workdir.join("stages")).expect("create stages");
        fs::create_dir_all(workdir.join("logs")).expect("create logs");
        write_pipeline(&workdir, default_pipeline_yaml());

        let result = open_workdir(&workdir.to_string_lossy());

        assert_eq!(result.state.phase, AppInitializationPhase::FullyInitialized);
        assert_eq!(
            result.state.stage_ids,
            vec!["ingest".to_string(), "normalize".to_string()]
        );
        assert!(workdir.join("app.db").exists());
    }

    #[test]
    fn invalid_config_returns_config_invalid_state() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("invalid-workdir");
        fs::create_dir_all(&workdir).expect("create workdir");
        fs::create_dir_all(workdir.join("stages")).expect("create stages");
        fs::create_dir_all(workdir.join("logs")).expect("create logs");
        write_pipeline(
            &workdir,
            r#"
project:
  name: beehive
  workdir: .
stages:
  - id: ingest
    output_folder: stages/out
    workflow_url: http://localhost:5678/webhook/ingest
"#,
        );

        let result = open_workdir(&workdir.to_string_lossy());

        assert_eq!(result.state.phase, AppInitializationPhase::ConfigInvalid);
        assert_eq!(result.state.config_status, "invalid");
        assert_eq!(result.state.database_status, "not_ready");
        assert!(!result.state.validation.is_valid);
    }

    #[test]
    fn missing_pipeline_returns_bootstrap_failed_state() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("missing-pipeline");
        fs::create_dir_all(&workdir).expect("create workdir");

        let result = open_workdir(&workdir.to_string_lossy());

        assert_eq!(result.state.phase, AppInitializationPhase::BootstrapFailed);
        assert_eq!(result.state.config_status, "not_loaded");
        assert!(result
            .state
            .errors
            .iter()
            .any(|error| error.code == "pipeline_config_missing"));
    }

    #[test]
    fn relative_workdir_path_returns_bootstrap_failed_state() {
        let result = open_workdir("relative-workdir");

        assert_eq!(result.state.phase, AppInitializationPhase::BootstrapFailed);
        assert_eq!(
            result.state.selected_workdir_path.as_deref(),
            Some("relative-workdir")
        );
        assert!(result.state.errors.iter().any(|error| {
            error.code == "invalid_workdir_path" || error.message.contains("must be absolute")
        }));
    }

    #[test]
    fn workdir_inside_application_directory_returns_bootstrap_failed_state() {
        let nested_path = std::env::current_dir()
            .expect("current dir")
            .join("nested-workdir-inside-app");

        let result = open_workdir(&nested_path.to_string_lossy());

        assert_eq!(result.state.phase, AppInitializationPhase::BootstrapFailed);
        assert!(result
            .state
            .errors
            .iter()
            .any(|error| error.code == "workdir_inside_application_directory"));
    }
}
