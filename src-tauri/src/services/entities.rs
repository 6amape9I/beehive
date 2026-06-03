use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::database::{self, RegisterS3ArtifactPointerInput};
use crate::domain::{
    EntityDetailPayload, EntityDetailResult, EntityFileS3JsonPayload, EntityListQuery,
    EntityListResult, EntityMutationPayload, EntityMutationResult, ImportJsonBatchPayload,
    ImportJsonBatchRequest, ImportJsonFileInput, ImportJsonFileResult, ResetEntityStageRequest,
    S3StorageConfig, StageStatus, StorageProvider, UpdateEntityRequest,
};
use crate::s3_client::{AwsS3MetadataClient, S3MetadataClient, S3ObjectMetadata};
use crate::services::workspaces;

trait JsonObjectUploader {
    fn object_exists(&self, bucket: &str, key: &str) -> Result<bool, String>;
    fn put_json_object(
        &self,
        bucket: &str,
        key: &str,
        bytes: Vec<u8>,
        metadata: HashMap<String, String>,
    ) -> Result<S3ObjectMetadata, String>;
}

struct AwsJsonObjectUploader {
    client: AwsS3MetadataClient,
}

trait JsonObjectReader {
    fn get_object_bytes(&self, bucket: &str, key: &str) -> Result<Option<Vec<u8>>, String>;
}

struct AwsJsonObjectReader {
    client: AwsS3MetadataClient,
}

impl JsonObjectUploader for AwsJsonObjectUploader {
    fn object_exists(&self, bucket: &str, key: &str) -> Result<bool, String> {
        Ok(self.client.head_object(bucket, key)?.is_some())
    }

    fn put_json_object(
        &self,
        bucket: &str,
        key: &str,
        bytes: Vec<u8>,
        metadata: HashMap<String, String>,
    ) -> Result<S3ObjectMetadata, String> {
        self.client.put_json_object(bucket, key, bytes, metadata)
    }
}

