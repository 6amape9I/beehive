use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::domain::{
    ConfigValidationIssue, ConfigValidationResult, PipelineConfig, ProjectConfig, RuntimeConfig,
    StageDefinition, StorageConfig, StorageProvider, ValidationSeverity,
};

#[derive(Debug, Deserialize)]
struct RawPipelineConfig {
    project: Option<RawProjectConfig>,
    storage: Option<RawStorageConfig>,
    runtime: Option<RawRuntimeConfig>,
    stages: Option<Vec<RawStageDefinition>>,
}

#[derive(Debug, Deserialize)]
struct RawProjectConfig {
    name: Option<String>,
    workdir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawRuntimeConfig {
    scan_interval_sec: Option<u64>,
    max_parallel_tasks: Option<u64>,
    stuck_task_timeout_sec: Option<u64>,
    request_timeout_sec: Option<u64>,
    file_stability_delay_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawStorageConfig {
    provider: Option<String>,
    bucket: Option<String>,
    workspace_prefix: Option<String>,
    region: Option<String>,
    endpoint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawStageDefinition {
    id: Option<String>,
    input_folder: Option<String>,
    input_uri: Option<String>,
    output_folder: Option<String>,
    workflow_url: Option<String>,
    max_attempts: Option<i64>,
    retry_delay_sec: Option<i64>,
    next_stage: Option<String>,
    save_path_aliases: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct LoadedPipelineConfig {
    pub config: Option<PipelineConfig>,
    pub validation: ConfigValidationResult,
    pub loaded_at: String,
}

pub fn default_pipeline_yaml() -> &'static str {
    r#"project:
  name: beehive
  workdir: .

runtime:
  scan_interval_sec: 5
  max_parallel_tasks: 3
  stuck_task_timeout_sec: 900
  request_timeout_sec: 30
  file_stability_delay_ms: 1000

stages:
  - id: ingest
    input_folder: stages/incoming
    output_folder: stages/normalized
    workflow_url: http://localhost:5678/webhook/ingest
    max_attempts: 3
    retry_delay_sec: 10
    next_stage: normalize
  - id: normalize
    input_folder: stages/normalized
    output_folder: stages/done
    workflow_url: http://localhost:5678/webhook/normalize
    max_attempts: 3
    retry_delay_sec: 10
    next_stage:
"#
}

pub fn load_pipeline_config(path: &Path) -> LoadedPipelineConfig {
    let loaded_at = chrono::Utc::now().to_rfc3339();
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            return LoadedPipelineConfig {
                config: None,
                validation: ConfigValidationResult::from_issues(vec![issue(
                    ValidationSeverity::Error,
                    "config_read_failed",
                    "pipeline.yaml",
                    format!("Failed to read pipeline.yaml: {error}"),
                )]),
                loaded_at,
            };
        }
    };

    parse_pipeline_config(&content, loaded_at)
}

pub fn parse_pipeline_config(content: &str, loaded_at: String) -> LoadedPipelineConfig {
    let raw = match serde_yaml::from_str::<RawPipelineConfig>(content) {
        Ok(raw) => raw,
        Err(error) => {
            return LoadedPipelineConfig {
                config: None,
                validation: ConfigValidationResult::from_issues(vec![issue(
                    ValidationSeverity::Error,
                    "yaml_parse_failed",
                    "pipeline.yaml",
                    format!("Failed to parse YAML: {error}"),
                )]),
                loaded_at,
            };
        }
    };

    let (config, validation) = validate_and_build(raw);
    LoadedPipelineConfig {
        config,
        validation,
        loaded_at,
    }
}

fn validate_and_build(raw: RawPipelineConfig) -> (Option<PipelineConfig>, ConfigValidationResult) {
    let mut issues = Vec::new();

    let project = match raw.project {
        Some(project) => {
            let name = required_string(
                project.name,
                "missing_project_name",
                "project.name",
                &mut issues,
            );
            let workdir = required_string(
                project.workdir,
                "missing_project_workdir",
                "project.workdir",
                &mut issues,
            );

            match (name, workdir) {
                (Some(name), Some(workdir)) => Some(ProjectConfig { name, workdir }),
                _ => None,
            }
        }
        None => {
            issues.push(issue(
                ValidationSeverity::Error,
                "missing_project",
                "project",
                "The project section is required.",
            ));
            None
        }
    };

    let storage = build_storage_config(raw.storage, &mut issues);
    let is_s3_mode = storage
        .as_ref()
        .is_some_and(|storage| storage.provider == StorageProvider::S3);
    let s3_bucket = storage
        .as_ref()
        .and_then(|storage| storage.bucket.as_deref());

    let runtime = match raw.runtime {
        Some(runtime) => {
            let request_timeout_sec = runtime.request_timeout_sec.unwrap_or(30);
            if request_timeout_sec == 0 {
                issues.push(issue(
                    ValidationSeverity::Error,
                    "invalid_runtime_request_timeout_sec",
                    "runtime.request_timeout_sec",
                    "request_timeout_sec must be greater than 0.",
                ));
            }
            RuntimeConfig {
                scan_interval_sec: runtime.scan_interval_sec.unwrap_or(5),
                max_parallel_tasks: runtime.max_parallel_tasks.unwrap_or(3),
                stuck_task_timeout_sec: runtime.stuck_task_timeout_sec.unwrap_or(900),
                request_timeout_sec: request_timeout_sec.max(1),
                file_stability_delay_ms: runtime.file_stability_delay_ms.unwrap_or(1000),
            }
        }
        None => {
            issues.push(issue(
                ValidationSeverity::Warning,
                "runtime_defaults_applied",
                "runtime",
                "The runtime section is missing; safe defaults were applied.",
            ));
            RuntimeConfig::default()
        }
    };

    let stages = match raw.stages {
        Some(stages) => build_stages(stages, is_s3_mode, s3_bucket, &mut issues),
        None => {
            issues.push(issue(
                ValidationSeverity::Error,
                "missing_stages",
                "stages",
                "The stages field must be an array.",
            ));
            Vec::new()
        }
    };
    validate_stage_links(&stages, &mut issues);

    let validation = ConfigValidationResult::from_issues(issues);
    if validation.is_valid {
        if let Some(project) = project {
            return (
                Some(PipelineConfig {
                    project,
                    storage,
                    runtime,
                    stages,
                }),
                validation,
            );
        }
    }

    (None, validation)
}

fn build_storage_config(
    raw_storage: Option<RawStorageConfig>,
    issues: &mut Vec<ConfigValidationIssue>,
) -> Option<StorageConfig> {
    let Some(raw_storage) = raw_storage else {
        return None;
    };
    let provider_text = normalize_optional_string(raw_storage.provider)
        .unwrap_or_else(|| "local".to_string())
        .to_lowercase();
    let provider = match provider_text.as_str() {
        "local" => StorageProvider::Local,
        "s3" => StorageProvider::S3,
        _ => {
            issues.push(issue(
                ValidationSeverity::Error,
                "invalid_storage_provider",
                "storage.provider",
                "storage.provider must be 'local' or 's3'.",
            ));
            StorageProvider::Local
        }
    };

    let bucket = normalize_optional_string(raw_storage.bucket);
    let workspace_prefix = normalize_optional_string(raw_storage.workspace_prefix);
    let region = normalize_optional_string(raw_storage.region);
    let endpoint = normalize_optional_string(raw_storage.endpoint);

    if provider == StorageProvider::S3 {
        if bucket.is_none() {
            issues.push(issue(
                ValidationSeverity::Error,
                "missing_storage_bucket",
                "storage.bucket",
                "storage.bucket is required when storage.provider is s3.",
            ));
        }
        match workspace_prefix.as_deref() {
            Some(prefix) => {
                if normalize_logical_route(prefix, "storage.workspace_prefix").is_err() {
                    issues.push(issue(
                        ValidationSeverity::Error,
                        "invalid_storage_workspace_prefix",
                        "storage.workspace_prefix",
                        "storage.workspace_prefix must be a safe slash-separated logical prefix.",
                    ));
                }
            }
            None => issues.push(issue(
                ValidationSeverity::Error,
                "missing_storage_workspace_prefix",
                "storage.workspace_prefix",
                "storage.workspace_prefix is required when storage.provider is s3.",
            )),
        }
    }

    Some(StorageConfig {
        provider,
        bucket,
        workspace_prefix,
        region,
        endpoint,
    })
}

fn build_stages(
    raw_stages: Vec<RawStageDefinition>,
    is_s3_mode: bool,
    s3_bucket: Option<&str>,
    issues: &mut Vec<ConfigValidationIssue>,
) -> Vec<StageDefinition> {
    let mut stage_ids = HashSet::new();
    let mut stages = Vec::new();

    for (index, raw_stage) in raw_stages.into_iter().enumerate() {
        let prefix = format!("stages[{index}]");
        let id = required_string(
            raw_stage.id,
            "missing_stage_id",
            &format!("{prefix}.id"),
            issues,
        );
        let input_folder = if is_s3_mode {
            normalize_optional_string(raw_stage.input_folder).unwrap_or_default()
        } else {
            required_string(
                raw_stage.input_folder,
                "missing_stage_input_folder",
                &format!("{prefix}.input_folder"),
                issues,
            )
            .unwrap_or_default()
        };
        let input_uri = normalize_optional_string(raw_stage.input_uri);
        if is_s3_mode {
            match input_uri.as_deref() {
                Some(uri) => {
                    if let Err(message) = validate_s3_uri(uri, s3_bucket) {
                        issues.push(issue(
                            ValidationSeverity::Error,
                            "invalid_stage_input_uri",
                            format!("{prefix}.input_uri"),
                            message,
                        ));
                    }
                }
                None => issues.push(issue(
                    ValidationSeverity::Error,
                    "missing_stage_input_uri",
                    format!("{prefix}.input_uri"),
                    "input_uri is required for S3 stages in B1.",
                )),
            }
        }
        let output_folder = normalize_optional_string(raw_stage.output_folder);
        let next_stage = normalize_optional_string(raw_stage.next_stage);
        if !is_s3_mode && next_stage.is_some() && output_folder.is_none() {
            issues.push(issue(
                ValidationSeverity::Error,
                "missing_stage_output_folder",
                format!("{prefix}.output_folder"),
                "output_folder is required when next_stage is configured.",
            ));
        }
        let workflow_url = required_string(
            raw_stage.workflow_url,
            "missing_stage_workflow_url",
            &format!("{prefix}.workflow_url"),
            issues,
        );
        if let Some(workflow_url) = workflow_url.as_ref() {
            if !is_allowed_workflow_url(workflow_url) {
                issues.push(issue(
                    ValidationSeverity::Error,
                    "invalid_stage_workflow_url",
                    format!("{prefix}.workflow_url"),
                    "workflow_url must be an http:// or https:// URL.",
                ));
            }
        }

        if let Some(id) = &id {
            if !stage_ids.insert(id.clone()) {
                issues.push(issue(
                    ValidationSeverity::Error,
                    "duplicate_stage_id",
                    format!("{prefix}.id"),
                    format!("Stage id '{id}' is declared more than once."),
                ));
            }
        }

        let max_attempts = raw_stage.max_attempts.unwrap_or(3);
        if max_attempts < 1 {
            issues.push(issue(
                ValidationSeverity::Error,
                "invalid_stage_max_attempts",
                format!("{prefix}.max_attempts"),
                "max_attempts must be greater than or equal to 1.",
            ));
        }

        let retry_delay_sec = raw_stage.retry_delay_sec.unwrap_or(0);
        if retry_delay_sec < 0 {
            issues.push(issue(
                ValidationSeverity::Error,
                "invalid_stage_retry_delay_sec",
                format!("{prefix}.retry_delay_sec"),
                "retry_delay_sec must be greater than or equal to 0.",
            ));
        }

        let mut save_path_aliases = Vec::new();
        for alias in raw_stage.save_path_aliases.unwrap_or_default() {
            let Some(alias) = normalize_optional_string(Some(alias)) else {
                continue;
            };
            if normalize_logical_route(&alias, "save_path_aliases").is_err() {
                issues.push(issue(
                    ValidationSeverity::Error,
                    "invalid_stage_save_path_alias",
                    format!("{prefix}.save_path_aliases"),
                    "save_path_aliases must contain only safe logical routes.",
                ));
            }
            save_path_aliases.push(alias);
        }

        if let (Some(id), Some(workflow_url)) = (id, workflow_url) {
            stages.push(StageDefinition {
                id,
                input_folder,
                input_uri,
                output_folder: output_folder.unwrap_or_default(),
                workflow_url,
                max_attempts: max_attempts.max(1) as u64,
                retry_delay_sec: retry_delay_sec.max(0) as u64,
                next_stage,
                save_path_aliases,
            });
        }
    }

    stages
}

fn validate_stage_links(stages: &[StageDefinition], issues: &mut Vec<ConfigValidationIssue>) {
    let stage_ids: HashSet<&str> = stages.iter().map(|stage| stage.id.as_str()).collect();
    for (index, stage) in stages.iter().enumerate() {
        if let Some(next_stage) = stage.next_stage.as_deref() {
            if !stage_ids.contains(next_stage) {
                issues.push(issue(
                    ValidationSeverity::Error,
                    "unknown_stage_next_stage",
                    format!("stages[{index}].next_stage"),
                    format!("next_stage '{next_stage}' does not reference a declared stage."),
                ));
            }
        }
    }
}

fn is_allowed_workflow_url(value: &str) -> bool {
    value.starts_with("https://") || value.starts_with("http://")
}

fn validate_s3_uri(value: &str, expected_bucket: Option<&str>) -> Result<(), String> {
    let Some(without_scheme) = value.strip_prefix("s3://") else {
        return Err("input_uri must start with s3://.".to_string());
    };
    let Some((bucket, key)) = without_scheme.split_once('/') else {
        return Err("input_uri must include bucket and key prefix.".to_string());
    };
    if bucket.trim().is_empty() || key.trim().is_empty() {
        return Err("input_uri must include non-empty bucket and key prefix.".to_string());
    }
    if let Some(expected_bucket) = expected_bucket {
        if bucket != expected_bucket {
            return Err(format!(
                "input_uri bucket '{bucket}' must match configured storage.bucket '{expected_bucket}'."
            ));
        }
    }
    normalize_logical_route(key, "input_uri key").map(|_| ())
}

fn normalize_logical_route(value: &str, label: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} is empty."));
    }
    let normalized = trimmed.replace('\\', "/");
    if trimmed.starts_with("\\\\") || normalized.starts_with("//") {
        return Err(format!("{label} must not be a UNC path."));
    }
    let bytes = normalized.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(format!("{label} must not contain a Windows drive prefix."));
    }
    if normalized.contains(':') {
        return Err(format!("{label} must not contain ':' characters."));
    }
    let logical = if normalized.starts_with('/') {
        if normalized == "/main_dir" || normalized.starts_with("/main_dir/") {
            normalized.trim_start_matches('/').to_string()
        } else {
            return Err(format!("{label} must not be an absolute OS path."));
        }
    } else {
        normalized
    };
    let mut parts = Vec::new();
    for component in logical.split('/') {
        match component {
            "" | "." => {}
            ".." => return Err(format!("{label} must not contain '..' components.")),
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err(format!("{label} does not contain a path."));
    }
    Ok(parts.join("/"))
}

