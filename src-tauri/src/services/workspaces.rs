use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::config;
use crate::database;
use crate::domain::{
    CreateWorkspaceRequest, PipelineConfig, ProjectConfig, RuntimeConfig, StorageConfig,
    StorageProvider, UpdateWorkspaceRequest, WorkspaceDescriptor, WorkspaceMutationPayload,
};
use crate::workdir::path_string;

const DEFAULT_WORKSPACES_ROOT: &str = "/tmp/beehive-web-workspaces";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceRegistryFile {
    #[serde(default)]
    workspaces: Vec<WorkspaceRegistryRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(default)]
    is_archived: bool,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    archived_at: Option<String>,
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
    pub(crate) is_archived: bool,
    pub(crate) created_at: Option<String>,
    pub(crate) updated_at: Option<String>,
    pub(crate) archived_at: Option<String>,
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
            stage_count: load_pipeline_stage_count(&self.pipeline_path).unwrap_or(0),
            is_archived: self.is_archived,
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            archived_at: self.archived_at.clone(),
        }
    }
}

pub(crate) fn list_workspace_descriptors(
    include_archived: bool,
) -> Result<Vec<WorkspaceDescriptor>, String> {
    Ok(load_registry()?
        .into_iter()
        .filter(|workspace| include_archived || !workspace.is_archived)
        .map(|workspace| workspace.descriptor())
        .collect())
}

pub(crate) fn get_workspace_descriptor(workspace_id: &str) -> Result<WorkspaceDescriptor, String> {
    Ok(get_workspace(workspace_id)?.descriptor())
}

pub(crate) fn create_workspace(
    input: &CreateWorkspaceRequest,
) -> Result<WorkspaceMutationPayload, String> {
    let registry_path = registry_path();
    let mut registry = load_registry_file_from_path(&registry_path)?;
    let existing = validate_and_build_registry(registry.clone())?;
    let existing_ids = existing
        .iter()
        .map(|workspace| workspace.id.as_str())
        .collect::<HashSet<_>>();

    let name = normalize_required(&input.name, "name")?;
    let id = match input
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => normalize_workspace_id(id, "id")?,
        None => generate_workspace_id(&name, &existing_ids),
    };
    if existing_ids.contains(id.as_str()) {
        return Err(format!("Duplicate workspace id '{id}'."));
    }

    let bucket = normalize_required(&input.bucket, "bucket")?;
    let workspace_prefix = normalize_route_prefix(&input.workspace_prefix)?;
    let root = workspaces_root();
    let workdir_path = root.join(&id);
    let pipeline_path = workdir_path.join("pipeline.yaml");
    let database_path = workdir_path.join("app.db");
    let now = Utc::now().to_rfc3339();

    fs::create_dir_all(&workdir_path).map_err(|error| {
        format!(
            "Failed to create workspace directory '{}': {error}",
            workdir_path.display()
        )
    })?;
    fs::create_dir_all(workdir_path.join("logs")).map_err(|error| {
        format!(
            "Failed to create workspace logs directory '{}': {error}",
            workdir_path.join("logs").display()
        )
    })?;

    let record = WorkspaceRegistryRecord {
        id,
        name,
        provider: StorageProvider::S3,
        bucket: Some(bucket),
        workspace_prefix: Some(workspace_prefix),
        region: normalize_optional(input.region.clone()),
        endpoint: normalize_optional(input.endpoint.clone()),
        workdir_path: path_string(&workdir_path),
        pipeline_path: path_string(&pipeline_path),
        database_path: path_string(&database_path),
        is_archived: false,
        created_at: Some(now.clone()),
        updated_at: Some(now),
        archived_at: None,
    };
    let workspace = record_to_workspace(record.clone(), "new workspace")?;
    let config = empty_workspace_config(&workspace)?;
    write_pipeline_yaml_atomic(&workspace.pipeline_path, &serialize_pipeline_yaml(&config)?)?;
    database::bootstrap_database(&workspace.database_path, &config)?;

    registry.workspaces.push(record);
    let backup_path = write_registry_file_atomic(&registry_path, &registry)?;
    let workspace = get_workspace(&workspace.id)?;
    Ok(WorkspaceMutationPayload {
        workspace: Some(workspace.descriptor()),
        hard_deleted: false,
        archived: false,
        backup_path: backup_path.map(|path| path_string(&path)),
    })
}

