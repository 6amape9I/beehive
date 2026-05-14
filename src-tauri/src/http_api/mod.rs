#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::domain::{
    CreateS3StageRequest, RegisterS3SourceArtifactRequest, RunDueTasksResult,
    RunPipelineWavesResult, S3ReconciliationResult, StageRunOutputsResult,
    UpdateStageNextStageRequest, UpdateStageNextStageResult, WorkspaceRegistryEntryResult,
    WorkspaceRegistryListResult,
};
use crate::services::{artifacts, pipeline, runtime, workspaces};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HttpApiResponse {
    pub status_code: u16,
    pub body: Value,
}

#[derive(Debug, Clone, Deserialize)]
struct RunSmallBatchBody {
    max_tasks: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct RunPipelineWavesBody {
    max_waves: Option<u64>,
    max_tasks_per_wave: Option<u64>,
    stop_on_first_failure: Option<bool>,
}

pub fn handle_json_request(method: &str, path: &str, body: Option<&str>) -> HttpApiResponse {
    match route_json_request(method, path, body) {
        Ok(response) => response,
        Err((status_code, code, message)) => error_response(status_code, code, message),
    }
}

fn route_json_request(
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<HttpApiResponse, (u16, &'static str, String)> {
    let parts = path
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if method == "GET" && parts == ["api", "health"] {
        return Ok(json_response(200, json!({ "status": "ok" })));
    }
    if method == "GET" && parts == ["api", "workspaces"] {
        let result = match workspaces::list_workspace_descriptors() {
            Ok(workspaces) => WorkspaceRegistryListResult {
                workspaces,
                errors: Vec::new(),
            },
            Err(message) => WorkspaceRegistryListResult {
                workspaces: Vec::new(),
                errors: vec![command_error("workspace_registry_failed", message)],
            },
        };
        return Ok(json_response(200, serde_json::to_value(result).unwrap()));
    }
    if parts.len() >= 3 && parts[0] == "api" && parts[1] == "workspaces" {
        let workspace_id = parts[2];
        match (method, parts.as_slice()) {
            ("GET", ["api", "workspaces", _]) => {
                let result = match workspaces::get_workspace_descriptor(workspace_id) {
                    Ok(workspace) => WorkspaceRegistryEntryResult {
                        workspace: Some(workspace),
                        errors: Vec::new(),
                    },
                    Err(message) => WorkspaceRegistryEntryResult {
                        workspace: None,
                        errors: vec![command_error("workspace_not_found", message)],
                    },
                };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("GET", ["api", "workspaces", _, "workspace-explorer"]) => {
                let result = runtime::workspace_explorer(workspace_id)
                    .map_err(|message| (400, "workspace_explorer_failed", message))?;
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("POST", ["api", "workspaces", _, "reconcile-s3"]) => {
                let result = match runtime::reconcile_s3_workspace(workspace_id) {
                    Ok(summary) => S3ReconciliationResult {
                        summary: Some(summary),
                        errors: Vec::new(),
                    },
                    Err(message) => S3ReconciliationResult {
                        summary: None,
                        errors: vec![command_error("s3_reconciliation_failed", message)],
                    },
                };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("POST", ["api", "workspaces", _, "register-s3-source"]) => {
                let input = parse_body::<RegisterS3SourceArtifactRequest>(body)?;
                let result = runtime::register_s3_source_artifact(workspace_id, &input);
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("POST", ["api", "workspaces", _, "run-small-batch"]) => {
                let input = parse_optional_body::<RunSmallBatchBody>(body)?;
                let result =
                    match runtime::run_small_batch(workspace_id, input.max_tasks.unwrap_or(3)) {
                        Ok(summary) => RunDueTasksResult {
                            summary: Some(summary),
                            errors: Vec::new(),
                        },
                        Err(message) => RunDueTasksResult {
                            summary: None,
                            errors: vec![command_error("run_due_tasks_limited_failed", message)],
                        },
                    };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("POST", ["api", "workspaces", _, "run-pipeline-waves"]) => {
                let input = parse_optional_body::<RunPipelineWavesBody>(body)?;
                let result = match runtime::run_pipeline_waves(
                    workspace_id,
                    input.max_waves.unwrap_or(5),
                    input.max_tasks_per_wave.unwrap_or(3),
                    input.stop_on_first_failure.unwrap_or(true),
                ) {
                    Ok(summary) => RunPipelineWavesResult {
                        summary: Some(summary),
                        errors: Vec::new(),
                    },
                    Err(message) => RunPipelineWavesResult {
                        summary: None,
                        errors: vec![command_error("run_pipeline_waves_failed", message)],
                    },
                };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("POST", ["api", "workspaces", _, "stages"]) => {
                let input = parse_body::<CreateS3StageRequest>(body)?;
                let result = match pipeline::create_s3_stage_for_workspace(workspace_id, &input) {
                    Ok(payload) => crate::domain::CreateS3StageResult {
                        payload: Some(payload),
                        errors: Vec::new(),
                    },
                    Err(message) => crate::domain::CreateS3StageResult {
                        payload: None,
                        errors: vec![command_error("create_s3_stage_failed", message)],
                    },
                };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("POST", ["api", "workspaces", _, "stages", stage_id, "next-stage"]) => {
                let input = parse_body::<UpdateStageNextStageRequest>(body)?;
                let result = match pipeline::update_stage_next_stage_for_workspace(
                    workspace_id,
                    stage_id,
                    &input,
                ) {
                    Ok(payload) => UpdateStageNextStageResult {
                        payload: Some(payload),
                        errors: Vec::new(),
                    },
                    Err(message) => UpdateStageNextStageResult {
                        payload: None,
                        errors: vec![command_error("update_stage_next_stage_failed", message)],
                    },
                };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("GET", ["api", "workspaces", _, "stage-runs", run_id, "outputs"]) => {
                let result =
                    match artifacts::list_stage_run_outputs_for_workspace(workspace_id, run_id) {
                        Ok(payload) => StageRunOutputsResult {
                            payload: Some(payload),
                            errors: Vec::new(),
                        },
                        Err(message) => StageRunOutputsResult {
                            payload: None,
                            errors: vec![command_error("stage_run_outputs_failed", message)],
                        },
                    };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            _ => Err((
                404,
                "route_not_found",
                format!("No route for {method} {path}"),
            )),
        }
    } else {
        Err((
            404,
            "route_not_found",
            format!("No route for {method} {path}"),
        ))
    }
}

fn parse_body<T: for<'de> Deserialize<'de>>(
    body: Option<&str>,
) -> Result<T, (u16, &'static str, String)> {
    let Some(body) = body else {
        return Err((
            400,
            "request_body_missing",
            "JSON request body is required.".to_string(),
        ));
    };
    serde_json::from_str(body).map_err(|error| {
        (
            400,
            "request_body_invalid",
            format!("Invalid JSON body: {error}"),
        )
    })
}

fn parse_optional_body<T: for<'de> Deserialize<'de> + Default>(
    body: Option<&str>,
) -> Result<T, (u16, &'static str, String)> {
    match body {
        Some(body) if !body.trim().is_empty() => serde_json::from_str(body).map_err(|error| {
            (
                400,
                "request_body_invalid",
                format!("Invalid JSON body: {error}"),
            )
        }),
        _ => Ok(T::default()),
    }
}

fn json_response(status_code: u16, body: Value) -> HttpApiResponse {
    HttpApiResponse { status_code, body }
}

fn error_response(status_code: u16, code: &'static str, message: String) -> HttpApiResponse {
    json_response(
        status_code,
        json!({
            "errors": [command_error(code, message)]
        }),
    )
}

fn command_error(code: &str, message: impl Into<String>) -> crate::domain::CommandErrorInfo {
    crate::domain::CommandErrorInfo {
        code: code.to_string(),
        message: message.into(),
        path: None,
    }
}

impl Default for RunSmallBatchBody {
    fn default() -> Self {
        Self { max_tasks: Some(3) }
    }
}

impl Default for RunPipelineWavesBody {
    fn default() -> Self {
        Self {
            max_waves: Some(5),
            max_tasks_per_wave: Some(3),
            stop_on_first_failure: Some(true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_route_returns_ok() {
        let response = handle_json_request("GET", "/api/health", None);
        assert_eq!(response.status_code, 200);
        assert_eq!(response.body["status"].as_str(), Some("ok"));
    }

    #[test]
    fn workspace_list_route_returns_registry_result() {
        let response = handle_json_request("GET", "/api/workspaces", None);
        assert_eq!(response.status_code, 200);
        assert!(response.body["workspaces"].is_array());
    }

    #[test]
    fn missing_route_returns_json_error() {
        let response = handle_json_request("GET", "/api/missing", None);
        assert_eq!(response.status_code, 404);
        assert_eq!(
            response.body["errors"][0]["code"].as_str(),
            Some("route_not_found")
        );
    }
}
