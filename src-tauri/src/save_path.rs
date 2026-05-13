use std::path::{Path, PathBuf};

use crate::domain::{ArtifactLocation, S3StorageConfig, StageRecord, StorageProvider};

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
    #[allow(dead_code)]
    pub logical_path: String,
    pub target_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct S3SavePathRoute {
    pub stage: StageRecord,
    #[allow(dead_code)]
    pub logical_path: String,
    pub location: ArtifactLocation,
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

pub(crate) fn resolve_s3_save_path_route(
    raw_save_path: &str,
    storage: &S3StorageConfig,
    active_stages: &[StageRecord],
) -> Result<S3SavePathRoute, SavePathRouteError> {
    let requested = normalize_s3_requested_route(raw_save_path, storage)?;
    let mut matches = Vec::<(StageRecord, String)>::new();

    for stage in active_stages.iter().filter(|stage| stage.is_active) {
        let mut stage_paths = Vec::<String>::new();
        if let Some(input_uri) = stage.input_uri.as_deref() {
            if let Ok((bucket, key)) = parse_s3_uri(input_uri) {
                if bucket == storage.bucket {
                    stage_paths.push(key);
                }
            }
        }
        if !stage.input_folder.trim().is_empty() {
            if let Ok(input_folder) = normalize_stage_input_folder(&stage.input_folder) {
                stage_paths.push(input_folder);
            }
        }
        for alias in &stage.save_path_aliases {
            stage_paths.push(normalize_save_path(alias)?);
        }
        stage_paths.sort();
        stage_paths.dedup();
        if stage_paths.iter().any(|path| path == &requested) {
            matches.push((stage.clone(), requested.clone()));
        }
    }

    match matches.len() {
        0 => Err(SavePathRouteError {
            kind: SavePathRouteErrorKind::Unknown,
            message: format!("save_path '{raw_save_path}' does not match any active S3 route."),
        }),
        1 => {
            let (stage, logical_path) = matches.remove(0);
            Ok(S3SavePathRoute {
                stage,
                logical_path: logical_path.clone(),
                location: ArtifactLocation {
                    provider: StorageProvider::S3,
                    local_path: None,
                    bucket: Some(storage.bucket.clone()),
                    key: Some(logical_path),
                    version_id: None,
                    etag: None,
                    checksum_sha256: None,
                    size: None,
                },
            })
        }
        _ => Err(SavePathRouteError {
            kind: SavePathRouteErrorKind::Ambiguous,
            message: format!("save_path '{raw_save_path}' matches more than one active S3 route."),
        }),
    }
}

pub(crate) fn parse_s3_uri(value: &str) -> Result<(String, String), SavePathRouteError> {
    let Some(rest) = value.trim().strip_prefix("s3://") else {
        return unsafe_path("S3 URI must start with s3://.");
    };
    let Some((bucket, key)) = rest.split_once('/') else {
        return unsafe_path("S3 URI must include bucket and key prefix.");
    };
    if bucket.trim().is_empty() || key.trim().is_empty() {
        return unsafe_path("S3 URI must include non-empty bucket and key prefix.");
    }
    Ok((
        bucket.to_string(),
        normalize_relative_logical_path(key, "S3 key")?,
    ))
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
        if normalized_separators == "/main_dir" || normalized_separators.starts_with("/main_dir/") {
            normalized_separators.trim_start_matches('/').to_string()
        } else {
            return unsafe_path("save_path must not be an absolute OS path.");
        }
    } else {
        normalized_separators
    };

    normalize_relative_logical_path(&logical, "save_path")
}