pub(crate) fn update_workspace(
    workspace_id: &str,
    input: &UpdateWorkspaceRequest,
) -> Result<WorkspaceMutationPayload, String> {
    let registry_path = registry_path();
    let mut registry = load_registry_file_from_path(&registry_path)?;
    let index = find_registry_index(&registry, workspace_id)?;
    let before = record_to_workspace(registry.workspaces[index].clone(), "workspace")?;

    let next_bucket = match input.bucket.as_ref() {
        Some(bucket) => Some(normalize_required(bucket, "bucket")?),
        None => before.bucket.clone(),
    };
    let next_prefix = match input.workspace_prefix.as_ref() {
        Some(prefix) => Some(normalize_route_prefix(prefix)?),
        None => before.workspace_prefix.clone(),
    };
    let storage_changes = next_bucket != before.bucket || next_prefix != before.workspace_prefix;
    if storage_changes && !workspace_is_empty(&before)? {
        return Err(
            "Нельзя изменить bucket/prefix: в workspace уже есть зарегистрированные artifacts или история запусков."
                .to_string(),
        );
    }

    if let Some(name) = input.name.as_ref() {
        registry.workspaces[index].name = normalize_required(name, "name")?;
    }
    if input.bucket.is_some() {
        registry.workspaces[index].bucket = next_bucket;
    }
    if input.workspace_prefix.is_some() {
        registry.workspaces[index].workspace_prefix = next_prefix;
    }
    if input.region.is_some() {
        registry.workspaces[index].region = normalize_optional(input.region.clone());
    }
    if input.endpoint.is_some() {
        registry.workspaces[index].endpoint = normalize_optional(input.endpoint.clone());
    }
    registry.workspaces[index].updated_at = Some(Utc::now().to_rfc3339());

    if storage_changes {
        let updated = record_to_workspace(registry.workspaces[index].clone(), "workspace")?;
        let config = empty_workspace_config(&updated)?;
        write_pipeline_yaml_atomic(&updated.pipeline_path, &serialize_pipeline_yaml(&config)?)?;
        database::bootstrap_database(&updated.database_path, &config)?;
    }

    let backup_path = write_registry_file_atomic(&registry_path, &registry)?;
    let workspace = get_workspace(workspace_id)?;
    Ok(WorkspaceMutationPayload {
        workspace: Some(workspace.descriptor()),
        hard_deleted: false,
        archived: workspace.is_archived,
        backup_path: backup_path.map(|path| path_string(&path)),
    })
}

pub(crate) fn archive_or_delete_workspace(
    workspace_id: &str,
) -> Result<WorkspaceMutationPayload, String> {
    let registry_path = registry_path();
    let mut registry = load_registry_file_from_path(&registry_path)?;
    let index = find_registry_index(&registry, workspace_id)?;
    let workspace = record_to_workspace(registry.workspaces[index].clone(), "workspace")?;

    if workspace_is_empty(&workspace)? {
        registry.workspaces.remove(index);
        let backup_path = write_registry_file_atomic(&registry_path, &registry)?;
        return Ok(WorkspaceMutationPayload {
            workspace: None,
            hard_deleted: true,
            archived: false,
            backup_path: backup_path.map(|path| path_string(&path)),
        });
    }

    let now = Utc::now().to_rfc3339();
    registry.workspaces[index].is_archived = true;
    registry.workspaces[index].archived_at = Some(now.clone());
    registry.workspaces[index].updated_at = Some(now);
    let backup_path = write_registry_file_atomic(&registry_path, &registry)?;
    let workspace = get_workspace(workspace_id)?;
    Ok(WorkspaceMutationPayload {
        workspace: Some(workspace.descriptor()),
        hard_deleted: false,
        archived: true,
        backup_path: backup_path.map(|path| path_string(&path)),
    })
}