fn required_string(
    value: Option<String>,
    code: &str,
    path: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) -> Option<String> {
    match value.map(|value| value.trim().to_string()) {
        Some(value) if !value.is_empty() => Some(value),
        _ => {
            issues.push(issue(
                ValidationSeverity::Error,
                code,
                path,
                format!("{path} is required."),
            ));
            None
        }
    }
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pipeline_config_is_valid() {
        let loaded = parse_pipeline_config(default_pipeline_yaml(), "now".to_string());

        assert!(loaded.validation.is_valid);
        assert_eq!(loaded.config.unwrap().stages.len(), 2);
    }

    #[test]
    fn duplicate_stage_ids_are_invalid() {
        let yaml = r#"
project:
  name: beehive
  workdir: .
stages:
  - id: ingest
    input_folder: stages/incoming
    output_folder: stages/normalized
    workflow_url: http://localhost:5678/webhook/ingest
  - id: ingest
    input_folder: stages/other
    output_folder: stages/done
    workflow_url: http://localhost:5678/webhook/other
"#;

        let loaded = parse_pipeline_config(yaml, "now".to_string());

        assert!(!loaded.validation.is_valid);
        assert!(loaded
            .validation
            .issues
            .iter()
            .any(|issue| issue.code == "duplicate_stage_id"));
    }

    #[test]
    fn missing_required_stage_field_is_invalid() {
        let yaml = r#"
project:
  name: beehive
  workdir: .
stages:
  - id: ingest
    output_folder: stages/normalized
    workflow_url: http://localhost:5678/webhook/ingest
"#;

        let loaded = parse_pipeline_config(yaml, "now".to_string());

        assert!(!loaded.validation.is_valid);
        assert!(loaded
            .validation
            .issues
            .iter()
            .any(|issue| issue.code == "missing_stage_input_folder"));
    }

    #[test]
    fn missing_runtime_applies_defaults_without_invalidating_config() {
        let yaml = r#"
project:
  name: beehive
  workdir: .
stages:
  - id: ingest
    input_folder: stages/incoming
    output_folder: stages/normalized
    workflow_url: http://localhost:5678/webhook/ingest
"#;

        let loaded = parse_pipeline_config(yaml, "now".to_string());
        let config = loaded.config.expect("config");

        assert!(loaded.validation.is_valid);
        assert_eq!(config.runtime, RuntimeConfig::default());
        assert!(loaded
            .validation
            .issues
            .iter()
            .any(|issue| issue.code == "runtime_defaults_applied"));
    }

    #[test]
    fn terminal_stage_without_output_folder_is_valid() {
        let yaml = r#"
project:
  name: beehive
  workdir: .
stages:
  - id: terminal
    input_folder: stages/terminal
    workflow_url: http://localhost:5678/webhook/terminal
"#;

        let loaded = parse_pipeline_config(yaml, "now".to_string());
        let config = loaded.config.expect("config");

        assert!(loaded.validation.is_valid);
        assert_eq!(config.stages[0].output_folder, "");
        assert_eq!(config.stages[0].next_stage, None);
    }

    #[test]
    fn s3_pipeline_config_is_valid_without_stage_input_folder() {
        let yaml = r#"
project:
  name: beehive-s3-dev
  workdir: .
storage:
  provider: s3
  bucket: steos-s3-data
  workspace_prefix: main_dir
runtime:
  request_timeout_sec: 300
stages:
  - id: raw
    input_uri: s3://steos-s3-data/main_dir/raw
    workflow_url: http://localhost:5678/webhook/raw
    next_stage: raw_entities
    save_path_aliases:
      - main_dir/raw
      - /main_dir/raw
  - id: raw_entities
    input_uri: s3://steos-s3-data/main_dir/processed/raw_entities
    workflow_url: http://localhost:5678/webhook/raw_entities
    save_path_aliases:
      - main_dir/processed/raw_entities
"#;

        let loaded = parse_pipeline_config(yaml, "now".to_string());
        let config = loaded.config.expect("s3 config");

        assert!(loaded.validation.is_valid, "{:?}", loaded.validation.issues);
        assert_eq!(
            config.storage.as_ref().map(|storage| &storage.provider),
            Some(&StorageProvider::S3)
        );
        assert_eq!(config.stages[0].input_folder, "");
        assert_eq!(
            config.stages[1].input_uri.as_deref(),
            Some("s3://steos-s3-data/main_dir/processed/raw_entities")
        );
        assert_eq!(config.stages[1].save_path_aliases.len(), 1);
    }

    #[test]
    fn s3_pipeline_rejects_missing_bucket_invalid_input_uri_and_unsafe_alias() {
        let missing_bucket = r#"
project:
  name: beehive-s3-dev
  workdir: .
storage:
  provider: s3
  workspace_prefix: main_dir
stages:
  - id: raw
    input_uri: s3://steos-s3-data/main_dir/raw
    workflow_url: http://localhost:5678/webhook/raw
"#;
        let invalid_uri = r#"
project:
  name: beehive-s3-dev
  workdir: .
storage:
  provider: s3
  bucket: steos-s3-data
  workspace_prefix: main_dir
stages:
  - id: raw
    input_uri: file://main_dir/raw
    workflow_url: http://localhost:5678/webhook/raw
"#;
        let unsafe_alias = r#"
project:
  name: beehive-s3-dev
  workdir: .
storage:
  provider: s3
  bucket: steos-s3-data
  workspace_prefix: main_dir
stages:
  - id: raw
    input_uri: s3://steos-s3-data/main_dir/raw
    workflow_url: http://localhost:5678/webhook/raw
    save_path_aliases:
      - ../outside
"#;

        for (yaml, code) in [
            (missing_bucket, "missing_storage_bucket"),
            (invalid_uri, "invalid_stage_input_uri"),
            (unsafe_alias, "invalid_stage_save_path_alias"),
        ] {
            let loaded = parse_pipeline_config(yaml, "now".to_string());
            assert!(!loaded.validation.is_valid);
            assert!(loaded
                .validation
                .issues
                .iter()
                .any(|issue| issue.code == code));
        }
    }

    #[test]
    fn non_terminal_stage_without_output_folder_is_invalid() {
        let yaml = r#"
project:
  name: beehive
  workdir: .
stages:
  - id: ingest
    input_folder: stages/incoming
    workflow_url: http://localhost:5678/webhook/ingest
    next_stage: done
  - id: done
    input_folder: stages/done
    workflow_url: http://localhost:5678/webhook/done
"#;

        let loaded = parse_pipeline_config(yaml, "now".to_string());

        assert!(!loaded.validation.is_valid);
        assert!(loaded
            .validation
            .issues
            .iter()
            .any(|issue| issue.code == "missing_stage_output_folder"));
    }

    #[test]
    fn stage9_demo_workdir_fixture_is_valid() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let workdir = repo_root.join("demo").join("workdir");
        let pipeline_path = workdir.join("pipeline.yaml");
        let loaded = load_pipeline_config(&pipeline_path);
        let config = loaded.config.expect("demo config should parse");

        assert!(loaded.validation.is_valid, "{:?}", loaded.validation.issues);
        assert_eq!(config.project.name, "beehive-demo");
        assert!(config
            .stages
            .iter()
            .any(|stage| stage.id == "semantic_split"
                && stage.workflow_url
                    == "https://n8n-dev.steos.io/webhook/b0c81347-5f51-4142-b1d9-18451d8c4ecf"));
        assert!(config
            .stages
            .iter()
            .any(|stage| stage.id == "review" && stage.next_stage.is_none()));

        let incoming = workdir.join("stages").join("incoming");
        let files = std::fs::read_dir(&incoming)
            .expect("demo incoming dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        assert!(files.len() >= 10);

        for file in files {
            let value = serde_json::from_slice::<serde_json::Value>(
                &std::fs::read(file.path()).expect("demo json bytes"),
            )
            .expect("demo json parses");
            assert!(value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .is_some());
            assert_eq!(
                value
                    .get("current_stage")
                    .and_then(serde_json::Value::as_str),
                Some("semantic_split")
            );
            let payload = value
                .get("payload")
                .and_then(serde_json::Value::as_object)
                .expect("payload object");
            assert!(payload.get("entity_name").is_some());
            assert!(payload.get("beehive").is_none());
        }
    }
}
