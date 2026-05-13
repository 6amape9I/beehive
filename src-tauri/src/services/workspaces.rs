use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::domain::{StorageProvider, WorkspaceDescriptor};

#[derive(Debug, Clone, Deserialize)]
struct WorkspaceRegistryFile {
    workspaces: Vec<WorkspaceRegistryRecord>,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkspaceRegistryRecord {
    id: String,
    name: String,
    provider: StorageProvider,
    bucket: Option<String>,
    workspace_prefix: Option<String>,
    region: Option<String>,
    endpoint: Option<String>,
    workdir_path: String,
    pipeline_path: String,
    database_path: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RegisteredWorkspace {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) provider: StorageProvider,
    pub(crate) bucket: Option<String>,
    pub(crate) workspace_prefix: Option<String>,
    pub(crate) region: Option<String>,
    pub(crate) endpoint: Option<String>,
    pub(crate) workdir_path: PathBuf,
    pub(crate) pipeline_path: PathBuf,
    pub(crate) database_path: PathBuf,
}

impl RegisteredWorkspace {
    pub(crate) fn descriptor(&self) -> WorkspaceDescriptor {
        WorkspaceDescriptor {
            id: self.id.clone(),
            name: self.name.clone(),
            provider: self.provider.clone(),
            bucket: self.bucket.clone(),
            workspace_prefix: self.workspace_prefix.clone(),
            region: self.region.clone(),
            endpoint: self.endpoint.clone(),
        }
    }
}

pub(crate) fn list_workspace_descriptors() -> Result<Vec<WorkspaceDescriptor>, String> {
    Ok(load_registry()?
        .into_iter()
        .map(|workspace| workspace.descriptor())
        .collect())
}

pub(crate) fn get_workspace_descriptor(workspace_id: &str) -> Result<WorkspaceDescriptor, String> {
    Ok(get_workspace(workspace_id)?.descriptor())
}

pub(crate) fn get_workspace(workspace_id: &str) -> Result<RegisteredWorkspace, String> {
    let normalized_id = workspace_id.trim();
    if normalized_id.is_empty() {
        return Err("workspace_id is required.".to_string());
    }
    load_registry()?
        .into_iter()
        .find(|workspace| workspace.id == normalized_id)
        .ok_or_else(|| format!("Unknown workspace_id '{normalized_id}'."))
}

pub(crate) fn registry_path() -> PathBuf {
    if let Some(path) = std::env::var_os("BEEHIVE_WORKSPACES_CONFIG") {
        return PathBuf::from(path);
    }

    let cwd_candidate = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("config")
        .join("workspaces.yaml");
    if cwd_candidate.exists() {
        return cwd_candidate;
    }

    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|path| path.join("config").join("workspaces.yaml"))
        .unwrap_or(cwd_candidate)
}

fn load_registry() -> Result<Vec<RegisteredWorkspace>, String> {
    load_registry_from_path(&registry_path())
}

pub(crate) fn load_registry_from_path(path: &Path) -> Result<Vec<RegisteredWorkspace>, String> {
    let content = fs::read_to_string(path).map_err(|error| {
        format!(
            "Failed to read workspace registry '{}': {error}",
            path.display()
        )
    })?;
    let registry: WorkspaceRegistryFile = serde_yaml::from_str(&content).map_err(|error| {
        format!(
            "Failed to parse workspace registry '{}': {error}",
            path.display()
        )
    })?;
    validate_and_build_registry(registry)
}