pub(crate) fn restore_workspace(workspace_id: &str) -> Result<WorkspaceMutationPayload, String> {
    let registry_path = registry_path();
    let mut registry = load_registry_file_from_path(&registry_path)?;
    let index = find_registry_index(&registry, workspace_id)?;
    let now = Utc::now().to_rfc3339();
    registry.workspaces[index].is_archived = false;
    registry.workspaces[index].archived_at = None;
    registry.workspaces[index].updated_at = Some(now);
    let backup_path = write_registry_file_atomic(&registry_path, &registry)?;
    let workspace = get_workspace(workspace_id)?;
    Ok(WorkspaceMutationPayload {
        workspace: Some(workspace.descriptor()),
        hard_deleted: false,
        archived: false,
        backup_path: backup_path.map(|path| path_string(&path)),
    })
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

fn workspaces_root() -> PathBuf {
    std::env::var_os("BEEHIVE_WORKSPACES_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_WORKSPACES_ROOT))
}

fn load_registry() -> Result<Vec<RegisteredWorkspace>, String> {
    load_registry_from_path(&registry_path())
}

pub(crate) fn load_registry_from_path(path: &Path) -> Result<Vec<RegisteredWorkspace>, String> {
    validate_and_build_registry(load_registry_file_from_path(path)?)
}

fn load_registry_file_from_path(path: &Path) -> Result<WorkspaceRegistryFile, String> {
    if !path.exists() {
        return Ok(WorkspaceRegistryFile {
            workspaces: Vec::new(),
        });
    }
    let content = fs::read_to_string(path).map_err(|error| {
        format!(
            "Failed to read workspace registry '{}': {error}",
            path.display()
        )
    })?;
    serde_yaml::from_str(&content).map_err(|error| {
        format!(
            "Failed to parse workspace registry '{}': {error}",
            path.display()
        )
    })
}

fn validate_and_build_registry(
    registry: WorkspaceRegistryFile,
) -> Result<Vec<RegisteredWorkspace>, String> {
    let mut seen_ids = HashSet::new();
    let mut workspaces = Vec::new();

    for (index, record) in registry.workspaces.into_iter().enumerate() {
        let prefix = format!("workspaces[{index}]");
        let workspace = record_to_workspace(record, &prefix)?;
        if !seen_ids.insert(workspace.id.clone()) {
            return Err(format!("Duplicate workspace id '{}'.", workspace.id));
        }
        workspaces.push(workspace);
    }

    Ok(workspaces)
}

fn record_to_workspace(
    record: WorkspaceRegistryRecord,
    prefix: &str,
) -> Result<RegisteredWorkspace, String> {
    let id = normalize_workspace_id(&record.id, &format!("{prefix}.id"))?;
    let name = normalize_required(&record.name, &format!("{prefix}.name"))?;
    let workdir_path = absolute_path(&record.workdir_path, &format!("{prefix}.workdir_path"))?;
    let pipeline_path = absolute_path(&record.pipeline_path, &format!("{prefix}.pipeline_path"))?;
    let database_path = absolute_path(&record.database_path, &format!("{prefix}.database_path"))?;

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

    Ok(RegisteredWorkspace {
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
        is_archived: record.is_archived,
        created_at: record.created_at,
        updated_at: record.updated_at,
        archived_at: record.archived_at,
    })
}

fn find_registry_index(
    registry: &WorkspaceRegistryFile,
    workspace_id: &str,
) -> Result<usize, String> {
    let workspace_id = workspace_id.trim();
    if workspace_id.is_empty() {
        return Err("workspace_id is required.".to_string());
    }
    registry
        .workspaces
        .iter()
        .position(|workspace| workspace.id == workspace_id)
        .ok_or_else(|| format!("Unknown workspace_id '{workspace_id}'."))
}

fn empty_workspace_config(workspace: &RegisteredWorkspace) -> Result<PipelineConfig, String> {
    Ok(PipelineConfig {
        project: ProjectConfig {
            name: workspace.name.clone(),
            workdir: ".".to_string(),
        },
        storage: Some(StorageConfig {
            provider: StorageProvider::S3,
            bucket: Some(
                workspace
                    .bucket
                    .clone()
                    .ok_or_else(|| format!("Workspace '{}' has no bucket.", workspace.id))?,
            ),
            workspace_prefix: Some(
                workspace.workspace_prefix.clone().ok_or_else(|| {
                    format!("Workspace '{}' has no workspace_prefix.", workspace.id)
                })?,
            ),
            region: workspace.region.clone(),
            endpoint: workspace.endpoint.clone(),
        }),
        runtime: RuntimeConfig::default(),
        stages: Vec::new(),
    })
}

fn workspace_is_empty(workspace: &RegisteredWorkspace) -> Result<bool, String> {
    Ok(load_pipeline_stage_count(&workspace.pipeline_path)? == 0
        && database_table_count(&workspace.database_path, "stages")? == 0
        && database_table_count(&workspace.database_path, "entity_files")? == 0
        && database_table_count(&workspace.database_path, "stage_runs")? == 0)
}

fn load_pipeline_stage_count(path: &Path) -> Result<u64, String> {
    if !path.exists() {
        return Ok(0);
    }
    let loaded = config::load_pipeline_config(path);
    if !loaded.validation.is_valid {
        return Err(format!(
            "pipeline.yaml '{}' is invalid; refusing workspace CRUD operation.",
            path.display()
        ));
    }
    Ok(loaded
        .config
        .map(|config| config.stages.len() as u64)
        .unwrap_or(0))
}

fn database_table_count(database_path: &Path, table_name: &str) -> Result<u64, String> {
    if !database_path.exists() {
        return Ok(0);
    }
    let connection = Connection::open(database_path).map_err(|error| {
        format!(
            "Failed to open SQLite database '{}': {error}",
            database_path.display()
        )
    })?;
    let exists = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![table_name],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| format!("Failed to inspect SQLite table '{table_name}': {error}"))?;
    if exists.is_none() {
        return Ok(0);
    }
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table_name}"), [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|value| value as u64)
        .map_err(|error| format!("Failed to count SQLite table '{table_name}': {error}"))
}

