use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use chrono::Utc;
use rusqlite::Connection;
use serde_json::json;

use crate::config;
use crate::database;
use crate::discovery;
use crate::domain::{
    AppEventLevel, CommandErrorInfo, ConfigValidationIssue, ConfigValidationResult, PipelineConfig,
    PipelineConfigDraft, PipelineEditorState, ProjectConfig, ProjectConfigDraft, RuntimeConfig,
    RuntimeConfigDraft, SavePipelineConfigResult, StageDefinition, StageDefinitionDraft,
    StageUsageSummary, StorageProvider, ValidatePipelineConfigDraftResult, ValidationSeverity,
};
use crate::workdir::{self, path_string};

struct EditorPaths {
    workdir_path: PathBuf,
    database_path: PathBuf,
    pipeline_path: PathBuf,
}

struct DraftValidation {
    validation: ConfigValidationResult,
    config: Option<PipelineConfig>,
    yaml_preview: Option<String>,
    stage_usages: Vec<StageUsageSummary>,
}

pub fn get_pipeline_editor_state(path: &str) -> Result<PipelineEditorState, String> {
    let paths = editor_paths(path)?;
    let yaml_text = fs::read_to_string(&paths.pipeline_path).map_err(|error| {
        format!(
            "Failed to read pipeline config '{}': {error}",
            paths.pipeline_path.display()
        )
    })?;
    let loaded = config::parse_pipeline_config(&yaml_text, Utc::now().to_rfc3339());
    let stage_usages = load_stage_usages(&paths.database_path)?;
    let draft = loaded.config.as_ref().map(draft_from_config);
    let yaml_preview = loaded
        .config
        .as_ref()
        .and_then(|config| serialize_pipeline_yaml(config).ok())
        .unwrap_or_else(|| yaml_text.clone());

    Ok(PipelineEditorState {
        config: loaded.config,
        draft,
        yaml_text,
        yaml_preview,
        validation: loaded.validation,
        stage_usages,
        loaded_at: loaded.loaded_at,
    })
}

pub fn validate_pipeline_config_draft(
    path: &str,
    draft: &PipelineConfigDraft,
) -> Result<ValidatePipelineConfigDraftResult, String> {
    let paths = editor_paths(path)?;
    let result = validate_draft(&paths, draft)?;
    Ok(ValidatePipelineConfigDraftResult {
        validation: result.validation,
        normalized_config: result.config,
        yaml_preview: result.yaml_preview,
        stage_usages: result.stage_usages,
        errors: Vec::new(),
    })
}

pub fn save_pipeline_config(
    path: &str,
    draft: &PipelineConfigDraft,
    operator_comment: Option<&str>,
) -> Result<SavePipelineConfigResult, String> {
    let paths = editor_paths(path)?;
    let validation_result = validate_draft(&paths, draft)?;
    if !validation_result.validation.is_valid {
        return Ok(SavePipelineConfigResult {
            state: None,
            backup_path: None,
            errors: vec![command_error(
                "pipeline_config_invalid",
                "Draft pipeline config has validation errors and was not saved.",
                Some(path_string(&paths.pipeline_path)),
            )],
        });
    }
    let config = validation_result.config.ok_or_else(|| {
        "Draft validation succeeded but no normalized config was produced.".to_string()
    })?;
    let yaml_text = validation_result.yaml_preview.ok_or_else(|| {
        "Draft validation succeeded but no YAML preview was produced.".to_string()
    })?;

    let before = config::load_pipeline_config(&paths.pipeline_path)
        .config
        .unwrap_or_else(|| PipelineConfig {
            project: config.project.clone(),
            storage: config.storage.clone(),
            runtime: config.runtime.clone(),
            stages: Vec::new(),
        });
    let backup_path = write_pipeline_yaml_atomic(&paths.pipeline_path, &yaml_text)?;

    database::bootstrap_database(&paths.database_path, &config)?;
    let provision_summary =
        discovery::ensure_stage_directories(&paths.workdir_path, &paths.database_path)?;

    let change_set = stage_change_set(&before, &config);
    let now = Utc::now().to_rfc3339();
    let connection = database::open_connection(&paths.database_path)?;
    database::insert_app_event(
        &connection,
        AppEventLevel::Info,
        "pipeline_config_saved",
        "Pipeline configuration was saved from Stage Editor.",
        Some(json!({
            "stage_count": config.stages.len(),
            "added_stage_ids": change_set.added,
            "removed_stage_ids": change_set.removed,
            "updated_stage_ids": change_set.updated,
            "operator_comment": operator_comment,
            "backup_path": path_string(&backup_path),
            "created_directory_count": provision_summary.created_directory_count,
            "created_paths": provision_summary.created_paths,
        })),
        &now,
    )?;

    Ok(SavePipelineConfigResult {
        state: Some(get_pipeline_editor_state(path)?),
        backup_path: Some(path_string(&backup_path)),
        errors: Vec::new(),
    })
}

