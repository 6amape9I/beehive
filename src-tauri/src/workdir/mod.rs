use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::config::default_pipeline_yaml;
use crate::domain::{WorkdirHealthIssue, WorkdirHealthSeverity, WorkdirState};

pub fn initialize(path: &Path) -> Result<WorkdirState, String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("Failed to create workdir '{}': {error}", path.display()))?;

    let stages_dir = path.join("stages");
    let logs_dir = path.join("logs");
    fs::create_dir_all(&stages_dir).map_err(|error| {
        format!(
            "Failed to create stages directory '{}': {error}",
            stages_dir.display()
        )
    })?;
    fs::create_dir_all(&logs_dir).map_err(|error| {
        format!(
            "Failed to create logs directory '{}': {error}",
            logs_dir.display()
        )
    })?;

    let pipeline_path = path.join("pipeline.yaml");
    if !pipeline_path.exists() {
        fs::write(&pipeline_path, default_pipeline_yaml()).map_err(|error| {
            format!(
                "Failed to write default pipeline config '{}': {error}",
                pipeline_path.display()
            )
        })?;
    }

    Ok(inspect(path, true))
}

pub fn inspect(path: &Path, initialization_flow: bool) -> WorkdirState {
    let pipeline_path = path.join("pipeline.yaml");
    let database_path = path.join("app.db");
    let stages_dir = path.join("stages");
    let logs_dir = path.join("logs");

    let exists = path.exists() && path.is_dir();
    let pipeline_config_exists = pipeline_path.exists() && pipeline_path.is_file();
    let database_exists = database_path.exists() && database_path.is_file();
    let stages_dir_exists = stages_dir.exists() && stages_dir.is_dir();
    let logs_dir_exists = logs_dir.exists() && logs_dir.is_dir();

    let mut health_issues = Vec::new();
    if !exists {
        health_issues.push(health_issue(
            WorkdirHealthSeverity::Error,
            "workdir_missing",
            path,
            "The selected workdir does not exist.",
        ));
    }
    if exists && !pipeline_config_exists {
        health_issues.push(health_issue(
            WorkdirHealthSeverity::Error,
            "pipeline_config_missing",
            &pipeline_path,
            "pipeline.yaml is required to open an existing workdir.",
        ));
    }
    if exists && !database_exists {
        health_issues.push(health_issue(
            WorkdirHealthSeverity::Info,
            "database_missing",
            &database_path,
            "app.db is missing and will be created during bootstrap.",
        ));
    }
    if exists && !stages_dir_exists && !initialization_flow {
        health_issues.push(health_issue(
            WorkdirHealthSeverity::Warning,
            "stages_dir_missing",
            &stages_dir,
            "stages/ is missing.",
        ));
    }
    if exists && !logs_dir_exists && !initialization_flow {
        health_issues.push(health_issue(
            WorkdirHealthSeverity::Warning,
            "logs_dir_missing",
            &logs_dir,
            "logs/ is missing.",
        ));
    }

    WorkdirState {
        workdir_path: path_string(path),
        pipeline_config_path: path_string(&pipeline_path),
        database_path: path_string(&database_path),
        stages_dir_path: path_string(&stages_dir),
        logs_dir_path: path_string(&logs_dir),
        exists,
        pipeline_config_exists,
        database_exists,
        stages_dir_exists,
        logs_dir_exists,
        health_issues,
    }
}

pub fn resolve_user_path(path: &str) -> Result<PathBuf, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Workdir path cannot be empty.".to_string());
    }

    let candidate = PathBuf::from(trimmed);
    if !candidate.is_absolute() {
        return Err(
            "Workdir path must be absolute. Use Browse or enter a full path outside the application directory."
                .to_string(),
        );
    }

    let runtime_dir = fs::canonicalize(
        std::env::current_dir()
            .map_err(|error| format!("Failed to determine the application directory: {error}"))?,
    )
    .map_err(|error| format!("Failed to resolve the application directory: {error}"))?;

    resolve_user_path_with_runtime_dir(&candidate, &runtime_dir)
}

fn resolve_user_path_with_runtime_dir(path: &Path, runtime_dir: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(
            "Workdir path must be absolute. Use Browse or enter a full path outside the application directory."
                .to_string(),
        );
    }

    let normalized = normalize_absolute_path(path)?;
    let comparison_target = canonicalize_for_validation(&normalized)?;

    if path_is_within(&comparison_target, runtime_dir) {
        return Err(format!(
            "Workdir must be outside the application directory '{}'. Choose a separate folder to avoid dev-mode restarts and mutable runtime files inside the app tree.",
            runtime_dir.display()
        ));
    }

    Ok(strip_windows_verbatim_prefix(&comparison_target))
}

fn normalize_absolute_path(path: &Path) -> Result<PathBuf, String> {
    let mut normalized = PathBuf::new();
    let mut tail = Vec::<OsString>::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::Normal(part) => tail.push(part.to_os_string()),
            Component::ParentDir => {
                if tail.pop().is_none() {
                    return Err(format!(
                        "Workdir path '{}' escapes beyond its root through '..'.",
                        path.display()
                    ));
                }
            }
        }
    }

    for part in tail {
        normalized.push(part);
    }

    Ok(normalized)
}

