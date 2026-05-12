use std::path::{Path, PathBuf};

use crate::domain::StageRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SavePathRouteErrorKind {
    Unsafe,
    Unknown,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SavePathRouteError {
    pub kind: SavePathRouteErrorKind,
    pub message: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SavePathRoute {
    pub stage: StageRecord,
    pub logical_path: String,
    pub target_dir: PathBuf,
}

pub(crate) fn resolve_save_path_route(
    raw_save_path: &str,
    workdir_path: &Path,
    active_stages: &[StageRecord],
) -> Result<SavePathRoute, SavePathRouteError> {
    let requested = normalize_save_path(raw_save_path)?;
    let mut matches = Vec::new();

    for stage in active_stages.iter().filter(|stage| stage.is_active) {
        let Ok(stage_logical_path) = normalize_stage_input_folder(&stage.input_folder) else {
            continue;
        };
        if stage_logical_path == requested {
            matches.push((stage.clone(), stage_logical_path));
        }
    }

    match matches.len() {
        0 => Err(SavePathRouteError {
            kind: SavePathRouteErrorKind::Unknown,
            message: format!(
                "save_path '{}' does not match any active stage input_folder.",
                raw_save_path
            ),
        }),
        1 => {
            let (stage, logical_path) = matches.remove(0);
            let target_dir = target_dir_for_logical_path(workdir_path, &logical_path)?;
            Ok(SavePathRoute {
                stage,
                logical_path,
                target_dir,
            })
        }
        _ => Err(SavePathRouteError {
            kind: SavePathRouteErrorKind::Ambiguous,
            message: format!(
                "save_path '{}' matches more than one active stage input_folder.",
                raw_save_path
            ),
        }),
    }
}

pub(crate) fn route_for_stage_input_folder(
    workdir_path: &Path,
    stage: &StageRecord,
) -> Result<SavePathRoute, SavePathRouteError> {
    let logical_path = normalize_stage_input_folder(&stage.input_folder)?;
    let target_dir = target_dir_for_logical_path(workdir_path, &logical_path)?;
    Ok(SavePathRoute {
        stage: stage.clone(),
        logical_path,
        target_dir,
    })
}

fn normalize_save_path(raw_value: &str) -> Result<String, SavePathRouteError> {
    let trimmed = raw_value.trim();
    if trimmed.is_empty() {
        return unsafe_path("save_path is empty.");
    }

    let normalized_separators = trimmed.replace('\\', "/");
    if is_unc_path(trimmed, &normalized_separators) {
        return unsafe_path("save_path must not be a UNC path.");
    }
    if has_windows_drive_prefix(&normalized_separators) || normalized_separators.contains(':') {
        return unsafe_path("save_path must not contain a Windows drive prefix.");
    }

    let logical = if normalized_separators.starts_with('/') {
        if normalized_separators == "/main_dir" || normalized_separators.starts_with("/main_dir/")
        {
            normalized_separators.trim_start_matches('/').to_string()
        } else {
            return unsafe_path("save_path must not be an absolute OS path.");
        }
    } else {
        normalized_separators
    };

    normalize_relative_logical_path(&logical, "save_path")
}

fn normalize_stage_input_folder(raw_value: &str) -> Result<String, SavePathRouteError> {
    let trimmed = raw_value.trim();
    if trimmed.is_empty() {
        return unsafe_path("stage input_folder is empty.");
    }
    let normalized_separators = trimmed.replace('\\', "/");
    if normalized_separators.starts_with('/')
        || is_unc_path(trimmed, &normalized_separators)
        || has_windows_drive_prefix(&normalized_separators)
        || normalized_separators.contains(':')
    {
        return unsafe_path("stage input_folder must be a relative path inside the workdir.");
    }
    normalize_relative_logical_path(&normalized_separators, "stage input_folder")
}

fn normalize_relative_logical_path(
    value: &str,
    label: &str,
) -> Result<String, SavePathRouteError> {
    let mut components = Vec::new();
    for component in value.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                return unsafe_path(&format!("{label} must not contain '..' components."));
            }
            part => components.push(part),
        }
    }
    if components.is_empty() {
        return unsafe_path(&format!("{label} does not contain a relative path."));
    }
    Ok(components.join("/"))
}

fn target_dir_for_logical_path(
    workdir_path: &Path,
    logical_path: &str,
) -> Result<PathBuf, SavePathRouteError> {
    let workdir = workdir_path.canonicalize().map_err(|error| SavePathRouteError {
        kind: SavePathRouteErrorKind::Unsafe,
        message: format!(
            "Failed to canonicalize workdir '{}': {error}",
            workdir_path.display()
        ),
    })?;
    let mut target_dir = workdir.clone();
    for component in logical_path.split('/') {
        target_dir.push(component);
    }
    if !target_dir.starts_with(&workdir) {
        return unsafe_path("Resolved save_path target directory is outside the workdir.");
    }
    Ok(target_dir)
}

fn unsafe_path<T>(message: &str) -> Result<T, SavePathRouteError> {
    Err(SavePathRouteError {
        kind: SavePathRouteErrorKind::Unsafe,
        message: message.to_string(),
    })
}

fn has_windows_drive_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn is_unc_path(raw_value: &str, normalized_value: &str) -> bool {
    raw_value.starts_with("\\\\") || normalized_value.starts_with("//")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(id: &str, input_folder: &str) -> StageRecord {
        StageRecord {
            id: id.to_string(),
            input_folder: input_folder.to_string(),
            output_folder: String::new(),
            workflow_url: "http://localhost:5678/webhook/test".to_string(),
            max_attempts: 3,
            retry_delay_sec: 0,
            next_stage: None,
            is_active: true,
            archived_at: None,
            last_seen_in_config_at: None,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
            entity_count: 0,
        }
    }

    #[test]
    fn resolves_relative_and_legacy_main_dir_paths() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        std::fs::create_dir_all(&workdir).expect("workdir");
        let stages = vec![stage(
            "raw_entities",
            "main_dir/processed/raw_entities",
        )];

        let relative = resolve_save_path_route(
            "main_dir/processed/raw_entities",
            &workdir,
            &stages,
        )
        .expect("relative route");
        let legacy = resolve_save_path_route(
            "/main_dir/processed/raw_entities",
            &workdir,
            &stages,
        )
        .expect("legacy route");

        assert_eq!(relative.stage.id, "raw_entities");
        assert_eq!(legacy.stage.id, "raw_entities");
        assert_eq!(relative.logical_path, "main_dir/processed/raw_entities");
        assert!(relative.target_dir.starts_with(workdir.canonicalize().expect("canonical")));
    }

    #[test]
    fn rejects_unsafe_save_paths() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        std::fs::create_dir_all(&workdir).expect("workdir");
        let stages = vec![stage("target", "stages/target")];

        for value in [
            "",
            "../outside",
            "/etc/passwd",
            "C:\\Users\\bad\\file",
            "C:/Users/bad/file",
            "\\\\server\\share",
            "//server/share",
        ] {
            let error = resolve_save_path_route(value, &workdir, &stages)
                .expect_err("unsafe path should reject");
            assert_eq!(error.kind, SavePathRouteErrorKind::Unsafe);
        }
    }
}
