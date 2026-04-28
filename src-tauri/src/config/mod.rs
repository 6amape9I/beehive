use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::domain::{
    ConfigValidationIssue, ConfigValidationResult, PipelineConfig, ProjectConfig, RuntimeConfig,
    StageDefinition, ValidationSeverity,
};

#[derive(Debug, Deserialize)]
struct RawPipelineConfig {
    project: Option<RawProjectConfig>,
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
struct RawStageDefinition {
    id: Option<String>,
    input_folder: Option<String>,
    output_folder: Option<String>,
    workflow_url: Option<String>,
    max_attempts: Option<i64>,
    retry_delay_sec: Option<i64>,
    next_stage: Option<String>,
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
        Some(stages) => build_stages(stages, &mut issues),
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
                    runtime,
                    stages,
                }),
                validation,
            );
        }
    }

    (None, validation)
}

fn build_stages(
    raw_stages: Vec<RawStageDefinition>,
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
        let input_folder = required_string(
            raw_stage.input_folder,
            "missing_stage_input_folder",
            &format!("{prefix}.input_folder"),
            issues,
        );
        let output_folder = normalize_optional_string(raw_stage.output_folder);
        let next_stage = normalize_optional_string(raw_stage.next_stage);
        if next_stage.is_some() && output_folder.is_none() {
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

        if let (Some(id), Some(input_folder), Some(workflow_url)) = (id, input_folder, workflow_url)
        {
            stages.push(StageDefinition {
                id,
                input_folder,
                output_folder: output_folder.unwrap_or_default(),
                workflow_url,
                max_attempts: max_attempts.max(1) as u64,
                retry_delay_sec: retry_delay_sec.max(0) as u64,
                next_stage,
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