fn canonicalize_for_validation(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return fs::canonicalize(path).map_err(|error| {
            format!(
                "Failed to resolve workdir path '{}': {error}",
                path.display()
            )
        });
    }

    let mut existing_parent = path;
    let mut remainder = Vec::<OsString>::new();

    while !existing_parent.exists() {
        let Some(name) = existing_parent.file_name() else {
            return Err(format!(
                "Cannot validate workdir path '{}' because no existing parent directory could be resolved.",
                path.display()
            ));
        };
        remainder.push(name.to_os_string());
        existing_parent = existing_parent.parent().ok_or_else(|| {
            format!(
                "Cannot validate workdir path '{}' because no existing parent directory could be resolved.",
                path.display()
            )
        })?;
    }

    let mut resolved = fs::canonicalize(existing_parent).map_err(|error| {
        format!(
            "Failed to resolve workdir parent directory '{}': {error}",
            existing_parent.display()
        )
    })?;

    for component in remainder.iter().rev() {
        resolved.push(component);
    }

    normalize_absolute_path(&resolved)
}

fn path_is_within(path: &Path, parent: &Path) -> bool {
    let path_components = normalized_components(path);
    let parent_components = normalized_components(parent);

    path_components.len() >= parent_components.len()
        && path_components
            .iter()
            .zip(parent_components.iter())
            .all(|(left, right)| left == right)
}

fn normalized_components(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

pub fn path_string(path: &Path) -> String {
    strip_windows_verbatim_prefix(path)
        .to_string_lossy()
        .to_string()
}

fn strip_windows_verbatim_prefix(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

fn health_issue(
    severity: WorkdirHealthSeverity,
    code: impl Into<String>,
    path: &Path,
    message: impl Into<String>,
) -> WorkdirHealthIssue {
    WorkdirHealthIssue {
        severity,
        code: code.into(),
        path: path_string(path),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[test]
    fn initialize_creates_required_stage_one_files_and_directories() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("new-workdir");

        let state = initialize(&workdir).expect("initialize workdir");

        assert_eq!(state.workdir_path, path_string(&workdir));
        assert!(workdir.join("pipeline.yaml").exists());
        assert!(workdir.join("stages").is_dir());
        assert!(workdir.join("logs").is_dir());
        assert!(!workdir.join("app.db").exists());
        assert!(state.pipeline_config_exists);
        assert!(!state.database_exists);
        assert!(state.stages_dir_exists);
        assert!(state.logs_dir_exists);
    }

    #[test]
    fn inspect_existing_workdir_reports_healthy_state() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("existing-workdir");
        fs::create_dir_all(workdir.join("stages")).expect("create stages");
        fs::create_dir_all(workdir.join("logs")).expect("create logs");
        fs::write(workdir.join("pipeline.yaml"), default_pipeline_yaml()).expect("write pipeline");
        fs::write(workdir.join("app.db"), []).expect("write db placeholder");

        let state = inspect(&workdir, false);

        assert!(state.exists);
        assert!(state.pipeline_config_exists);
        assert!(state.database_exists);
        assert!(state.stages_dir_exists);
        assert!(state.logs_dir_exists);
        assert!(state.health_issues.is_empty());
    }

    #[test]
    fn resolve_user_path_rejects_relative_paths() {
        let error = resolve_user_path_with_runtime_dir(
            Path::new("relative-workdir"),
            &env::temp_dir().join("beehive-runtime"),
        )
        .expect_err("relative path should fail");

        assert!(error.contains("must be absolute"));
    }

    #[test]
    fn resolve_user_path_rejects_paths_inside_application_directory() {
        let runtime_dir = tempfile::tempdir().expect("tempdir");
        let runtime_canonical = fs::canonicalize(runtime_dir.path()).expect("canonical runtime");
        let workdir = runtime_canonical.join("nested-workdir");

        let error = resolve_user_path_with_runtime_dir(&workdir, &runtime_canonical)
            .expect_err("nested workdir should fail");

        assert!(error.contains("outside the application directory"));
    }

    #[test]
    fn resolve_user_path_rejects_disguised_nested_paths_with_dot_dot() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let runtime_dir = tempdir.path().join("runtime");
        fs::create_dir_all(&runtime_dir).expect("create runtime");
        let runtime_canonical = fs::canonicalize(&runtime_dir).expect("canonical runtime");
        let disguised = runtime_canonical
            .join("allowed")
            .join("..")
            .join("nested-workdir");

        let error = resolve_user_path_with_runtime_dir(&disguised, &runtime_canonical)
            .expect_err("disguised nested workdir should fail");

        assert!(error.contains("outside the application directory"));
    }

    #[test]
    fn resolve_user_path_accepts_absolute_path_outside_runtime_directory() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let runtime_dir = tempdir.path().join("runtime");
        let external_parent = tempdir.path().join("external-parent");
        fs::create_dir_all(&runtime_dir).expect("create runtime");
        fs::create_dir_all(&external_parent).expect("create external parent");
        let runtime_canonical = fs::canonicalize(&runtime_dir).expect("canonical runtime");
        let candidate = external_parent.join("stage2-workdir");

        let resolved = resolve_user_path_with_runtime_dir(&candidate, &runtime_canonical)
            .expect("absolute path outside runtime should work");

        assert_eq!(resolved, candidate);
    }
}