fn normalize_s3_requested_route(
    raw_value: &str,
    storage: &S3StorageConfig,
) -> Result<String, SavePathRouteError> {
    let trimmed = raw_value.trim();
    if trimmed.starts_with("s3://") {
        let (bucket, key) = parse_s3_uri(trimmed)?;
        if bucket != storage.bucket {
            return Err(SavePathRouteError {
                kind: SavePathRouteErrorKind::Unknown,
                message: format!(
                    "save_path bucket '{}' is not configured storage bucket '{}'.",
                    bucket, storage.bucket
                ),
            });
        }
        Ok(key)
    } else {
        normalize_save_path(trimmed)
    }
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

fn normalize_relative_logical_path(value: &str, label: &str) -> Result<String, SavePathRouteError> {
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
    let workdir = workdir_path
        .canonicalize()
        .map_err(|error| SavePathRouteError {
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
            input_uri: None,
            output_folder: String::new(),
            workflow_url: "http://localhost:5678/webhook/test".to_string(),
            max_attempts: 3,
            retry_delay_sec: 0,
            next_stage: None,
            save_path_aliases: Vec::new(),
            allow_empty_outputs: false,
            is_active: true,
            archived_at: None,
            last_seen_in_config_at: None,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
            entity_count: 0,
        }
    }

    fn s3_stage(id: &str, input_uri: &str, aliases: Vec<&str>) -> StageRecord {
        StageRecord {
            id: id.to_string(),
            input_folder: String::new(),
            input_uri: Some(input_uri.to_string()),
            output_folder: String::new(),
            workflow_url: "http://localhost:5678/webhook/test".to_string(),
            max_attempts: 3,
            retry_delay_sec: 0,
            next_stage: None,
            save_path_aliases: aliases.into_iter().map(ToOwned::to_owned).collect(),
            allow_empty_outputs: false,
            is_active: true,
            archived_at: None,
            last_seen_in_config_at: None,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
            entity_count: 0,
        }
    }

    fn s3_config() -> S3StorageConfig {
        S3StorageConfig {
            bucket: "steos-s3-data".to_string(),
            workspace_prefix: "main_dir".to_string(),
            region: None,
            endpoint: None,
        }
    }

    #[test]
    fn resolves_relative_and_legacy_main_dir_paths() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        std::fs::create_dir_all(&workdir).expect("workdir");
        let stages = vec![stage("raw_entities", "main_dir/processed/raw_entities")];

        let relative =
            resolve_save_path_route("main_dir/processed/raw_entities", &workdir, &stages)
                .expect("relative route");
        let legacy = resolve_save_path_route("/main_dir/processed/raw_entities", &workdir, &stages)
            .expect("legacy route");

        assert_eq!(relative.stage.id, "raw_entities");
        assert_eq!(legacy.stage.id, "raw_entities");
        assert_eq!(relative.logical_path, "main_dir/processed/raw_entities");
        assert!(relative
            .target_dir
            .starts_with(workdir.canonicalize().expect("canonical")));
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

    #[test]
    fn resolves_s3_routes_from_logical_legacy_and_s3_uri_values() {
        let stages = vec![s3_stage(
            "raw_entities",
            "s3://steos-s3-data/main_dir/processed/raw_entities",
            vec!["/main_dir/processed/raw_entities"],
        )];
        let storage = s3_config();

        let logical =
            resolve_s3_save_path_route("main_dir/processed/raw_entities", &storage, &stages)
                .expect("logical");
        let legacy =
            resolve_s3_save_path_route("/main_dir/processed/raw_entities", &storage, &stages)
                .expect("legacy");
        let uri = resolve_s3_save_path_route(
            "s3://steos-s3-data/main_dir/processed/raw_entities",
            &storage,
            &stages,
        )
        .expect("uri");

        assert_eq!(logical.stage.id, "raw_entities");
        assert_eq!(legacy.logical_path, "main_dir/processed/raw_entities");
        assert_eq!(
            uri.location.key.as_deref(),
            Some("main_dir/processed/raw_entities")
        );
    }

    #[test]
    fn rejects_s3_unknown_and_unsafe_routes() {
        let stages = vec![s3_stage(
            "raw_entities",
            "s3://steos-s3-data/main_dir/processed/raw_entities",
            Vec::new(),
        )];
        let storage = s3_config();

        let unknown_bucket = resolve_s3_save_path_route(
            "s3://unknown-bucket/main_dir/processed/raw_entities",
            &storage,
            &stages,
        )
        .expect_err("bucket mismatch");
        let unknown_prefix =
            resolve_s3_save_path_route("main_dir/processed/unknown", &storage, &stages)
                .expect_err("prefix mismatch");

        assert_eq!(unknown_bucket.kind, SavePathRouteErrorKind::Unknown);
        assert_eq!(unknown_prefix.kind, SavePathRouteErrorKind::Unknown);
        for value in [
            "",
            "../outside",
            "s3://steos-s3-data/../../outside",
            "C:\\Users\\bad\\file",
            "\\\\server\\share",
        ] {
            let error =
                resolve_s3_save_path_route(value, &storage, &stages).expect_err("unsafe route");
            assert_eq!(error.kind, SavePathRouteErrorKind::Unsafe);
        }
    }

    #[test]
    fn ambiguous_s3_routes_are_rejected() {
        let stages = vec![
            s3_stage("a", "s3://steos-s3-data/main_dir/shared", Vec::new()),
            s3_stage("b", "s3://steos-s3-data/main_dir/shared", Vec::new()),
        ];
        let error = resolve_s3_save_path_route("main_dir/shared", &s3_config(), &stages)
            .expect_err("ambiguous");

        assert_eq!(error.kind, SavePathRouteErrorKind::Ambiguous);
    }
}
