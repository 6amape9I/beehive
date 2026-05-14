use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::config;
use crate::database;
use crate::domain::{
    CreateS3StagePayload, CreateS3StageRequest, PipelineConfig, ProjectConfig, RuntimeConfig,
    S3StageRouteHints, StageDefinition, StorageConfig, StorageProvider,
    UpdateStageNextStagePayload, UpdateStageNextStageRequest,
};
use crate::services::workspaces::{get_workspace, RegisteredWorkspace};
use crate::workdir::path_string;

pub(crate) fn create_s3_stage_for_workspace(
    workspace_id: &str,
    input: &CreateS3StageRequest,
) -> Result<CreateS3StagePayload, String> {
    let workspace = get_workspace(workspace_id)?;
    if workspace.provider != StorageProvider::S3 {
        return Err(format!(
            "Workspace '{}' is not an S3 workspace.",
            workspace.id
        ));
    }
    fs::create_dir_all(&workspace.workdir_path).map_err(|error| {
        format!(
            "Failed to create registered workspace workdir '{}': {error}",
            workspace.workdir_path.display()
        )
    })?;
    fs::create_dir_all(workspace.workdir_path.join("logs")).map_err(|error| {
        format!(
            "Failed to create registered workspace logs directory '{}': {error}",
            workspace.workdir_path.join("logs").display()
        )
    })?;

    let mut config = load_or_create_config(&workspace)?;
    let stage = build_s3_stage(&workspace, &config, input)?;
    reject_duplicate_active_stage(&workspace.database_path, &config, &stage.id)?;
    config.storage = Some(workspace_storage_config(&workspace)?);
    config.stages.push(stage.clone());

    let yaml_text = serde_yaml::to_string(&config)
        .map_err(|error| format!("Failed to serialize updated pipeline.yaml: {error}"))?;
    let reparsed = config::parse_pipeline_config(&yaml_text, Utc::now().to_rfc3339());
    if !reparsed.validation.is_valid {
        return Err(format!(
            "Generated S3 stage config failed validation: {:?}",
            reparsed.validation.issues
        ));
    }

    let backup_path = write_pipeline_yaml_atomic(&workspace.pipeline_path, &yaml_text)?;
    database::bootstrap_database(&workspace.database_path, &config)?;

    Ok(CreateS3StagePayload {
        route_hints: S3StageRouteHints {
            input_uri: stage.input_uri.clone().unwrap_or_default(),
            save_path_aliases: stage.save_path_aliases.clone(),
        },
        stage,
        backup_path: backup_path.map(|path| path_string(&path)),
    })
}

pub(crate) fn update_stage_next_stage_for_workspace(
    workspace_id: &str,
    stage_id: &str,
    input: &UpdateStageNextStageRequest,
) -> Result<UpdateStageNextStagePayload, String> {
    let workspace = get_workspace(workspace_id)?;
    let mut config = load_or_create_config(&workspace)?;
    let updated_stage = apply_next_stage_link(&mut config, stage_id, input)?;
    let yaml_text = serde_yaml::to_string(&config)
        .map_err(|error| format!("Failed to serialize updated pipeline.yaml: {error}"))?;
    let reparsed = config::parse_pipeline_config(&yaml_text, Utc::now().to_rfc3339());
    if !reparsed.validation.is_valid {
        return Err(format!(
            "Generated pipeline link config failed validation: {:?}",
            reparsed.validation.issues
        ));
    }
    let backup_path = write_pipeline_yaml_atomic(&workspace.pipeline_path, &yaml_text)?;
    database::bootstrap_database(&workspace.database_path, &config)?;

    Ok(UpdateStageNextStagePayload {
        stage: updated_stage,
        backup_path: backup_path.map(|path| path_string(&path)),
    })
}

fn apply_next_stage_link(
    config: &mut PipelineConfig,
    stage_id: &str,
    input: &UpdateStageNextStageRequest,
) -> Result<StageDefinition, String> {
    let source_stage_id = normalize_required(stage_id, "stage_id")?;
    let next_stage = normalize_optional(input.next_stage.clone());
    let source_index = config
        .stages
        .iter()
        .position(|stage| stage.id == source_stage_id)
        .ok_or_else(|| format!("Stage '{source_stage_id}' does not exist."))?;

    if let Some(next_stage) = next_stage.as_deref() {
        if next_stage == source_stage_id {
            return Err("next_stage cannot reference the same stage.".to_string());
        }
        if !config.stages.iter().any(|stage| stage.id == next_stage) {
            return Err(format!("Target stage '{next_stage}' does not exist."));
        }
    }

    config.stages[source_index].next_stage = next_stage;
    Ok(config.stages[source_index].clone())
}