fn validate_and_build_registry(
    registry: WorkspaceRegistryFile,
) -> Result<Vec<RegisteredWorkspace>, String> {
    let mut seen_ids = HashSet::new();
    let mut workspaces = Vec::new();

    for (index, record) in registry.workspaces.into_iter().enumerate() {
        let prefix = format!("workspaces[{index}]");
        let id = normalize_required(&record.id, &format!("{prefix}.id"))?;
        if !is_safe_workspace_id(&id) {
            return Err(format!(
                "{prefix}.id must contain only letters, numbers, underscores, and hyphens."
            ));
        }
        if !seen_ids.insert(id.clone()) {
            return Err(format!("Duplicate workspace id '{id}' in registry."));
        }
        let name = normalize_required(&record.name, &format!("{prefix}.name"))?;
        let workdir_path = absolute_path(&record.workdir_path, &format!("{prefix}.workdir_path"))?;
        let pipeline_path =
            absolute_path(&record.pipeline_path, &format!("{prefix}.pipeline_path"))?;
        let database_path =
            absolute_path(&record.database_path, &format!("{prefix}.database_path"))?;

        if !pipeline_path.starts_with(&workdir_path) {
            return Err(format!(
                "{prefix}.pipeline_path must be inside the workspace workdir."
            ));
        }
        if !database_path.starts_with(&workdir_path) {
            return Err(format!(
                "{prefix}.database_path must be inside the workspace workdir."
            ));
        }

        let bucket = normalize_optional(record.bucket);
        let workspace_prefix = normalize_optional(record.workspace_prefix);
        if record.provider == StorageProvider::S3 {
            if bucket.is_none() {
                return Err(format!("{prefix}.bucket is required for S3 workspaces."));
            }
            if workspace_prefix.is_none() {
                return Err(format!(
                    "{prefix}.workspace_prefix is required for S3 workspaces."
                ));
            }
        }

        workspaces.push(RegisteredWorkspace {
            id,
            name,
            provider: record.provider,
            bucket,
            workspace_prefix,
            region: normalize_optional(record.region),
            endpoint: normalize_optional(record.endpoint),
            workdir_path,
            pipeline_path,
            database_path,
        });
    }

    Ok(workspaces)
}

fn normalize_required(value: &str, path: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(format!("{path} is required."))
    } else {
        Ok(value.to_string())
    }
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn absolute_path(value: &str, path: &str) -> Result<PathBuf, String> {
    let normalized = normalize_required(value, path)?;
    let path_buf = PathBuf::from(&normalized);
    if !path_buf.is_absolute() {
        return Err(format!("{path} must be an absolute server-side path."));
    }
    Ok(path_buf)
}

fn is_safe_workspace_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_registry_loads_valid_config_without_exposing_paths() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("smoke");
        let registry_path = tempdir.path().join("workspaces.yaml");
        fs::write(
            &registry_path,
            format!(
                r#"
workspaces:
  - id: smoke
    name: Smoke
    provider: s3
    bucket: steos-s3-data
    workspace_prefix: beehive-smoke/test_workflow
    region: ru-1
    endpoint: https://s3.example
    workdir_path: {}
    pipeline_path: {}
    database_path: {}
"#,
                workdir.display(),
                workdir.join("pipeline.yaml").display(),
                workdir.join("app.db").display()
            ),
        )
        .expect("registry");

        let workspaces = load_registry_from_path(&registry_path).expect("load registry");
        assert_eq!(workspaces.len(), 1);
        let descriptor = workspaces[0].descriptor();
        assert_eq!(descriptor.id, "smoke");
        assert_eq!(descriptor.bucket.as_deref(), Some("steos-s3-data"));
    }

    #[test]
    fn workspace_registry_rejects_duplicate_ids() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("smoke");
        let registry_path = tempdir.path().join("workspaces.yaml");
        fs::write(
            &registry_path,
            format!(
                r#"
workspaces:
  - id: smoke
    name: Smoke
    provider: s3
    bucket: bucket
    workspace_prefix: prefix
    workdir_path: {}
    pipeline_path: {}
    database_path: {}
  - id: smoke
    name: Smoke Duplicate
    provider: s3
    bucket: bucket
    workspace_prefix: prefix
    workdir_path: {}
    pipeline_path: {}
    database_path: {}
"#,
                workdir.display(),
                workdir.join("pipeline.yaml").display(),
                workdir.join("app.db").display(),
                workdir.display(),
                workdir.join("pipeline.yaml").display(),
                workdir.join("app.db").display()
            ),
        )
        .expect("registry");

        let error =
            load_registry_from_path(&registry_path).expect_err("duplicate id should be rejected");
        assert!(error.contains("Duplicate workspace id"));
    }
}
