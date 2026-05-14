#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::domain::{
    CreateS3StageRequest, CreateWorkspaceRequest, EntityDetailResult, EntityListQuery,
    EntityListResult, EntityMutationResult, ImportJsonBatchRequest, ImportJsonBatchResult,
    RegisterS3SourceArtifactRequest, RunDueTasksResult, RunPipelineWavesResult,
    RunSelectedPipelineWavesRequest, RunSelectedPipelineWavesResult, S3ReconciliationResult,
    S3StageMutationResult, StageRunOutputsResult, UpdateEntityRequest, UpdateS3StageRequest,
    UpdateStageNextStageResult, UpdateWorkspaceRequest, WorkspaceMutationResult,
    WorkspaceRegistryEntryResult, WorkspaceRegistryListResult,
};
use crate::services::{artifacts, entities, pipeline, runtime, selected_runner, workspaces};

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
    let (path_only, query) = split_path_query(path);
    let parts = path_only
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if method == "GET" && parts == ["api", "health"] {
        return Ok(json_response(200, json!({ "status": "ok" })));
    }
    if method == "GET" && parts == ["api", "workspaces"] {
        let include_archived = query_bool(query, "include_archived").unwrap_or(false);
        let result = match workspaces::list_workspace_descriptors(include_archived) {
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
    if method == "POST" && parts == ["api", "workspaces"] {
        let input = parse_body::<CreateWorkspaceRequest>(body)?;
        let result = match workspaces::create_workspace(&input) {
            Ok(payload) => WorkspaceMutationResult {
                payload: Some(payload),
                errors: Vec::new(),
            },
            Err(message) => WorkspaceMutationResult {
                payload: None,
                errors: vec![command_error("create_workspace_failed", message)],
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
            ("PATCH", ["api", "workspaces", _]) => {
                let input = parse_body::<UpdateWorkspaceRequest>(body)?;
                let result = match workspaces::update_workspace(workspace_id, &input) {
                    Ok(payload) => WorkspaceMutationResult {
                        payload: Some(payload),
                        errors: Vec::new(),
                    },
                    Err(message) => WorkspaceMutationResult {
                        payload: None,
                        errors: vec![command_error("update_workspace_failed", message)],
                    },
                };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("DELETE", ["api", "workspaces", _]) => {
                let result = match workspaces::archive_or_delete_workspace(workspace_id) {
                    Ok(payload) => WorkspaceMutationResult {
                        payload: Some(payload),
                        errors: Vec::new(),
                    },
                    Err(message) => WorkspaceMutationResult {
                        payload: None,
                        errors: vec![command_error("delete_workspace_failed", message)],
                    },
                };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("POST", ["api", "workspaces", _, "restore"]) => {
                let result = match workspaces::restore_workspace(workspace_id) {
                    Ok(payload) => WorkspaceMutationResult {
                        payload: Some(payload),
                        errors: Vec::new(),
                    },
                    Err(message) => WorkspaceMutationResult {
                        payload: None,
                        errors: vec![command_error("restore_workspace_failed", message)],
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
            ("GET", ["api", "workspaces", _, "entities"]) => {
                let query = entity_list_query_from_query(query);
                let result = match entities::list_entities_for_workspace(workspace_id, query) {
                    Ok(result) => result,
                    Err(message) => EntityListResult {
                        entities: Vec::new(),
                        total: 0,
                        page: 1,
                        page_size: 50,
                        available_stages: Vec::new(),
                        available_statuses: Vec::new(),
                        errors: vec![command_error("list_entities_failed", message)],
                    },
                };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("POST", ["api", "workspaces", _, "entities", "import-json-batch"]) => {
                let input = parse_body::<ImportJsonBatchRequest>(body)?;
                let result = match entities::import_json_batch_for_workspace(workspace_id, &input) {
                    Ok(payload) => ImportJsonBatchResult {
                        payload: Some(payload),
                        errors: Vec::new(),
                    },
                    Err(message) => ImportJsonBatchResult {
                        payload: None,
                        errors: vec![command_error("import_json_batch_failed", message)],
                    },
                };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("GET", ["api", "workspaces", _, "entities", entity_id]) => {
                let result = match entities::get_entity_for_workspace(workspace_id, entity_id) {
                    Ok(detail) => entities::entity_detail_result(
                        detail,
                        format!("Entity '{entity_id}' was not found."),
                    ),
                    Err(message) => EntityDetailResult {
                        detail: None,
                        errors: vec![command_error("get_entity_failed", message)],
                    },
                };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("PATCH", ["api", "workspaces", _, "entities", entity_id]) => {
                let input = parse_body::<UpdateEntityRequest>(body)?;
                let result =
                    match entities::update_entity_for_workspace(workspace_id, entity_id, &input) {
                        Ok(payload) => entities::entity_mutation_result(
                            payload,
                            format!("Entity '{entity_id}' was not found."),
                        ),
                        Err(message) => EntityMutationResult {
                            payload: None,
                            errors: vec![command_error("update_entity_failed", message)],
                        },
                    };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("DELETE", ["api", "workspaces", _, "entities", entity_id]) => {
                let result = match entities::archive_entity_for_workspace(workspace_id, entity_id) {
                    Ok(payload) => entities::entity_mutation_result(
                        payload,
                        format!("Entity '{entity_id}' was not found."),
                    ),
                    Err(message) => EntityMutationResult {
                        payload: None,
                        errors: vec![command_error("archive_entity_failed", message)],
                    },
                };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("POST", ["api", "workspaces", _, "entities", entity_id, "restore"]) => {
                let result = match entities::restore_entity_for_workspace(workspace_id, entity_id) {
                    Ok(payload) => entities::entity_mutation_result(
                        payload,
                        format!("Entity '{entity_id}' was not found."),
                    ),
                    Err(message) => EntityMutationResult {
                        payload: None,
                        errors: vec![command_error("restore_entity_failed", message)],
                    },
                };
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
            ("POST", ["api", "workspaces", _, "run-selected-pipeline-waves"]) => {
                let input = parse_body::<RunSelectedPipelineWavesRequest>(body)?;
                let result =
                    match selected_runner::run_selected_pipeline_waves(workspace_id, &input) {
                        Ok(summary) => RunSelectedPipelineWavesResult {
                            summary: Some(summary),
                            errors: Vec::new(),
                        },
                        Err(message) => RunSelectedPipelineWavesResult {
                            summary: None,
                            errors: vec![command_error(
                                "run_selected_pipeline_waves_failed",
                                message,
                            )],
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
            ("PATCH", ["api", "workspaces", _, "stages", stage_id]) => {
                let input = parse_body::<UpdateS3StageRequest>(body)?;
                let result =
                    match pipeline::update_s3_stage_for_workspace(workspace_id, stage_id, &input) {
                        Ok(payload) => S3StageMutationResult {
                            payload: Some(payload),
                            errors: Vec::new(),
                        },
                        Err(message) => S3StageMutationResult {
                            payload: None,
                            errors: vec![command_error("update_s3_stage_failed", message)],
                        },
                    };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("DELETE", ["api", "workspaces", _, "stages", stage_id]) => {
                let result =
                    match pipeline::archive_or_delete_stage_for_workspace(workspace_id, stage_id) {
                        Ok(payload) => S3StageMutationResult {
                            payload: Some(payload),
                            errors: Vec::new(),
                        },
                        Err(message) => S3StageMutationResult {
                            payload: None,
                            errors: vec![command_error("delete_s3_stage_failed", message)],
                        },
                    };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("POST", ["api", "workspaces", _, "stages", stage_id, "restore"]) => {
                let result = match pipeline::restore_stage_for_workspace(workspace_id, stage_id) {
                    Ok(payload) => S3StageMutationResult {
                        payload: Some(payload),
                        errors: Vec::new(),
                    },
                    Err(message) => S3StageMutationResult {
                        payload: None,
                        errors: vec![command_error("restore_s3_stage_failed", message)],
                    },
                };
                Ok(json_response(200, serde_json::to_value(result).unwrap()))
            }
            ("POST", ["api", "workspaces", _, "stages", _, "next-stage"]) => {
                let result = UpdateStageNextStageResult {
                    payload: None,
                    errors: vec![command_error(
                        "next_stage_deprecated",
                        "next_stage is deprecated. Route outputs through n8n save_path."
                            .to_string(),
                    )],
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

fn split_path_query(path: &str) -> (&str, Option<&str>) {
    match path.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (path, None),
    }
}

fn query_bool(query: Option<&str>, key: &str) -> Option<bool> {
    let query = query?;
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if name == key {
            return Some(matches!(value, "true" | "1" | "yes"));
        }
    }
    None
}

fn query_param(query: Option<&str>, key: &str) -> Option<String> {
    let query = query?;
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if name == key {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn query_u64(query: Option<&str>, key: &str) -> Option<u64> {
    query_param(query, key).and_then(|value| value.parse::<u64>().ok())
}

fn entity_list_query_from_query(query: Option<&str>) -> EntityListQuery {
    EntityListQuery {
        search: query_param(query, "search"),
        stage_id: query_param(query, "stage_id"),
        status: query_param(query, "status"),
        include_archived: query_bool(query, "include_archived"),
        limit: query_u64(query, "limit"),
        offset: query_u64(query, "offset"),
        page: query_u64(query, "page"),
        page_size: query_u64(query, "page_size"),
        sort_by: query_param(query, "sort_by"),
        sort_direction: query_param(query, "sort_direction"),
        ..EntityListQuery::default()
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
    fn workspace_crud_routes_parse_request_bodies() {
        let create = handle_json_request(
            "POST",
            "/api/workspaces",
            Some(
                r#"{"id":"bad/path","name":"Pilot","bucket":"bucket","workspace_prefix":"prefix","region":"ru-1","endpoint":"https://s3.example"}"#,
            ),
        );
        assert_eq!(create.status_code, 200);
        assert_eq!(
            create.body["errors"][0]["code"].as_str(),
            Some("create_workspace_failed")
        );

        let update = handle_json_request(
            "PATCH",
            "/api/workspaces/missing",
            Some(r#"{"name":"Updated","endpoint":"https://s3.example"}"#),
        );
        assert_eq!(update.status_code, 200);
        assert_eq!(
            update.body["errors"][0]["code"].as_str(),
            Some("update_workspace_failed")
        );

        let delete = handle_json_request("DELETE", "/api/workspaces/missing", None);
        assert_eq!(delete.status_code, 200);
        assert_eq!(
            delete.body["errors"][0]["code"].as_str(),
            Some("delete_workspace_failed")
        );

        let restore = handle_json_request("POST", "/api/workspaces/missing/restore", None);
        assert_eq!(restore.status_code, 200);
        assert_eq!(
            restore.body["errors"][0]["code"].as_str(),
            Some("restore_workspace_failed")
        );
    }

    #[test]
    fn stage_crud_routes_parse_request_bodies() {
        let update = handle_json_request(
            "PATCH",
            "/api/workspaces/missing/stages/stage_a",
            Some(
                r#"{"workflow_url":"https://n8n.example/webhook/a","max_attempts":5,"retry_delay_sec":90,"allow_empty_outputs":true,"next_stage":null}"#,
            ),
        );
        assert_eq!(update.status_code, 200);
        assert_eq!(
            update.body["errors"][0]["code"].as_str(),
            Some("update_s3_stage_failed")
        );

        let delete = handle_json_request("DELETE", "/api/workspaces/missing/stages/stage_a", None);
        assert_eq!(delete.status_code, 200);
        assert_eq!(
            delete.body["errors"][0]["code"].as_str(),
            Some("delete_s3_stage_failed")
        );

        let restore = handle_json_request(
            "POST",
            "/api/workspaces/missing/stages/stage_a/restore",
            None,
        );
        assert_eq!(restore.status_code, 200);
        assert_eq!(
            restore.body["errors"][0]["code"].as_str(),
            Some("restore_s3_stage_failed")
        );

        let next_stage = handle_json_request(
            "POST",
            "/api/workspaces/missing/stages/stage_a/next-stage",
            Some(r#"{"next_stage":"stage_b"}"#),
        );
        assert_eq!(next_stage.status_code, 200);
        assert_eq!(
            next_stage.body["errors"][0]["code"].as_str(),
            Some("next_stage_deprecated")
        );
    }

    #[test]
    fn entity_crud_routes_parse_request_bodies() {
        let list = handle_json_request(
            "GET",
            "/api/workspaces/missing/entities?include_archived=true&limit=5&offset=0",
            None,
        );
        assert_eq!(list.status_code, 200);
        assert_eq!(
            list.body["errors"][0]["code"].as_str(),
            Some("list_entities_failed")
        );

        let update = handle_json_request(
            "PATCH",
            "/api/workspaces/missing/entities/entity-1",
            Some(r#"{"operator_note":"reviewed","display_name":"Entity One"}"#),
        );
        assert_eq!(update.status_code, 200);
        assert_eq!(
            update.body["errors"][0]["code"].as_str(),
            Some("update_entity_failed")
        );

        let archive =
            handle_json_request("DELETE", "/api/workspaces/missing/entities/entity-1", None);
        assert_eq!(archive.status_code, 200);
        assert_eq!(
            archive.body["errors"][0]["code"].as_str(),
            Some("archive_entity_failed")
        );

        let restore = handle_json_request(
            "POST",
            "/api/workspaces/missing/entities/entity-1/restore",
            None,
        );
        assert_eq!(restore.status_code, 200);
        assert_eq!(
            restore.body["errors"][0]["code"].as_str(),
            Some("restore_entity_failed")
        );

        let import = handle_json_request(
            "POST",
            "/api/workspaces/missing/entities/import-json-batch",
            Some(
                r#"{"stage_id":"raw","files":[{"file_name":"a.json","content":{"id":"a"}}],"options":{"overwrite_existing":false}}"#,
            ),
        );
        assert_eq!(import.status_code, 200);
        assert_eq!(
            import.body["errors"][0]["code"].as_str(),
            Some("import_json_batch_failed")
        );
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

    #[test]
    fn selected_pipeline_waves_route_parses_request_body() {
        let response = handle_json_request(
            "POST",
            "/api/workspaces/missing/run-selected-pipeline-waves",
            Some(
                r#"{"root_entity_file_ids":[1],"max_waves":2,"max_tasks_per_wave":1,"stop_on_first_failure":true}"#,
            ),
        );
        assert_eq!(response.status_code, 200);
        assert_eq!(
            response.body["errors"][0]["code"].as_str(),
            Some("run_selected_pipeline_waves_failed")
        );
        assert!(response.body["errors"][0]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("missing"));
    }
}