fn editor_paths(path: &str) -> Result<EditorPaths, String> {
    let workdir_path = workdir::resolve_user_path(path)?;
    let state = workdir::inspect(&workdir_path, false);
    if !state.exists {
        return Err(format!(
            "The selected workdir '{}' does not exist.",
            state.workdir_path
        ));
    }
    if !state.pipeline_config_exists {
        return Err(format!(
            "pipeline.yaml is required to edit this workdir: {}",
            state.pipeline_config_path
        ));
    }
    Ok(EditorPaths {
        workdir_path,
        database_path: PathBuf::from(state.database_path),
        pipeline_path: PathBuf::from(state.pipeline_config_path),
    })
}

fn validate_draft(
    paths: &EditorPaths,
    draft: &PipelineConfigDraft,
) -> Result<DraftValidation, String> {
    let stage_usages = load_stage_usages(&paths.database_path)?;
    let mut issues = Vec::new();

    let project_name = required_string(
        &draft.project.name,
        "missing_project_name",
        "project.name",
        &mut issues,
    );
    let project_workdir = required_string(
        &draft.project.workdir,
        "missing_project_workdir",
        "project.workdir",
        &mut issues,
    );
    if project_workdir.as_deref() != Some(".") {
        issues.push(issue(
            ValidationSeverity::Warning,
            "project_workdir_selected_workdir_warning",
            "project.workdir",
            "Runtime uses the selected workdir path; project.workdir is kept for config compatibility.",
        ));
    }

    validate_min_i64(
        draft.runtime.scan_interval_sec,
        1,
        "invalid_runtime_scan_interval_sec",
        "runtime.scan_interval_sec",
        "scan_interval_sec must be greater than or equal to 1.",
        &mut issues,
    );
    validate_min_i64(
        draft.runtime.max_parallel_tasks,
        1,
        "invalid_runtime_max_parallel_tasks",
        "runtime.max_parallel_tasks",
        "max_parallel_tasks must be greater than or equal to 1.",
        &mut issues,
    );
    validate_min_i64(
        draft.runtime.stuck_task_timeout_sec,
        1,
        "invalid_runtime_stuck_task_timeout_sec",
        "runtime.stuck_task_timeout_sec",
        "stuck_task_timeout_sec must be greater than or equal to 1.",
        &mut issues,
    );
    validate_min_i64(
        draft.runtime.request_timeout_sec,
        1,
        "invalid_runtime_request_timeout_sec",
        "runtime.request_timeout_sec",
        "request_timeout_sec must be greater than or equal to 1.",
        &mut issues,
    );
    validate_min_i64(
        draft.runtime.file_stability_delay_ms,
        0,
        "invalid_runtime_file_stability_delay_ms",
        "runtime.file_stability_delay_ms",
        "file_stability_delay_ms must be greater than or equal to 0.",
        &mut issues,
    );

    if draft.stages.is_empty() {
        issues.push(issue(
            ValidationSeverity::Error,
            "missing_stages",
            "stages",
            "At least one stage is required.",
        ));
    }

    validate_stage_drafts(paths, draft, &stage_usages, &mut issues);
    add_removed_stage_warnings(draft, &stage_usages, &mut issues);

    let validation = ConfigValidationResult::from_issues(issues);
    if !validation.is_valid {
        return Ok(DraftValidation {
            validation,
            config: None,
            yaml_preview: None,
            stage_usages,
        });
    }

    let config = PipelineConfig {
        project: ProjectConfig {
            name: project_name.unwrap_or_default(),
            workdir: project_workdir.unwrap_or_else(|| ".".to_string()),
        },
        storage: draft.storage.clone(),
        runtime: RuntimeConfig {
            scan_interval_sec: draft.runtime.scan_interval_sec as u64,
            max_parallel_tasks: draft.runtime.max_parallel_tasks as u64,
            stuck_task_timeout_sec: draft.runtime.stuck_task_timeout_sec as u64,
            request_timeout_sec: draft.runtime.request_timeout_sec as u64,
            file_stability_delay_ms: draft.runtime.file_stability_delay_ms as u64,
        },
        stages: draft
            .stages
            .iter()
            .map(|stage| StageDefinition {
                id: stage.id.trim().to_string(),
                input_folder: stage.input_folder.trim().to_string(),
                input_uri: stage.input_uri.clone(),
                output_folder: if normalize_optional(&stage.next_stage).is_some() {
                    stage.output_folder.trim().to_string()
                } else {
                    normalize_optional_str(&stage.output_folder).unwrap_or_default()
                },
                workflow_url: stage.workflow_url.trim().to_string(),
                max_attempts: stage.max_attempts as u64,
                retry_delay_sec: stage.retry_delay_sec as u64,
                next_stage: normalize_optional(&stage.next_stage),
                save_path_aliases: stage.save_path_aliases.clone(),
                allow_empty_outputs: stage.allow_empty_outputs,
            })
            .collect(),
    };
    let yaml_preview = serialize_pipeline_yaml(&config)?;
    let reparsed = config::parse_pipeline_config(&yaml_preview, Utc::now().to_rfc3339());
    if !reparsed.validation.is_valid {
        return Ok(DraftValidation {
            validation: reparsed.validation,
            config: None,
            yaml_preview: Some(yaml_preview),
            stage_usages,
        });
    }

    Ok(DraftValidation {
        validation,
        config: Some(config),
        yaml_preview: Some(yaml_preview),
        stage_usages,
    })
}