fn load_or_create_config(workspace: &RegisteredWorkspace) -> Result<PipelineConfig, String> {
    if !workspace.pipeline_path.exists() {
        return Ok(PipelineConfig {
            project: ProjectConfig {
                name: workspace.name.clone(),
                workdir: ".".to_string(),
            },
            storage: Some(workspace_storage_config(workspace)?),
            runtime: RuntimeConfig::default(),
            stages: Vec::new(),
        });
    }

    let loaded = config::load_pipeline_config(&workspace.pipeline_path);
    if !loaded.validation.is_valid {
        return Err(format!(
            "Existing pipeline.yaml '{}' is invalid and cannot be updated from stage creation.",
            workspace.pipeline_path.display()
        ));
    }
    let mut config = loaded.config.ok_or_else(|| {
        format!(
            "Existing pipeline.yaml '{}' could not be converted into runtime config.",
            workspace.pipeline_path.display()
        )
    })?;

    match config.storage.as_ref() {
        Some(storage) if storage.provider == StorageProvider::S3 => {
            let workspace_storage = workspace_storage_config(workspace)?;
            if storage.bucket != workspace_storage.bucket
                || storage.workspace_prefix != workspace_storage.workspace_prefix
            {
                return Err(format!(
                    "pipeline.yaml S3 storage does not match workspace registry for '{}'.",
                    workspace.id
                ));
            }
        }
        Some(storage)
            if storage.provider == StorageProvider::Local && !config.stages.is_empty() =>
        {
            return Err(format!(
                "Workspace '{}' has a local pipeline with stages; refusing to convert it to S3 automatically.",
                workspace.id
            ));
        }
        _ => {}
    }
    config.storage = Some(workspace_storage_config(workspace)?);
    Ok(config)
}

fn build_s3_stage(
    workspace: &RegisteredWorkspace,
    config: &PipelineConfig,
    input: &CreateS3StageRequest,
) -> Result<StageDefinition, String> {
    let stage_id = normalize_required(&input.stage_id, "stage_id")?;
    if !is_safe_stage_id(&stage_id) {
        return Err(
            "stage_id may contain only letters, numbers, underscores, and hyphens.".to_string(),
        );
    }
    if config.stages.iter().any(|stage| stage.id == stage_id) {
        return Err(format!(
            "Stage id '{stage_id}' already exists in pipeline.yaml."
        ));
    }

    let workflow_url = normalize_required(&input.workflow_url, "workflow_url")?;
    if !workflow_url.starts_with("http://") && !workflow_url.starts_with("https://") {
        return Err("workflow_url must start with http:// or https://.".to_string());
    }

    let next_stage = normalize_optional(input.next_stage.clone());
    if let Some(next_stage) = next_stage.as_deref() {
        if next_stage == stage_id {
            return Err("next_stage cannot reference the same stage.".to_string());
        }
        let existing_ids = config
            .stages
            .iter()
            .map(|stage| stage.id.as_str())
            .collect::<HashSet<_>>();
        if !existing_ids.contains(next_stage) {
            return Err(format!(
                "next_stage '{next_stage}' does not reference an existing stage."
            ));
        }
    }

    let bucket = workspace
        .bucket
        .as_deref()
        .ok_or_else(|| format!("Workspace '{}' has no S3 bucket.", workspace.id))?;
    let workspace_prefix = normalize_route_prefix(
        workspace
            .workspace_prefix
            .as_deref()
            .ok_or_else(|| format!("Workspace '{}' has no workspace_prefix.", workspace.id))?,
    )?;
    let stage_prefix = format!("{workspace_prefix}/stages/{stage_id}");
    let input_uri = format!("s3://{bucket}/{stage_prefix}");
    let save_path_aliases = vec![
        stage_prefix.clone(),
        format!("/{stage_prefix}"),
        format!("s3://{bucket}/{stage_prefix}"),
    ];

    Ok(StageDefinition {
        id: stage_id.clone(),
        input_folder: format!("stages/{stage_id}"),
        input_uri: Some(input_uri),
        output_folder: format!("stages/{stage_id}_out"),
        workflow_url,
        max_attempts: input.max_attempts.unwrap_or(3).max(1),
        retry_delay_sec: input.retry_delay_sec.unwrap_or(30),
        next_stage,
        save_path_aliases,
        allow_empty_outputs: input.allow_empty_outputs.unwrap_or(false),
    })
}