impl JsonObjectReader for AwsJsonObjectReader {
    fn get_object_bytes(&self, bucket: &str, key: &str) -> Result<Option<Vec<u8>>, String> {
        self.client.get_object_bytes(bucket, key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntityActionError {
    pub(crate) status_code: u16,
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl EntityActionError {
    fn new(status_code: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status_code,
            code,
            message: message.into(),
        }
    }
}

pub(crate) fn list_entities_for_workspace(
    workspace_id: &str,
    query: EntityListQuery,
) -> Result<EntityListResult, String> {
    let workspace = workspaces::get_workspace(workspace_id)?;
    let page = database::list_entity_table_page(&workspace.database_path, &query)?;
    let stages = database::list_stages(&workspace.database_path)?;
    Ok(EntityListResult {
        total: page.total,
        page: page.page,
        page_size: page.page_size,
        available_stages: stages.into_iter().map(|stage| stage.id).collect(),
        available_statuses: page.available_statuses,
        entities: page.entities,
        errors: Vec::new(),
    })
}

pub(crate) fn get_entity_for_workspace(
    workspace_id: &str,
    entity_id: &str,
) -> Result<Option<EntityDetailPayload>, String> {
    let workspace = workspaces::get_workspace(workspace_id)?;
    database::get_entity_detail(&workspace.database_path, entity_id)
}

pub(crate) fn view_entity_file_s3_json_for_workspace(
    workspace_id: &str,
    entity_file_id: i64,
) -> Result<EntityFileS3JsonPayload, EntityActionError> {
    let workspace = workspaces::get_workspace(workspace_id)
        .map_err(|message| EntityActionError::new(404, "workspace_not_found", message))?;
    let storage = workspace_s3_storage(&workspace)
        .map_err(|message| EntityActionError::new(400, "not_s3_artifact", message))?;
    let reader = AwsJsonObjectReader {
        client: AwsS3MetadataClient::from_storage_config(&storage)
            .map_err(|message| EntityActionError::new(500, "s3_read_failed", message))?,
    };
    view_entity_file_s3_json_with_reader(&workspace.database_path, entity_file_id, &reader)
}

fn view_entity_file_s3_json_with_reader(
    database_path: &Path,
    entity_file_id: i64,
    reader: &dyn JsonObjectReader,
) -> Result<EntityFileS3JsonPayload, EntityActionError> {
    let connection = database::open_connection(database_path)
        .map_err(|message| EntityActionError::new(500, "entity_file_lookup_failed", message))?;
    let file = database::find_entity_file_by_id(&connection, entity_file_id)
        .map_err(|message| EntityActionError::new(500, "entity_file_lookup_failed", message))?
        .ok_or_else(|| {
            EntityActionError::new(
                404,
                "entity_file_not_found",
                format!("Entity file id '{entity_file_id}' was not found."),
            )
        })?;
    if file.storage_provider != StorageProvider::S3 {
        return Err(EntityActionError::new(
            400,
            "not_s3_artifact",
            format!("Entity file id '{entity_file_id}' is not an S3 artifact."),
        ));
    }
    let bucket = file.bucket.clone().filter(|value| !value.trim().is_empty());
    let key = file.key.clone().filter(|value| !value.trim().is_empty());
    let (bucket, key) = match (bucket, key) {
        (Some(bucket), Some(key)) => (bucket, key),
        _ => {
            return Err(EntityActionError::new(
                400,
                "not_s3_artifact",
                format!("Entity file id '{entity_file_id}' has no stored S3 bucket/key."),
            ));
        }
    };

    let bytes = reader
        .get_object_bytes(&bucket, &key)
        .map_err(|message| EntityActionError::new(502, "s3_read_failed", message))?
        .ok_or_else(|| {
            EntityActionError::new(
                404,
                "s3_object_not_found",
                format!("S3 object s3://{bucket}/{key} was not found."),
            )
        })?;
    let body = std::str::from_utf8(&bytes).map_err(|error| {
        EntityActionError::new(
            422,
            "s3_json_invalid",
            format!("S3 object s3://{bucket}/{key} is not valid UTF-8 JSON: {error}"),
        )
    })?;
    let json = serde_json::from_str::<Value>(body).map_err(|error| {
        EntityActionError::new(
            422,
            "s3_json_invalid",
            format!("S3 object s3://{bucket}/{key} is not valid JSON: {error}"),
        )
    })?;

    Ok(EntityFileS3JsonPayload {
        entity_file_id,
        entity_id: file.entity_id,
        stage_id: file.stage_id,
        s3_uri: format!("s3://{bucket}/{key}"),
        bucket,
        key,
        json,
    })
}

pub(crate) fn reset_entity_stage_to_pending_for_workspace(
    workspace_id: &str,
    entity_id: &str,
    stage_id: &str,
    input: &ResetEntityStageRequest,
) -> Result<EntityDetailPayload, EntityActionError> {
    if !input.confirm {
        return Err(EntityActionError::new(
            400,
            "reset_confirmation_required",
            "Reset to pending requires confirm=true.",
        ));
    }
    let workspace = workspaces::get_workspace(workspace_id)
        .map_err(|message| EntityActionError::new(404, "workspace_not_found", message))?;
    database::reset_entity_stage_to_pending(
        &workspace.database_path,
        entity_id,
        stage_id,
        input.reason.as_deref(),
    )
    .map_err(reset_error_from_message)?;
    database::get_entity_detail(&workspace.database_path, entity_id)
        .map_err(|message| EntityActionError::new(500, "entity_detail_refresh_failed", message))?
        .ok_or_else(|| {
            EntityActionError::new(
                404,
                "entity_not_found",
                format!("Entity '{entity_id}' was not found after reset."),
            )
        })
}

pub(crate) fn update_entity_for_workspace(
    workspace_id: &str,
    entity_id: &str,
    input: &UpdateEntityRequest,
) -> Result<Option<EntityMutationPayload>, String> {
    let workspace = workspaces::get_workspace(workspace_id)?;
    Ok(
        database::update_entity_metadata(&workspace.database_path, entity_id, input)?
            .map(|entity| EntityMutationPayload { entity }),
    )
}

pub(crate) fn archive_entity_for_workspace(
    workspace_id: &str,
    entity_id: &str,
) -> Result<Option<EntityMutationPayload>, String> {
    let workspace = workspaces::get_workspace(workspace_id)?;
    Ok(
        database::archive_entity(&workspace.database_path, entity_id)?
            .map(|entity| EntityMutationPayload { entity }),
    )
}

pub(crate) fn restore_entity_for_workspace(
    workspace_id: &str,
    entity_id: &str,
) -> Result<Option<EntityMutationPayload>, String> {
    let workspace = workspaces::get_workspace(workspace_id)?;
    Ok(
        database::restore_entity(&workspace.database_path, entity_id)?
            .map(|entity| EntityMutationPayload { entity }),
    )
}

pub(crate) fn entity_detail_result(
    detail: Option<EntityDetailPayload>,
    not_found_message: String,
) -> EntityDetailResult {
    match detail {
        Some(detail) => EntityDetailResult {
            detail: Some(detail),
            errors: Vec::new(),
        },
        None => EntityDetailResult {
            detail: None,
            errors: vec![crate::domain::CommandErrorInfo {
                code: "entity_not_found".to_string(),
                message: not_found_message,
                path: None,
            }],
        },
    }
}

pub(crate) fn entity_mutation_result(
    payload: Option<EntityMutationPayload>,
    not_found_message: String,
) -> EntityMutationResult {
    match payload {
        Some(payload) => EntityMutationResult {
            payload: Some(payload),
            errors: Vec::new(),
        },
        None => EntityMutationResult {
            payload: None,
            errors: vec![crate::domain::CommandErrorInfo {
                code: "entity_not_found".to_string(),
                message: not_found_message,
                path: None,
            }],
        },
    }
}

pub(crate) fn import_json_batch_for_workspace(
    workspace_id: &str,
    input: &ImportJsonBatchRequest,
) -> Result<ImportJsonBatchPayload, String> {
    let workspace = workspaces::get_workspace(workspace_id)?;
    let storage = workspace_s3_storage(&workspace)?;
    let uploader = AwsJsonObjectUploader {
        client: AwsS3MetadataClient::from_storage_config(&storage)?,
    };
    import_json_batch_with_uploader(workspace_id, input, &uploader)
}

fn import_json_batch_with_uploader(
    workspace_id: &str,
    input: &ImportJsonBatchRequest,
    uploader: &dyn JsonObjectUploader,
) -> Result<ImportJsonBatchPayload, String> {
    let workspace = workspaces::get_workspace(workspace_id)?;
    let storage = workspace_s3_storage(&workspace)?;
    let stage_id = normalize_stage_id(&input.stage_id)?;
    let stage = database::list_stages(&workspace.database_path)?
        .into_iter()
        .find(|stage| stage.id == stage_id)
        .ok_or_else(|| format!("Target stage '{stage_id}' was not found."))?;
    if !stage.is_active {
        return Err(format!("Target stage '{stage_id}' is inactive."));
    }

    let overwrite_existing = input.options.overwrite_existing.unwrap_or(false);
    let mut seen_keys = HashSet::new();
    let mut results = Vec::new();

    for file in &input.files {
        match import_one_file(
            &workspace.database_path,
            &storage,
            &stage_id,
            &mut seen_keys,
            overwrite_existing,
            file,
            uploader,
        ) {
            Ok(result) => results.push(result),
            Err(error) => {
                let (status, message) = if let Some(message) = error.strip_prefix("invalid: ") {
                    ("invalid", message.to_string())
                } else {
                    ("failed", error)
                };
                results.push(ImportJsonFileResult {
                    file_name: file.file_name.clone(),
                    status: status.to_string(),
                    entity_id: None,
                    artifact_id: None,
                    bucket: None,
                    key: None,
                    object_key: None,
                    error: Some(message),
                })
            }
        }
    }

    let imported_count = results
        .iter()
        .filter(|result| result.status == "imported")
        .count() as u64;
    let invalid_count = results
        .iter()
        .filter(|result| result.status == "invalid")
        .count() as u64;
    let failed_count = results
        .iter()
        .filter(|result| result.status == "failed")
        .count() as u64;
    let skipped_count = results
        .iter()
        .filter(|result| result.status == "skipped")
        .count() as u64;

    Ok(ImportJsonBatchPayload {
        stage_id,
        uploaded_count: imported_count,
        registered_count: imported_count,
        imported_count,
        invalid_count,
        failed_count,
        skipped_count,
        files: results,
    })
}

fn import_one_file(
    database_path: &std::path::Path,
    storage: &S3StorageConfig,
    stage_id: &str,
    seen_keys: &mut HashSet<String>,
    overwrite_existing: bool,
    file: &ImportJsonFileInput,
    uploader: &dyn JsonObjectUploader,
) -> Result<ImportJsonFileResult, String> {
    if !file.content.is_object() {
        return Err("invalid: JSON file content must be an object.".to_string());
    }
    let file_name = sanitize_json_file_name(&file.file_name)?;
    let bytes = serde_json::to_vec(&file.content)
        .map_err(|error| format!("Failed to serialize JSON content: {error}"))?;
    let checksum = sha256_hex(&bytes);
    let short_hash = checksum.chars().take(12).collect::<String>();
    let entity_id = derive_entity_id(&file.content, &file_name, &short_hash);
    let artifact_id = derive_artifact_id(&file.content, &entity_id, &short_hash);
    let object_key = choose_object_key(
        storage,
        stage_id,
        &file_name,
        &short_hash,
        seen_keys,
        overwrite_existing,
        uploader,
    )?;
    let mut metadata = HashMap::new();
    metadata.insert("beehive-entity-id".to_string(), entity_id.clone());
    metadata.insert("beehive-artifact-id".to_string(), artifact_id.clone());
    metadata.insert("beehive-stage-id".to_string(), stage_id.to_string());

    let uploaded = uploader.put_json_object(&storage.bucket, &object_key, bytes, metadata)?;
    let files = database::register_s3_artifact_pointers(
        database_path,
        &[RegisterS3ArtifactPointerInput {
            entity_id: entity_id.clone(),
            artifact_id: artifact_id.clone(),
            relation_to_source: Some("source".to_string()),
            stage_id: stage_id.to_string(),
            bucket: storage.bucket.clone(),
            key: object_key.clone(),
            version_id: uploaded.version_id,
            etag: uploaded.etag,
            checksum_sha256: Some(checksum),
            size: uploaded.size,
            last_modified: uploaded.last_modified,
            source_file_id: None,
            producer_run_id: None,
            status: StageStatus::Pending,
        }],
    )?;
    let registered = files
        .first()
        .ok_or_else(|| "S3 registration returned no entity file.".to_string())?;

    Ok(ImportJsonFileResult {
        file_name,
        status: "imported".to_string(),
        entity_id: Some(registered.entity_id.clone()),
        artifact_id: registered.artifact_id.clone(),
        bucket: registered.bucket.clone(),
        key: registered.key.clone(),
        object_key: registered.key.clone(),
        error: None,
    })
}

fn workspace_s3_storage(
    workspace: &workspaces::RegisteredWorkspace,
) -> Result<S3StorageConfig, String> {
    if workspace.provider != StorageProvider::S3 {
        return Err(format!(
            "Workspace '{}' is not an S3 workspace.",
            workspace.id
        ));
    }
    Ok(S3StorageConfig {
        bucket: workspace
            .bucket
            .clone()
            .ok_or_else(|| format!("Workspace '{}' has no bucket.", workspace.id))?,
        workspace_prefix: workspace
            .workspace_prefix
            .clone()
            .ok_or_else(|| format!("Workspace '{}' has no workspace_prefix.", workspace.id))?,
        region: workspace.region.clone(),
        endpoint: workspace.endpoint.clone(),
    })
}

fn reset_error_from_message(message: String) -> EntityActionError {
    if let Some(message) = message.strip_prefix("active_worker_lease_exists: ") {
        return EntityActionError::new(409, "active_worker_lease_exists", message);
    }
    if let Some(message) = message.strip_prefix("state_in_progress_cannot_reset: ") {
        return EntityActionError::new(409, "state_in_progress_cannot_reset", message);
    }
    if let Some(message) = message.strip_prefix("state_queued_cannot_reset: ") {
        return EntityActionError::new(409, "state_queued_cannot_reset", message);
    }
    if let Some(message) = message.strip_prefix("state_not_resettable: ") {
        return EntityActionError::new(400, "state_not_resettable", message);
    }
    if message.starts_with("No stage state exists") {
        return EntityActionError::new(404, "entity_stage_state_not_found", message);
    }
    EntityActionError::new(400, "manual_reset_failed", message)
}

fn choose_object_key(
    storage: &S3StorageConfig,
    stage_id: &str,
    file_name: &str,
    short_hash: &str,
    seen_keys: &mut HashSet<String>,
    overwrite_existing: bool,
    uploader: &dyn JsonObjectUploader,
) -> Result<String, String> {
    let prefix = storage.workspace_prefix.trim_matches('/');
    let base_key = format!("{prefix}/stages/{stage_id}/{file_name}");
    if overwrite_existing {
        seen_keys.insert(base_key.clone());
        return Ok(base_key);
    }
    if seen_keys.insert(base_key.clone()) && !uploader.object_exists(&storage.bucket, &base_key)? {
        return Ok(base_key);
    }

    let hashed_name = append_hash_suffix(file_name, short_hash);
    let hashed_key = format!("{prefix}/stages/{stage_id}/{hashed_name}");
    if seen_keys.insert(hashed_key.clone())
        && !uploader.object_exists(&storage.bucket, &hashed_key)?
    {
        return Ok(hashed_key);
    }

    for index in 2..1000 {
        let candidate_name = append_hash_and_index(file_name, short_hash, index);
        let candidate_key = format!("{prefix}/stages/{stage_id}/{candidate_name}");
        if seen_keys.insert(candidate_key.clone())
            && !uploader.object_exists(&storage.bucket, &candidate_key)?
        {
            return Ok(candidate_key);
        }
    }

    Err(format!(
        "Could not choose a non-conflicting S3 object key for '{file_name}'."
    ))
}

fn sanitize_json_file_name(value: &str) -> Result<String, String> {
    let leaf = value
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(value)
        .trim()
        .to_string();
    if leaf.is_empty() {
        return Err("invalid: file_name is required.".to_string());
    }
    if leaf.contains("..") {
        return Err("invalid: file_name must not contain '..'.".to_string());
    }
    let sanitized = leaf
        .chars()
        .map(|ch| {
            if ch.is_control() || ch == '/' || ch == '\\' {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();
    if !sanitized.to_ascii_lowercase().ends_with(".json") {
        return Err("invalid: file_name must end with .json.".to_string());
    }
    Ok(sanitized)
}

fn derive_entity_id(content: &Value, file_name: &str, short_hash: &str) -> String {
    content
        .get("entity_id")
        .and_then(Value::as_str)
        .or_else(|| content.get("id").and_then(Value::as_str))
        .map(sanitize_logical_id)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            let stem = file_stem(file_name);
            let stem = sanitize_logical_id(&stem);
            format!("{stem}_{short_hash}")
        })
}

fn derive_artifact_id(content: &Value, entity_id: &str, short_hash: &str) -> String {
    content
        .get("artifact_id")
        .and_then(Value::as_str)
        .map(sanitize_logical_id)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            if entity_id.is_empty() {
                format!("source__{short_hash}")
            } else {
                format!("{entity_id}__source")
            }
        })
}

fn sanitize_logical_id(value: &str) -> String {
    let mut output = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_control() || ch == '/' || ch == '\\' {
                '_'
            } else if ch.is_whitespace() {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();
    while output.contains("..") {
        output = output.replace("..", ".");
    }
    output.trim_matches(['_', '.', '-']).to_string()
}

fn normalize_stage_id(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("stage_id is required.".to_string());
    }
    Ok(value.to_string())
}

fn append_hash_suffix(file_name: &str, short_hash: &str) -> String {
    let stem = file_stem(file_name);
    format!("{stem}__{short_hash}.json")
}

fn append_hash_and_index(file_name: &str, short_hash: &str, index: u64) -> String {
    let stem = file_stem(file_name);
    format!("{stem}__{short_hash}_{index}.json")
}

fn file_stem(file_name: &str) -> String {
    file_name
        .strip_suffix(".json")
        .or_else(|| file_name.strip_suffix(".JSON"))
        .unwrap_or(file_name)
        .to_string()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::fs;

    use crate::discovery::scan_workspace;
    use crate::domain::{PipelineConfig, ProjectConfig, RuntimeConfig, StageDefinition};

    struct MockJsonObjectReader {
        response: Result<Option<Vec<u8>>, String>,
        calls: RefCell<Vec<(String, String)>>,
    }

    impl MockJsonObjectReader {
        fn with_bytes(bytes: Vec<u8>) -> Self {
            Self {
                response: Ok(Some(bytes)),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn missing() -> Self {
            Self {
                response: Ok(None),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl JsonObjectReader for MockJsonObjectReader {
        fn get_object_bytes(&self, bucket: &str, key: &str) -> Result<Option<Vec<u8>>, String> {
            self.calls
                .borrow_mut()
                .push((bucket.to_string(), key.to_string()));
            self.response.clone()
        }
    }

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

    fn stage(id: &str) -> StageDefinition {
        StageDefinition {
            id: id.to_string(),
            input_folder: format!("stages/{id}"),
            input_uri: None,
            output_folder: format!("stages/{id}-out"),
            workflow_url: format!("http://localhost:5678/webhook/{id}"),
            max_attempts: 3,
            retry_delay_sec: 10,
            next_stage: None,
            save_path_aliases: Vec::new(),
            resource_class: Default::default(),
            allow_empty_outputs: false,
            allow_multiple_outputs: false,
        }
    }

    fn s3_stage(id: &str) -> StageDefinition {
        StageDefinition {
            input_folder: String::new(),
            input_uri: Some(format!("s3://steos-s3-data/main_dir/{id}")),
            output_folder: String::new(),
            ..stage(id)
        }
    }

    fn register_s3_file(database_path: &Path) -> crate::domain::EntityFileRecord {
        database::register_s3_artifact_pointers(
            database_path,
            &[RegisterS3ArtifactPointerInput {
                entity_id: "entity-alpha".to_string(),
                artifact_id: "artifact-alpha".to_string(),
                relation_to_source: Some("source".to_string()),
                stage_id: "raw_entities".to_string(),
                bucket: "steos-s3-data".to_string(),
                key: "main_dir/raw_entities/entity-alpha.json".to_string(),
                version_id: None,
                etag: None,
                checksum_sha256: None,
                size: Some(42),
                last_modified: None,
                source_file_id: None,
                producer_run_id: None,
                status: StageStatus::Pending,
            }],
        )
        .expect("register s3 file")
        .remove(0)
    }

    #[test]
    fn sanitizes_json_file_name_without_losing_cyrillic() {
        assert_eq!(
            sanitize_json_file_name("Папка/Сущность 1.json").expect("name"),
            "Сущность 1.json"
        );
        assert!(sanitize_json_file_name("bad..name.json").is_err());
        assert!(sanitize_json_file_name("entity.txt").is_err());
    }

    #[test]
    fn derives_ids_from_content_or_filename() {
        let content = serde_json::json!({"id": "entity 1", "artifact_id": "artifact/1"});
        assert_eq!(
            derive_entity_id(&content, "fallback.json", "abcdef"),
            "entity_1"
        );
        assert_eq!(
            derive_artifact_id(&content, "entity_1", "abcdef"),
            "artifact_1"
        );
        let fallback = serde_json::json!({"name": "No id"});
        assert_eq!(
            derive_entity_id(&fallback, "Файл сущности.json", "abcdef123456"),
            "Файл_сущности_abcdef123456"
        );
        assert_eq!(
            derive_artifact_id(&fallback, "entity_1", "abcdef"),
            "entity_1__source"
        );
    }

    #[test]
    fn s3_json_view_reads_registered_bucket_key_and_returns_json() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        database::bootstrap_database(&database_path, &test_config(vec![s3_stage("raw_entities")]))
            .expect("bootstrap");
        let file = register_s3_file(&database_path);
        let reader =
            MockJsonObjectReader::with_bytes(br#"{"id":"entity-alpha","value":42}"#.to_vec());

        let payload =
            view_entity_file_s3_json_with_reader(&database_path, file.id, &reader).expect("json");

        assert_eq!(payload.entity_file_id, file.id);
        assert_eq!(
            payload.s3_uri,
            "s3://steos-s3-data/main_dir/raw_entities/entity-alpha.json"
        );
        assert_eq!(payload.json["value"].as_i64(), Some(42));
        assert_eq!(
            reader.calls.borrow().as_slice(),
            &[(
                "steos-s3-data".to_string(),
                "main_dir/raw_entities/entity-alpha.json".to_string()
            )]
        );
    }

    #[test]
    fn s3_json_view_rejects_missing_and_non_s3_files() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        database::bootstrap_database(&database_path, &test_config(vec![stage("incoming")]))
            .expect("bootstrap");
        let source_path = workdir
            .join("stages")
            .join("incoming")
            .join("entity-1.json");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("parent");
        fs::write(&source_path, r#"{"id":"entity-1"}"#).expect("source");
        scan_workspace(&workdir, &database_path).expect("scan");
        let file = database::list_entity_files(&database_path, Some("entity-1"))
            .expect("files")
            .remove(0);
        let reader = MockJsonObjectReader::missing();

        let missing = view_entity_file_s3_json_with_reader(&database_path, 9999, &reader)
            .expect_err("missing");
        let non_s3 = view_entity_file_s3_json_with_reader(&database_path, file.id, &reader)
            .expect_err("non s3");

        assert_eq!(missing.code, "entity_file_not_found");
        assert_eq!(missing.status_code, 404);
        assert_eq!(non_s3.code, "not_s3_artifact");
        assert!(reader.calls.borrow().is_empty());
    }

    #[test]
    fn s3_json_view_reports_s3_missing_and_invalid_json() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        database::bootstrap_database(&database_path, &test_config(vec![s3_stage("raw_entities")]))
            .expect("bootstrap");
        let file = register_s3_file(&database_path);

        let missing = view_entity_file_s3_json_with_reader(
            &database_path,
            file.id,
            &MockJsonObjectReader::missing(),
        )
        .expect_err("missing object");
        let invalid = view_entity_file_s3_json_with_reader(
            &database_path,
            file.id,
            &MockJsonObjectReader::with_bytes(b"not json".to_vec()),
        )
        .expect_err("invalid json");

        assert_eq!(missing.code, "s3_object_not_found");
        assert_eq!(invalid.code, "s3_json_invalid");
    }

    #[test]
    fn s3_json_view_preserves_cyrillic_json() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        database::bootstrap_database(&database_path, &test_config(vec![s3_stage("raw_entities")]))
            .expect("bootstrap");
        let file = register_s3_file(&database_path);
        let bytes = serde_json::to_vec(&serde_json::json!({
            "name": "Кольца Кайзера-Флейшера",
            "payload": { "язык": "русский" }
        }))
        .expect("json bytes");

        let payload = view_entity_file_s3_json_with_reader(
            &database_path,
            file.id,
            &MockJsonObjectReader::with_bytes(bytes),
        )
        .expect("json");

        assert_eq!(
            payload.json["name"].as_str(),
            Some("Кольца Кайзера-Флейшера")
        );
        assert_eq!(payload.json["payload"]["язык"].as_str(), Some("русский"));
    }
}