fn validate_stage_drafts(
    paths: &EditorPaths,
    draft: &PipelineConfigDraft,
    usages: &[StageUsageSummary],
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let is_s3_mode = draft
        .storage
        .as_ref()
        .is_some_and(|storage| storage.provider == StorageProvider::S3);
    let usage_ids = usages
        .iter()
        .map(|usage| usage.stage_id.as_str())
        .collect::<HashSet<_>>();
    let mut stage_ids = HashSet::<String>::new();
    for (index, stage) in draft.stages.iter().enumerate() {
        let prefix = format!("stages[{index}]");
        let id = stage.id.trim();
        if id.is_empty() {
            issues.push(issue(
                ValidationSeverity::Error,
                "missing_stage_id",
                format!("{prefix}.id"),
                "Stage id is required.",
            ));
        } else if !is_safe_stage_id(id) {
            issues.push(issue(
                ValidationSeverity::Error,
                "invalid_stage_id",
                format!("{prefix}.id"),
                "Stage id may contain only letters, numbers, underscores, and hyphens.",
            ));
        } else if !stage_ids.insert(id.to_string()) {
            issues.push(issue(
                ValidationSeverity::Error,
                "duplicate_stage_id",
                format!("{prefix}.id"),
                format!("Stage id '{id}' is declared more than once."),
            ));
        }

        if !stage.is_new {
            match stage.original_stage_id.as_deref() {
                Some(original) if original == id => {}
                Some(original) => issues.push(issue(
                    ValidationSeverity::Error,
                    "saved_stage_id_rename_forbidden",
                    format!("{prefix}.id"),
                    format!(
                        "Saved stage id '{}' cannot be renamed to '{}'. Create a new stage instead.",
                        original, id
                    ),
                )),
                None => issues.push(issue(
                    ValidationSeverity::Error,
                    "missing_original_stage_id",
                    format!("{prefix}.original_stage_id"),
                    "Saved stages must include their original stage id.",
                )),
            }
        } else if usage_ids.contains(id) {
            issues.push(issue(
                ValidationSeverity::Error,
                "stage_id_collides_with_existing_history",
                format!("{prefix}.id"),
                format!(
                    "Stage id '{id}' already exists in SQLite history and cannot be reused as a new draft stage."
                ),
            ));
        }

        validate_stage_folder(
            paths,
            &stage.input_folder,
            !is_s3_mode,
            "invalid_stage_input_folder",
            &format!("{prefix}.input_folder"),
            "input_folder must be a relative path inside the workdir.",
            issues,
        );
        let next_stage = normalize_optional(&stage.next_stage);
        let output_required = next_stage.is_some();
        validate_stage_folder(
            paths,
            &stage.output_folder,
            output_required,
            "invalid_stage_output_folder",
            &format!("{prefix}.output_folder"),
            "output_folder must be a relative path inside the workdir.",
            issues,
        );
        if output_required && normalize_optional_str(&stage.output_folder).is_none() {
            issues.push(issue(
                ValidationSeverity::Error,
                "missing_stage_output_folder",
                format!("{prefix}.output_folder"),
                "output_folder is required when next_stage is configured.",
            ));
        }

        let workflow_url = stage.workflow_url.trim();
        if workflow_url.is_empty() {
            issues.push(issue(
                ValidationSeverity::Error,
                "missing_stage_workflow_url",
                format!("{prefix}.workflow_url"),
                "workflow_url is required.",
            ));
        } else if !workflow_url.starts_with("http://") && !workflow_url.starts_with("https://") {
            issues.push(issue(
                ValidationSeverity::Error,
                "invalid_stage_workflow_url",
                format!("{prefix}.workflow_url"),
                "workflow_url must be an http:// or https:// URL.",
            ));
        }
        validate_min_i64(
            stage.max_attempts,
            1,
            "invalid_stage_max_attempts",
            &format!("{prefix}.max_attempts"),
            "max_attempts must be greater than or equal to 1.",
            issues,
        );
        validate_min_i64(
            stage.retry_delay_sec,
            0,
            "invalid_stage_retry_delay_sec",
            &format!("{prefix}.retry_delay_sec"),
            "retry_delay_sec must be greater than or equal to 0.",
            issues,
        );
    }

    validate_stage_links_and_cycles(draft, issues);
}