fn serialize_pipeline_yaml(config: &PipelineConfig) -> Result<String, String> {
    let yaml_text = serde_yaml::to_string(config)
        .map_err(|error| format!("Failed to serialize pipeline.yaml: {error}"))?;
    let reparsed = config::parse_pipeline_config(&yaml_text, Utc::now().to_rfc3339());
    if !reparsed.validation.is_valid {
        return Err(format!(
            "Generated pipeline.yaml failed validation: {:?}",
            reparsed.validation.issues
        ));
    }
    Ok(yaml_text)
}

fn write_registry_file_atomic(
    registry_path: &Path,
    registry: &WorkspaceRegistryFile,
) -> Result<Option<PathBuf>, String> {
    let yaml_text = serde_yaml::to_string(registry)
        .map_err(|error| format!("Failed to serialize workspaces.yaml: {error}"))?;
    write_text_file_atomic(registry_path, "workspaces.yaml", &yaml_text)
}

fn write_pipeline_yaml_atomic(
    pipeline_path: &Path,
    yaml_text: &str,
) -> Result<Option<PathBuf>, String> {
    write_text_file_atomic(pipeline_path, "pipeline.yaml", yaml_text)
}

fn write_text_file_atomic(
    target_path: &Path,
    label: &str,
    content: &str,
) -> Result<Option<PathBuf>, String> {
    let parent = target_path
        .parent()
        .ok_or_else(|| format!("{} path '{}' has no parent.", label, target_path.display()))?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "Failed to create parent directory '{}': {error}",
            parent.display()
        )
    })?;
    let now = Utc::now();
    let timestamp = now.format("%Y%m%dT%H%M%S%.3fZ");
    let unique_suffix = now
        .timestamp_nanos_opt()
        .unwrap_or_else(|| now.timestamp_micros() * 1000);
    let temp_path = parent.join(format!(".{label}.tmp.{timestamp}.{unique_suffix}"));
    let backup_path = parent.join(format!("{label}.bak.{timestamp}.{unique_suffix}"));

    {
        let mut file = File::create(&temp_path).map_err(|error| {
            format!(
                "Failed to create temp {} '{}': {error}",
                label,
                temp_path.display()
            )
        })?;
        file.write_all(content.as_bytes()).map_err(|error| {
            format!(
                "Failed to write temp {} '{}': {error}",
                label,
                temp_path.display()
            )
        })?;
        file.sync_all().map_err(|error| {
            format!(
                "Failed to sync temp {} '{}': {error}",
                label,
                temp_path.display()
            )
        })?;
    }

    if target_path.exists() {
        fs::rename(target_path, &backup_path).map_err(|error| {
            let _ = fs::remove_file(&temp_path);
            format!(
                "Failed to move existing {} '{}' to backup '{}': {error}",
                label,
                target_path.display(),
                backup_path.display()
            )
        })?;
        if let Err(error) = fs::rename(&temp_path, target_path) {
            let restore_result = fs::rename(&backup_path, target_path);
            let _ = fs::remove_file(&temp_path);
            return Err(match restore_result {
                Ok(()) => format!("Failed to install new {label}; original was restored: {error}"),
                Err(restore_error) => format!(
                    "Failed to install new {label} and failed to restore backup '{}': {error}; restore error: {restore_error}",
                    backup_path.display()
                ),
            });
        }
        Ok(Some(backup_path))
    } else {
        fs::rename(&temp_path, target_path).map_err(|error| {
            let _ = fs::remove_file(&temp_path);
            format!(
                "Failed to install new {} '{}': {error}",
                label,
                target_path.display()
            )
        })?;
        Ok(None)
    }
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