fn reject_duplicate_active_stage(
    database_path: &Path,
    config: &PipelineConfig,
    stage_id: &str,
) -> Result<(), String> {
    if !database_path.exists() {
        return Ok(());
    }
    database::bootstrap_database(database_path, config)?;
    let stages = database::list_stages(database_path)?;
    if stages
        .iter()
        .any(|stage| stage.id == stage_id && stage.is_active)
    {
        return Err(format!(
            "Stage id '{stage_id}' already exists as an active SQLite stage."
        ));
    }
    Ok(())
}

fn workspace_storage_config(workspace: &RegisteredWorkspace) -> Result<StorageConfig, String> {
    Ok(StorageConfig {
        provider: StorageProvider::S3,
        bucket: Some(
            workspace
                .bucket
                .clone()
                .ok_or_else(|| format!("Workspace '{}' has no bucket.", workspace.id))?,
        ),
        workspace_prefix: Some(
            workspace
                .workspace_prefix
                .clone()
                .ok_or_else(|| format!("Workspace '{}' has no workspace_prefix.", workspace.id))?,
        ),
        region: workspace.region.clone(),
        endpoint: workspace.endpoint.clone(),
    })
}

fn write_pipeline_yaml_atomic(
    pipeline_path: &Path,
    yaml_text: &str,
) -> Result<Option<PathBuf>, String> {
    let parent = pipeline_path
        .parent()
        .ok_or_else(|| format!("Pipeline path '{}' has no parent.", pipeline_path.display()))?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "Failed to create pipeline directory '{}': {error}",
            parent.display()
        )
    })?;
    let now = Utc::now();
    let timestamp = now.format("%Y%m%dT%H%M%S%.3fZ");
    let unique_suffix = now
        .timestamp_nanos_opt()
        .unwrap_or_else(|| now.timestamp_micros() * 1000);
    let temp_path = parent.join(format!(".pipeline.yaml.tmp.{timestamp}.{unique_suffix}"));
    let backup_path = parent.join(format!("pipeline.yaml.bak.{timestamp}.{unique_suffix}"));

    {
        let mut file = File::create(&temp_path).map_err(|error| {
            format!(
                "Failed to create temp pipeline YAML '{}': {error}",
                temp_path.display()
            )
        })?;
        file.write_all(yaml_text.as_bytes()).map_err(|error| {
            format!(
                "Failed to write temp pipeline YAML '{}': {error}",
                temp_path.display()
            )
        })?;
        file.sync_all().map_err(|error| {
            format!(
                "Failed to sync temp pipeline YAML '{}': {error}",
                temp_path.display()
            )
        })?;
    }

    if pipeline_path.exists() {
        fs::rename(pipeline_path, &backup_path).map_err(|error| {
            let _ = fs::remove_file(&temp_path);
            format!(
                "Failed to move existing pipeline.yaml '{}' to backup '{}': {error}",
                pipeline_path.display(),
                backup_path.display()
            )
        })?;
        if let Err(error) = fs::rename(&temp_path, pipeline_path) {
            let restore_result = fs::rename(&backup_path, pipeline_path);
            let _ = fs::remove_file(&temp_path);
            return Err(match restore_result {
                Ok(()) => {
                    format!("Failed to install new pipeline.yaml; original was restored: {error}")
                }
                Err(restore_error) => format!(
                    "Failed to install new pipeline.yaml and failed to restore backup '{}': {error}; restore error: {restore_error}",
                    backup_path.display()
                ),
            });
        }
        Ok(Some(backup_path))
    } else {
        fs::rename(&temp_path, pipeline_path).map_err(|error| {
            let _ = fs::remove_file(&temp_path);
            format!(
                "Failed to install new pipeline.yaml '{}': {error}",
                pipeline_path.display()
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

fn is_safe_stage_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(tempdir: &tempfile::TempDir) -> RegisteredWorkspace {
        let workdir = tempdir.path().join("workspace");
        RegisteredWorkspace {
            id: "smoke".to_string(),
            name: "Smoke".to_string(),
            provider: StorageProvider::S3,
            bucket: Some("bucket".to_string()),
            workspace_prefix: Some("prefix/root".to_string()),
            region: Some("ru-1".to_string()),
            endpoint: Some("https://s3.example".to_string()),
            pipeline_path: workdir.join("pipeline.yaml"),
            database_path: workdir.join("app.db"),
            workdir_path: workdir,
        }
    }

    #[test]
    fn stage_creation_generates_s3_routes() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workspace = workspace(&tempdir);
        let config = PipelineConfig {
            project: ProjectConfig {
                name: "Smoke".to_string(),
                workdir: ".".to_string(),
            },
            storage: Some(workspace_storage_config(&workspace).expect("storage")),
            runtime: RuntimeConfig::default(),
            stages: Vec::new(),
        };
        let stage = build_s3_stage(
            &workspace,
            &config,
            &CreateS3StageRequest {
                stage_id: "semantic_rich".to_string(),
                workflow_url: "https://n8n.example/webhook/semantic".to_string(),
                next_stage: None,
                max_attempts: None,
                retry_delay_sec: None,
                allow_empty_outputs: None,
            },
        )
        .expect("stage");

        assert_eq!(
            stage.input_uri.as_deref(),
            Some("s3://bucket/prefix/root/stages/semantic_rich")
        );
        assert_eq!(
            stage.save_path_aliases,
            vec![
                "prefix/root/stages/semantic_rich".to_string(),
                "/prefix/root/stages/semantic_rich".to_string(),
                "s3://bucket/prefix/root/stages/semantic_rich".to_string(),
            ]
        );
    }

    #[test]
    fn stage_creation_rejects_bad_workflow_url() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workspace = workspace(&tempdir);
        let config = PipelineConfig {
            project: ProjectConfig {
                name: "Smoke".to_string(),
                workdir: ".".to_string(),
            },
            storage: Some(workspace_storage_config(&workspace).expect("storage")),
            runtime: RuntimeConfig::default(),
            stages: Vec::new(),
        };
        let error = build_s3_stage(
            &workspace,
            &config,
            &CreateS3StageRequest {
                stage_id: "semantic_rich".to_string(),
                workflow_url: "ftp://n8n.example/webhook/semantic".to_string(),
                next_stage: None,
                max_attempts: None,
                retry_delay_sec: None,
                allow_empty_outputs: None,
            },
        )
        .expect_err("bad workflow url should be rejected");

        assert!(error.contains("workflow_url"));
    }

    #[test]
    fn stage_creation_rejects_duplicate_stage_id() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workspace = workspace(&tempdir);
        let existing = StageDefinition {
            id: "semantic_rich".to_string(),
            input_folder: "stages/semantic_rich".to_string(),
            input_uri: Some("s3://bucket/prefix/root/stages/semantic_rich".to_string()),
            output_folder: "stages/semantic_rich_out".to_string(),
            workflow_url: "https://n8n.example/webhook/semantic".to_string(),
            max_attempts: 3,
            retry_delay_sec: 30,
            next_stage: None,
            save_path_aliases: Vec::new(),
            allow_empty_outputs: false,
        };
        let config = PipelineConfig {
            project: ProjectConfig {
                name: "Smoke".to_string(),
                workdir: ".".to_string(),
            },
            storage: Some(workspace_storage_config(&workspace).expect("storage")),
            runtime: RuntimeConfig::default(),
            stages: vec![existing],
        };
        let error = build_s3_stage(
            &workspace,
            &config,
            &CreateS3StageRequest {
                stage_id: "semantic_rich".to_string(),
                workflow_url: "https://n8n.example/webhook/semantic".to_string(),
                next_stage: None,
                max_attempts: None,
                retry_delay_sec: None,
                allow_empty_outputs: None,
            },
        )
        .expect_err("duplicate should be rejected");

        assert!(error.contains("already exists"));
    }

    #[test]
    fn stage_linking_updates_next_stage_and_can_clear_it() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workspace = workspace(&tempdir);
        let mut config = PipelineConfig {
            project: ProjectConfig {
                name: "Smoke".to_string(),
                workdir: ".".to_string(),
            },
            storage: Some(workspace_storage_config(&workspace).expect("storage")),
            runtime: RuntimeConfig::default(),
            stages: Vec::new(),
        };
        for stage_id in ["stage_a", "stage_b"] {
            let stage = build_s3_stage(
                &workspace,
                &config,
                &CreateS3StageRequest {
                    stage_id: stage_id.to_string(),
                    workflow_url: format!("https://n8n.example/webhook/{stage_id}"),
                    next_stage: None,
                    max_attempts: None,
                    retry_delay_sec: None,
                    allow_empty_outputs: None,
                },
            )
            .expect("stage");
            config.stages.push(stage);
        }

        let linked = apply_next_stage_link(
            &mut config,
            "stage_a",
            &UpdateStageNextStageRequest {
                next_stage: Some("stage_b".to_string()),
            },
        )
        .expect("link");
        assert_eq!(linked.next_stage.as_deref(), Some("stage_b"));

        let cleared = apply_next_stage_link(
            &mut config,
            "stage_a",
            &UpdateStageNextStageRequest { next_stage: None },
        )
        .expect("clear link");
        assert_eq!(cleared.next_stage, None);
    }
}