fn validate_stage_links_and_cycles(
    draft: &PipelineConfigDraft,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let stage_ids = draft
        .stages
        .iter()
        .map(|stage| stage.id.trim().to_string())
        .collect::<HashSet<_>>();
    let mut next_by_stage = HashMap::new();

    for (index, stage) in draft.stages.iter().enumerate() {
        let id = stage.id.trim();
        if id.is_empty() {
            continue;
        }
        if let Some(next_stage) = normalize_optional(&stage.next_stage) {
            if next_stage == id {
                issues.push(issue(
                    ValidationSeverity::Error,
                    "stage_next_stage_self_loop",
                    format!("stages[{index}].next_stage"),
                    "next_stage cannot reference the same stage.",
                ));
            } else if !stage_ids.contains(&next_stage) {
                issues.push(issue(
                    ValidationSeverity::Error,
                    "unknown_stage_next_stage",
                    format!("stages[{index}].next_stage"),
                    format!("next_stage '{next_stage}' does not reference a declared stage."),
                ));
            }
            next_by_stage.insert(id.to_string(), next_stage);
        }
    }

    for stage_id in &stage_ids {
        let mut seen = HashSet::new();
        let mut current = stage_id.as_str();
        while let Some(next) = next_by_stage.get(current) {
            if !seen.insert(current.to_string()) {
                break;
            }
            if next == stage_id {
                issues.push(issue(
                    ValidationSeverity::Error,
                    "stage_graph_cycle",
                    "stages.next_stage",
                    format!("Stage graph contains a cycle involving '{stage_id}'."),
                ));
                break;
            }
            current = next;
        }
    }
}

fn add_removed_stage_warnings(
    draft: &PipelineConfigDraft,
    usages: &[StageUsageSummary],
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let draft_ids = draft
        .stages
        .iter()
        .map(|stage| stage.id.trim().to_string())
        .collect::<HashSet<_>>();
    for usage in usages {
        let has_history = usage.entity_count > 0
            || usage.entity_file_count > 0
            || usage.stage_state_count > 0
            || usage.run_count > 0;
        if usage.is_active && !draft_ids.contains(&usage.stage_id) && has_history {
            issues.push(issue(
                ValidationSeverity::Warning,
                "stage_removed_with_history",
                "stages",
                format!(
                    "Stage '{}' will become inactive/archived; historical entities, files, states, and runs remain in SQLite.",
                    usage.stage_id
                ),
            ));
        }
    }
}

fn validate_stage_folder(
    paths: &EditorPaths,
    value: &str,
    required: bool,
    code: &str,
    path: &str,
    message: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        if required {
            issues.push(issue(ValidationSeverity::Error, code, path, message));
        }
        return;
    }
    let folder = Path::new(trimmed);
    if folder.is_absolute() || trimmed.contains(':') {
        issues.push(issue(ValidationSeverity::Error, code, path, message));
        return;
    }
    if folder.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        issues.push(issue(ValidationSeverity::Error, code, path, message));
        return;
    }
    let normalized = paths.workdir_path.join(folder);
    if normalized == paths.workdir_path {
        issues.push(issue(
            ValidationSeverity::Error,
            code,
            path,
            "Stage folder cannot be the workdir root.",
        ));
    }
}

fn load_stage_usages(database_path: &Path) -> Result<Vec<StageUsageSummary>, String> {
    if !database_path.exists() {
        return Ok(Vec::new());
    }
    let connection = database::open_connection(database_path)?;
    load_stage_usages_from_connection(&connection)
}

