# Beehive S3 Control Plane Architecture

## Purpose

B1 moves Beehive toward a storage-aware control plane for S3+n8n pipelines. n8n remains the data-plane transformer. S3 stores business artifacts. Beehive owns runtime state, routing validation, attempts, lineage, and operator visibility.

B2.2 uses JSON control envelope body.
Headers are deprecated for S3 object keys and should not be used for source_key.
The control envelope is technical metadata, not business JSON.

## Control/Data Split

Beehive does:

- stores pipeline and stage config;
- stores artifact pointers;
- claims one eligible artifact for a stage run;
- creates `run_id`;
- triggers n8n with a technical S3 control envelope JSON body;
- validates the technical manifest returned by n8n;
- registers child artifact pointers;
- updates `entity_stage_states`, `stage_runs`, and `app_events`.

n8n does:

- receives a concrete artifact pointer from Beehive;
- downloads business JSON from S3;
- transforms it;
- writes output business JSON to S3;
- returns a technical manifest.

S3 does:

- stores source, intermediate, and final business artifacts;
- may store run/error manifests later;
- does not act as the runtime source of truth.

## Storage Model

B1 adds explicit storage metadata instead of treating S3 keys as local paths:

- `StorageProvider`: `local` or `s3`;
- `ArtifactLocation`: local path or S3 bucket/key plus optional version/etag/checksum/size;
- pipeline `storage`: provider, bucket, workspace prefix, optional region/endpoint;
- stage `input_uri`: S3 input prefix;
- stage `save_path_aliases`: logical route aliases.

SQLite remains the MVP control-plane database. Schema v6 extends existing tables:

- `stages.input_uri`;
- `stages.save_path_aliases_json`;
- `stages.allow_empty_outputs`;
- `entity_files.artifact_id`;
- `entity_files.relation_to_source`;
- `entity_files.storage_provider`;
- `entity_files.bucket`;
- `entity_files.object_key`;
- `entity_files.version_id`;
- `entity_files.etag`;
- `entity_files.checksum_sha256`;
- `entity_files.artifact_size`;
- `entity_files.producer_run_id`.

Local fields remain present for backward compatibility.

`entity_id` remains the logical Beehive entity identifier used by `entities` and `entity_stage_states`. `artifact_id` is the physical output identifier from one producer run and is stored separately on `entity_files`. S3 output registration never falls back from missing `entity_id` to `artifact_id`.

## Runtime Flow

S3 execution path:

1. `entity_stage_states` claim moves an S3 artifact to `queued`.
2. Beehive creates a `stage_runs` row with a technical audit envelope.
3. Beehive sends a JSON `beehive.s3_control_envelope.v1` body with bucket/key, source identity, route hints, and manifest prefix.
4. n8n returns `beehive.s3_artifact_manifest.v1`.
5. Beehive validates schema, run id, source, output bucket, output route, output key prefix, `entity_id`, `artifact_id`, and `relation_to_source`.
6. Valid outputs become S3 artifact pointer rows in one SQLite transaction.
7. Source state becomes `done`; child states become `pending`.

Local execution path remains the B0 payload-only behavior.

B2.2 changed S3 mode away from `X-Beehive-*` pointer headers for source object keys. Headers are deprecated for S3 `source_key` because real object keys may contain Cyrillic and other non-ASCII characters. The control envelope is technical metadata, not business JSON, and must not include source document bodies or `payload_json`.

Zero-output success manifests are valid only when the source stage has `allow_empty_outputs = true`. The default is false even if a stage has no `next_stage`.

S3 registration is idempotent for the same `producer_run_id + artifact_id + bucket/key` and rejects conflicting replays. Duplicate `artifact_id` values inside one manifest are invalid. A `same_entity` output must use the source logical `entity_id`; child or representation outputs still need an explicit `entity_id`.

## B2 S3 Reconciliation

B2 adds a real metadata-only S3 reconciliation path. It uses the official AWS Rust SDK through a small internal `S3MetadataClient` trait so unit tests can run with mocks and without credentials.

The reconciliation command:

- loads active stages that have an S3 `input_uri`;
- lists objects under each bucket/prefix;
- heads objects for S3 user metadata and object metadata;
- registers objects that expose Beehive identity metadata;
- records objects without Beehive identity as `s3_artifact_unmapped` events;
- marks tracked S3 artifacts missing when absent from the current prefix listing;
- restores previously missing S3 artifacts when they reappear.

Reconciliation does not read S3 business JSON bodies. Unknown S3 objects are not made runnable silently.

Supported identity metadata:

- `x-amz-meta-beehive-entity-id`;
- `x-amz-meta-beehive-artifact-id`;
- optional `x-amz-meta-beehive-stage-id`;
- optional `x-amz-meta-beehive-source-artifact-id`.

Manual source artifact registration is available for objects that are known by operator input rather than S3 metadata. The registration validates active S3 stage prefix ownership and creates a pending pointer state without reading the object body.

## Route Safety

S3 `save_path` is a logical route, not an OS path. Accepted forms:

- `main_dir/processed/raw_entities`;
- `/main_dir/processed/raw_entities`;
- `s3://steos-s3-data/main_dir/processed/raw_entities`.

Rejected forms include empty strings, `..`, Windows drive paths, UNC paths, absolute OS paths, unknown buckets, unknown prefixes, and ambiguous aliases. Invalid routes become blocked runtime states.

## B1 Boundaries

B1/B1.1 do not call real S3, list buckets, poll async manifests, manage n8n workflows, or implement a scheduler.

B2 adds real S3 list/head reconciliation and manual source artifact registration. B2 still does not manage n8n workflows through the n8n REST API, build a credential manager UI, read S3 business JSON during Beehive execution, or implement async manifest polling.