fn normalize_workspace_id(value: &str, path: &str) -> Result<String, String> {
    let value = normalize_required(value, path)?;
    if !is_safe_workspace_id(&value) {
        return Err(format!(
            "{path} must contain only letters, numbers, underscores, and hyphens."
        ));
    }
    Ok(value)
}

fn absolute_path(value: &str, path: &str) -> Result<PathBuf, String> {
    let normalized = normalize_required(value, path)?;
    let path_buf = PathBuf::from(&normalized);
    if !path_buf.is_absolute() {
        return Err(format!("{path} must be an absolute server-side path."));
    }
    Ok(path_buf)
}

fn normalize_route_prefix(value: &str) -> Result<String, String> {
    let normalized = value.trim().trim_matches('/').replace('\\', "/");
    if normalized.is_empty() {
        return Err("workspace_prefix cannot be empty.".to_string());
    }
    if normalized.contains(':') || normalized.starts_with("//") {
        return Err("workspace_prefix must be a logical S3 prefix, not an OS path.".to_string());
    }
    let mut parts = Vec::new();
    for component in normalized.split('/') {
        match component {
            "" | "." => {}
            ".." => return Err("workspace_prefix must not contain '..'.".to_string()),
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        Err("workspace_prefix cannot be empty.".to_string())
    } else {
        Ok(parts.join("/"))
    }
}

fn generate_workspace_id(name: &str, existing_ids: &HashSet<&str>) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for ch in name.trim().chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if ch == '_' {
            Some('_')
        } else if ch.is_whitespace() || ch == '-' {
            Some('-')
        } else {
            None
        };
        if let Some(ch) = normalized {
            if ch == '-' {
                if previous_dash {
                    continue;
                }
                previous_dash = true;
            } else {
                previous_dash = false;
            }
            slug.push(ch);
        }
    }
    let slug = slug.trim_matches('-').to_string();
    let base = if slug.is_empty() {
        "workspace".to_string()
    } else {
        slug
    };
    if !existing_ids.contains(base.as_str()) {
        return base;
    }
    for index in 2.. {
        let candidate = format!("{base}-{index}");
        if !existing_ids.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!()
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
    use crate::domain::CreateS3StageRequest;
    use crate::services::pipeline;
    use std::ffi::OsStr;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_test_env<F>(registry_path: &Path, root: &Path, run: F)
    where
        F: FnOnce(),
    {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_registry = std::env::var_os("BEEHIVE_WORKSPACES_CONFIG");
        let previous_root = std::env::var_os("BEEHIVE_WORKSPACES_ROOT");
        std::env::set_var("BEEHIVE_WORKSPACES_CONFIG", registry_path);
        std::env::set_var("BEEHIVE_WORKSPACES_ROOT", root);
        run();
        restore_env_var("BEEHIVE_WORKSPACES_CONFIG", previous_registry.as_deref());
        restore_env_var("BEEHIVE_WORKSPACES_ROOT", previous_root.as_deref());
    }

    fn restore_env_var(name: &str, value: Option<&OsStr>) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }

    fn registry_text(workdir: &Path) -> String {
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
        )
    }

    #[test]
    fn workspace_registry_loads_valid_config_without_exposing_paths() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("smoke");
        let registry_path = tempdir.path().join("workspaces.yaml");
        fs::write(&registry_path, registry_text(&workdir)).expect("registry");

        let workspaces = load_registry_from_path(&registry_path).expect("load registry");
        assert_eq!(workspaces.len(), 1);
        let descriptor = workspaces[0].descriptor();
        assert_eq!(descriptor.id, "smoke");
        assert_eq!(descriptor.bucket.as_deref(), Some("steos-s3-data"));
        assert!(!descriptor.is_archived);
        assert_eq!(descriptor.stage_count, 0);
    }

    #[test]
    fn old_registry_entries_without_created_at_still_load() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("smoke");
        let registry_path = tempdir.path().join("workspaces.yaml");
        fs::write(&registry_path, registry_text(&workdir)).expect("registry");

        let workspaces = load_registry_from_path(&registry_path).expect("load old registry");
        assert_eq!(workspaces[0].created_at, None);
        assert_eq!(workspaces[0].archived_at, None);
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

    #[test]
    fn create_workspace_writes_registry_and_initializes_files() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let registry_path = tempdir.path().join("workspaces.yaml");
        let root = tempdir.path().join("root");
        with_test_env(&registry_path, &root, || {
            let payload = create_workspace(&CreateWorkspaceRequest {
                id: Some("pilot".to_string()),
                name: "Pilot".to_string(),
                bucket: "bucket".to_string(),
                workspace_prefix: "prefix/root".to_string(),
                region: Some("ru-1".to_string()),
                endpoint: Some("https://s3.example".to_string()),
            })
            .expect("create workspace");
            let descriptor = payload.workspace.expect("workspace");
            assert_eq!(descriptor.id, "pilot");
            assert_eq!(descriptor.stage_count, 0);
            assert!(root.join("pilot").join("pipeline.yaml").exists());
            assert!(root.join("pilot").join("app.db").exists());
            let loaded = load_registry_from_path(&registry_path).expect("load registry");
            assert_eq!(loaded.len(), 1);
        });
    }

    #[test]
    fn create_workspace_rejects_duplicate_id() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let registry_path = tempdir.path().join("workspaces.yaml");
        let root = tempdir.path().join("root");
        with_test_env(&registry_path, &root, || {
            let input = CreateWorkspaceRequest {
                id: Some("pilot".to_string()),
                name: "Pilot".to_string(),
                bucket: "bucket".to_string(),
                workspace_prefix: "prefix/root".to_string(),
                region: None,
                endpoint: None,
            };
            create_workspace(&input).expect("create first");
            let error = create_workspace(&input).expect_err("duplicate id");
            assert!(error.contains("Duplicate workspace id"));
        });
    }

    #[test]
    fn update_workspace_rejects_dangerous_bucket_prefix_change_when_history_exists() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let registry_path = tempdir.path().join("workspaces.yaml");
        let root = tempdir.path().join("root");
        with_test_env(&registry_path, &root, || {
            create_workspace(&CreateWorkspaceRequest {
                id: Some("pilot".to_string()),
                name: "Pilot".to_string(),
                bucket: "bucket".to_string(),
                workspace_prefix: "prefix/root".to_string(),
                region: None,
                endpoint: None,
            })
            .expect("create");
            pipeline::create_s3_stage_for_workspace(
                "pilot",
                &CreateS3StageRequest {
                    stage_id: "stage_1".to_string(),
                    workflow_url: "https://n8n.example/webhook/stage-1".to_string(),
                    next_stage: None,
                    max_attempts: None,
                    retry_delay_sec: None,
                    allow_empty_outputs: None,
                },
            )
            .expect("create stage");

            let error = update_workspace(
                "pilot",
                &UpdateWorkspaceRequest {
                    name: None,
                    bucket: Some("other".to_string()),
                    workspace_prefix: None,
                    region: None,
                    endpoint: None,
                },
            )
            .expect_err("dangerous update");
            assert!(error.contains("Нельзя изменить bucket/prefix"));
        });
    }

    #[test]
    fn delete_archives_workspace_with_history_and_restore_works() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let registry_path = tempdir.path().join("workspaces.yaml");
        let root = tempdir.path().join("root");
        with_test_env(&registry_path, &root, || {
            create_workspace(&CreateWorkspaceRequest {
                id: Some("pilot".to_string()),
                name: "Pilot".to_string(),
                bucket: "bucket".to_string(),
                workspace_prefix: "prefix/root".to_string(),
                region: None,
                endpoint: None,
            })
            .expect("create");
            pipeline::create_s3_stage_for_workspace(
                "pilot",
                &CreateS3StageRequest {
                    stage_id: "stage_1".to_string(),
                    workflow_url: "https://n8n.example/webhook/stage-1".to_string(),
                    next_stage: None,
                    max_attempts: None,
                    retry_delay_sec: None,
                    allow_empty_outputs: None,
                },
            )
            .expect("create stage");

            let archived = archive_or_delete_workspace("pilot").expect("archive");
            assert!(archived.archived);
            assert!(!archived.hard_deleted);
            assert_eq!(list_workspace_descriptors(false).expect("list").len(), 0);
            assert_eq!(list_workspace_descriptors(true).expect("list").len(), 1);

            let restored = restore_workspace("pilot").expect("restore");
            assert!(!restored.archived);
            let descriptor = restored.workspace.expect("workspace");
            assert!(!descriptor.is_archived);
        });
    }
}