fn load_stage_usages_from_connection(
    connection: &Connection,
) -> Result<Vec<StageUsageSummary>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT
                stage.stage_id,
                stage.is_active,
                stage.last_seen_in_config_at,
                stage.archived_at,
                COALESCE((SELECT COUNT(DISTINCT entity_id) FROM entity_stage_states WHERE stage_id = stage.stage_id), 0),
                COALESCE((SELECT COUNT(*) FROM entity_files WHERE stage_id = stage.stage_id), 0),
                COALESCE((SELECT COUNT(*) FROM entity_stage_states WHERE stage_id = stage.stage_id), 0),
                COALESCE((SELECT COUNT(*) FROM stage_runs WHERE stage_id = stage.stage_id), 0)
            FROM stages stage
            ORDER BY stage.stage_id ASC
            "#,
        )
        .map_err(|error| format!("Failed to prepare stage usage query: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            let stage_id: String = row.get(0)?;
            let is_active = row.get::<_, i64>(1)? == 1;
            let entity_count = row.get::<_, i64>(4)? as u64;
            let entity_file_count = row.get::<_, i64>(5)? as u64;
            let stage_state_count = row.get::<_, i64>(6)? as u64;
            let run_count = row.get::<_, i64>(7)? as u64;
            let has_history =
                entity_count > 0 || entity_file_count > 0 || stage_state_count > 0 || run_count > 0;
            let mut warnings = Vec::new();
            if has_history {
                warnings.push(
                    "Removing this stage archives it from active config but preserves SQLite history."
                        .to_string(),
                );
            }
            Ok(StageUsageSummary {
                stage_id,
                is_active,
                entity_count,
                entity_file_count,
                stage_state_count,
                run_count,
                last_seen_in_config_at: row.get(2)?,
                archived_at: row.get(3)?,
                can_remove_from_config: true,
                can_rename: false,
                warnings,
            })
        })
        .map_err(|error| format!("Failed to query stage usage: {error}"))?;

    let mut usages = Vec::new();
    for row in rows {
        usages.push(row.map_err(|error| format!("Failed to read stage usage row: {error}"))?);
    }
    Ok(usages)
}

fn write_pipeline_yaml_atomic(pipeline_path: &Path, yaml_text: &str) -> Result<PathBuf, String> {
    let parent = pipeline_path
        .parent()
        .ok_or_else(|| format!("Pipeline path '{}' has no parent.", pipeline_path.display()))?;
    let now = Utc::now();
    let timestamp = now.format("%Y%m%dT%H%M%S%.3fZ");
    let unique_suffix = now
        .timestamp_nanos_opt()
        .unwrap_or_else(|| now.timestamp_micros() * 1000);
    let backup_path = parent.join(format!("pipeline.yaml.bak.{timestamp}.{unique_suffix}"));
    let temp_path = parent.join(format!(".pipeline.yaml.tmp.{timestamp}.{unique_suffix}"));

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
            Ok(()) => format!("Failed to install new pipeline.yaml; original was restored: {error}"),
            Err(restore_error) => format!(
                "Failed to install new pipeline.yaml and failed to restore backup '{}': {error}; restore error: {restore_error}",
                backup_path.display()
            ),
        });
    }

    Ok(backup_path)
}

fn serialize_pipeline_yaml(config: &PipelineConfig) -> Result<String, String> {
    serde_yaml::to_string(config)
        .map_err(|error| format!("Failed to serialize pipeline YAML: {error}"))
}

fn draft_from_config(config: &PipelineConfig) -> PipelineConfigDraft {
    PipelineConfigDraft {
        project: ProjectConfigDraft {
            name: config.project.name.clone(),
            workdir: config.project.workdir.clone(),
        },
        storage: config.storage.clone(),
        runtime: RuntimeConfigDraft {
            scan_interval_sec: config.runtime.scan_interval_sec as i64,
            max_parallel_tasks: config.runtime.max_parallel_tasks as i64,
            stuck_task_timeout_sec: config.runtime.stuck_task_timeout_sec as i64,
            request_timeout_sec: config.runtime.request_timeout_sec as i64,
            file_stability_delay_ms: config.runtime.file_stability_delay_ms as i64,
        },
        stages: config
            .stages
            .iter()
            .map(|stage| StageDefinitionDraft {
                id: stage.id.clone(),
                input_folder: stage.input_folder.clone(),
                input_uri: stage.input_uri.clone(),
                output_folder: stage.output_folder.clone(),
                workflow_url: stage.workflow_url.clone(),
                max_attempts: stage.max_attempts as i64,
                retry_delay_sec: stage.retry_delay_sec as i64,
                next_stage: stage.next_stage.clone(),
                save_path_aliases: stage.save_path_aliases.clone(),
                allow_empty_outputs: stage.allow_empty_outputs,
                original_stage_id: Some(stage.id.clone()),
                is_new: false,
            })
            .collect(),
    }
}

struct StageChangeSet {
    added: Vec<String>,
    removed: Vec<String>,
    updated: Vec<String>,
}

fn stage_change_set(before: &PipelineConfig, after: &PipelineConfig) -> StageChangeSet {
    let before_map = before
        .stages
        .iter()
        .map(|stage| (stage.id.clone(), stage.clone()))
        .collect::<HashMap<_, _>>();
    let after_map = after
        .stages
        .iter()
        .map(|stage| (stage.id.clone(), stage.clone()))
        .collect::<HashMap<_, _>>();
    let mut added = after_map
        .keys()
        .filter(|id| !before_map.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    let mut removed = before_map
        .keys()
        .filter(|id| !after_map.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    let mut updated = after_map
        .iter()
        .filter(|(id, stage)| before_map.get(*id).is_some_and(|before| before != *stage))
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    added.sort();
    removed.sort();
    updated.sort();
    StageChangeSet {
        added,
        removed,
        updated,
    }
}

fn is_safe_stage_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn validate_min_i64(
    value: i64,
    min: i64,
    code: &str,
    path: &str,
    message: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if value < min {
        issues.push(issue(ValidationSeverity::Error, code, path, message));
    }
}

fn required_string(
    value: &str,
    code: &str,
    path: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        issues.push(issue(
            ValidationSeverity::Error,
            code,
            path,
            format!("{path} is required."),
        ));
        None
    } else {
        Some(value.to_string())
    }
}

fn normalize_optional(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_optional_str(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn issue(
    severity: ValidationSeverity,
    code: impl Into<String>,
    path: impl Into<String>,
    message: impl Into<String>,
) -> ConfigValidationIssue {
    ConfigValidationIssue {
        severity,
        code: code.into(),
        path: path.into(),
        message: message.into(),
    }
}

fn command_error(code: &str, message: impl Into<String>, path: Option<String>) -> CommandErrorInfo {
    CommandErrorInfo {
        code: code.to_string(),
        message: message.into(),
        path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{bootstrap_database, list_app_events, list_stages};
    use rusqlite::{params, Connection};

    fn test_config(stages: Vec<StageDefinition>) -> PipelineConfig {
        PipelineConfig {
            project: ProjectConfig {
                name: "beehive".to_string(),
                workdir: ".".to_string(),
            },
            storage: None,
            runtime: RuntimeConfig::default(),
            stages,
        }
    }

    fn stage(id: &str, next_stage: Option<&str>) -> StageDefinition {
        StageDefinition {
            id: id.to_string(),
            input_folder: format!("stages/{id}"),
            input_uri: None,
            output_folder: next_stage
                .map(|_| format!("stages/{id}_out"))
                .unwrap_or_default(),
            workflow_url: format!("http://localhost:5678/webhook/{id}"),
            max_attempts: 3,
            retry_delay_sec: 10,
            next_stage: next_stage.map(ToOwned::to_owned),
            save_path_aliases: Vec::new(),
            allow_empty_outputs: false,
        }
    }

    fn setup_workdir(config: &PipelineConfig) -> (tempfile::TempDir, PathBuf) {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        fs::create_dir_all(&workdir).expect("workdir");
        let yaml = serialize_pipeline_yaml(config).expect("yaml");
        fs::write(workdir.join("pipeline.yaml"), yaml).expect("pipeline");
        bootstrap_database(&workdir.join("app.db"), config).expect("bootstrap");
        fs::create_dir_all(workdir.join("stages")).expect("stages");
        fs::create_dir_all(workdir.join("logs")).expect("logs");
        (tempdir, workdir)
    }

    fn draft_from_stages(stages: Vec<StageDefinitionDraft>) -> PipelineConfigDraft {
        PipelineConfigDraft {
            project: ProjectConfigDraft {
                name: "beehive".to_string(),
                workdir: ".".to_string(),
            },
            storage: None,
            runtime: RuntimeConfigDraft {
                scan_interval_sec: 5,
                max_parallel_tasks: 3,
                stuck_task_timeout_sec: 900,
                request_timeout_sec: 30,
                file_stability_delay_ms: 1000,
            },
            stages,
        }
    }

    fn new_stage_draft(id: &str, next_stage: Option<&str>) -> StageDefinitionDraft {
        StageDefinitionDraft {
            id: id.to_string(),
            input_folder: format!("stages/{id}"),
            input_uri: None,
            output_folder: next_stage
                .map(|_| format!("stages/{id}_out"))
                .unwrap_or_default(),
            workflow_url: format!("http://localhost:5678/webhook/{id}"),
            max_attempts: 3,
            retry_delay_sec: 10,
            next_stage: next_stage.map(ToOwned::to_owned),
            save_path_aliases: Vec::new(),
            allow_empty_outputs: false,
            original_stage_id: None,
            is_new: true,
        }
    }

    #[test]
    fn editor_state_returns_current_config_yaml_and_usage() {
        let (_tempdir, workdir) = setup_workdir(&test_config(vec![stage("incoming", None)]));
        let state = get_pipeline_editor_state(workdir.to_str().expect("workdir")).expect("state");

        assert!(state.validation.is_valid);
        assert_eq!(state.config.expect("config").stages.len(), 1);
        assert!(state.yaml_text.contains("incoming"));
        assert_eq!(state.stage_usages.len(), 1);
        assert_eq!(state.stage_usages[0].stage_id, "incoming");
        assert!(!state.stage_usages[0].can_rename);
    }

    #[test]
    fn valid_draft_saves_yaml_creates_backup_syncs_stage_and_provisions_directories() {
        let (_tempdir, workdir) = setup_workdir(&test_config(vec![stage("incoming", None)]));
        let draft = draft_from_stages(vec![
            StageDefinitionDraft {
                original_stage_id: Some("incoming".to_string()),
                is_new: false,
                ..new_stage_draft("incoming", Some("normalized"))
            },
            new_stage_draft("normalized", None),
        ]);

        let result = save_pipeline_config(
            workdir.to_str().expect("workdir"),
            &draft,
            Some("add stage"),
        )
        .expect("save");
        let pipeline_text = fs::read_to_string(workdir.join("pipeline.yaml")).expect("pipeline");
        let stages = list_stages(&workdir.join("app.db")).expect("stages");
        let events = list_app_events(&workdir.join("app.db"), 20).expect("events");

        assert!(result.backup_path.is_some());
        assert!(Path::new(result.backup_path.as_deref().unwrap()).exists());
        assert!(pipeline_text.contains("normalized"));
        assert!(stages
            .iter()
            .any(|stage| stage.id == "normalized" && stage.is_active));
        assert!(workdir.join("stages/normalized").exists());
        assert!(events
            .iter()
            .any(|event| event.code == "pipeline_config_saved"));
    }

    #[test]
    fn invalid_draft_does_not_overwrite_yaml() {
        let (_tempdir, workdir) = setup_workdir(&test_config(vec![stage("incoming", None)]));
        let before = fs::read_to_string(workdir.join("pipeline.yaml")).expect("before");
        let draft = draft_from_stages(vec![new_stage_draft("bad stage", None)]);

        let result = save_pipeline_config(workdir.to_str().expect("workdir"), &draft, None)
            .expect("rejected result");
        let after = fs::read_to_string(workdir.join("pipeline.yaml")).expect("after");

        assert!(result.state.is_none());
        assert!(!result.errors.is_empty());
        assert_eq!(before, after);
    }

    #[test]
    fn validation_rejects_invalid_stage_shapes_and_paths() {
        let (_tempdir, workdir) = setup_workdir(&test_config(vec![stage("incoming", None)]));
        let cases = vec![
            (
                "duplicate_stage_id",
                vec![new_stage_draft("dup", None), new_stage_draft("dup", None)],
            ),
            ("invalid_stage_id", vec![new_stage_draft("bad stage", None)]),
            (
                "invalid_stage_input_folder",
                vec![StageDefinitionDraft {
                    input_folder: "../outside".to_string(),
                    ..new_stage_draft("safe", None)
                }],
            ),
            (
                "invalid_stage_output_folder",
                vec![
                    StageDefinitionDraft {
                        output_folder: "C:\\outside".to_string(),
                        ..new_stage_draft("safe", Some("done"))
                    },
                    new_stage_draft("done", None),
                ],
            ),
            (
                "missing_stage_output_folder",
                vec![
                    StageDefinitionDraft {
                        output_folder: "".to_string(),
                        ..new_stage_draft("safe", Some("done"))
                    },
                    new_stage_draft("done", None),
                ],
            ),
            (
                "invalid_stage_workflow_url",
                vec![StageDefinitionDraft {
                    workflow_url: "ftp://example".to_string(),
                    ..new_stage_draft("safe", None)
                }],
            ),
            (
                "unknown_stage_next_stage",
                vec![new_stage_draft("safe", Some("missing"))],
            ),
            (
                "stage_next_stage_self_loop",
                vec![new_stage_draft("safe", Some("safe"))],
            ),
            (
                "stage_graph_cycle",
                vec![
                    new_stage_draft("a", Some("b")),
                    new_stage_draft("b", Some("a")),
                ],
            ),
        ];

        for (code, stages) in cases {
            let draft = draft_from_stages(stages);
            let result = validate_pipeline_config_draft(workdir.to_str().expect("workdir"), &draft)
                .unwrap_or_else(|error| panic!("{code} command failed: {error}"));
            assert!(
                result
                    .validation
                    .issues
                    .iter()
                    .any(|issue| issue.code == code),
                "expected validation code {code}, got {:?}",
                result.validation.issues
            );
        }
    }

    #[test]
    fn terminal_without_output_is_valid_but_non_terminal_requires_output() {
        let (_tempdir, workdir) = setup_workdir(&test_config(vec![stage("incoming", None)]));
        let terminal = validate_pipeline_config_draft(
            workdir.to_str().expect("workdir"),
            &draft_from_stages(vec![new_stage_draft("terminal", None)]),
        )
        .expect("terminal");
        let non_terminal = validate_pipeline_config_draft(
            workdir.to_str().expect("workdir"),
            &draft_from_stages(vec![
                StageDefinitionDraft {
                    output_folder: "".to_string(),
                    ..new_stage_draft("incoming", Some("done"))
                },
                new_stage_draft("done", None),
            ]),
        )
        .expect("non terminal");

        assert!(terminal.validation.is_valid);
        assert!(!non_terminal.validation.is_valid);
        assert!(non_terminal
            .validation
            .issues
            .iter()
            .any(|issue| issue.code == "missing_stage_output_folder"));
    }

    #[test]
    fn saved_stage_rename_is_rejected() {
        let (_tempdir, workdir) = setup_workdir(&test_config(vec![stage("incoming", None)]));
        let draft = draft_from_stages(vec![StageDefinitionDraft {
            id: "renamed".to_string(),
            original_stage_id: Some("incoming".to_string()),
            is_new: false,
            ..new_stage_draft("incoming", None)
        }]);

        let result = validate_pipeline_config_draft(workdir.to_str().expect("workdir"), &draft)
            .expect("validation");

        assert!(!result.validation.is_valid);
        assert!(result
            .validation
            .issues
            .iter()
            .any(|issue| issue.code == "saved_stage_id_rename_forbidden"));
    }

    #[test]
    fn removing_stage_archives_it_and_preserves_runtime_history() {
        let config = test_config(vec![stage("incoming", Some("done")), stage("done", None)]);
        let (_tempdir, workdir) = setup_workdir(&config);
        let db = workdir.join("app.db");
        let connection = Connection::open(&db).expect("connection");
        connection
            .execute(
                "INSERT INTO entities (entity_id, current_stage_id, current_status, latest_file_path, latest_file_id, file_count, validation_status, validation_errors_json, first_seen_at, last_seen_at, updated_at)
                 VALUES ('entity-1', 'done', 'done', 'stages/done/entity-1.json', NULL, 1, 'valid', '[]', 'now', 'now', 'now')",
                [],
            )
            .expect("entity");
        connection
            .execute(
                "INSERT INTO entity_files (entity_id, stage_id, file_path, file_name, checksum, file_mtime, file_size, payload_json, meta_json, status, validation_status, validation_errors_json, first_seen_at, last_seen_at, updated_at)
                 VALUES ('entity-1', 'done', 'stages/done/entity-1.json', 'entity-1.json', 'abc', 'now', 10, '{}', '{}', 'done', 'valid', '[]', 'now', 'now', 'now')",
                [],
            )
            .expect("file");
        let file_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO entity_stage_states (entity_id, stage_id, file_path, file_instance_id, file_exists, status, attempts, max_attempts, discovered_at, updated_at)
                 VALUES ('entity-1', 'done', 'stages/done/entity-1.json', ?1, 1, 'done', 1, 3, 'now', 'now')",
                params![file_id],
            )
            .expect("state");
        connection
            .execute(
                "INSERT INTO stage_runs (run_id, entity_id, entity_file_id, stage_id, attempt_no, workflow_url, request_json, success, started_at)
                 VALUES ('run-1', 'entity-1', ?1, 'done', 1, 'http://localhost', '{}', 1, 'now')",
                params![file_id],
            )
            .expect("run");
        drop(connection);

        let draft = draft_from_stages(vec![StageDefinitionDraft {
            original_stage_id: Some("incoming".to_string()),
            is_new: false,
            ..new_stage_draft("incoming", None)
        }]);
        let validation = validate_pipeline_config_draft(workdir.to_str().expect("workdir"), &draft)
            .expect("validation");
        assert!(validation
            .validation
            .issues
            .iter()
            .any(|issue| issue.code == "stage_removed_with_history"));

        save_pipeline_config(workdir.to_str().expect("workdir"), &draft, None).expect("save");
        let stages = list_stages(&db).expect("stages");
        let archived = stages
            .iter()
            .find(|stage| stage.id == "done")
            .expect("done");
        let connection = Connection::open(&db).expect("connection");
        let state_count = connection
            .query_row(
                "SELECT COUNT(*) FROM entity_stage_states WHERE stage_id = 'done'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("state count");
        let run_count = connection
            .query_row(
                "SELECT COUNT(*) FROM stage_runs WHERE stage_id = 'done'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("run count");

        assert!(!archived.is_active);
        assert!(archived.archived_at.is_some());
        assert_eq!(state_count, 1);
        assert_eq!(run_count, 1);
    }
}
