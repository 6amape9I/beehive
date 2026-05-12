# Beehive S3 Control Plane Architecture

## Purpose

B1 moves Beehive toward a storage-aware control plane for S3+n8n pipelines. n8n remains the data-plane transformer. S3 stores business artifacts. Beehive owns runtime state, routing validation, attempts, lineage, and operator visibility.

## Control/Data Split

Beehive does:

- stores pipeline and stage config;
- stores artifact pointers;
- claims one eligible artifact for a stage run;
- creates `run_id`;
- triggers n8n with technical S3 pointer headers;
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

SQLite remains the MVP control-plane database. Schema v5 extends existing tables:

- `stages.input_uri`;
- `stages.save_path_aliases_json`;
- `entity_files.storage_provider`;
- `entity_files.bucket`;
- `entity_files.object_key`;
- `entity_files.version_id`;
- `entity_files.etag`;
- `entity_files.checksum_sha256`;
- `entity_files.artifact_size`;
- `entity_files.producer_run_id`.

Local fields remain present for backward compatibility.

## Runtime Flow

S3 execution path:

1. `entity_stage_states` claim moves an S3 artifact to `queued`.
2. Beehive creates a `stage_runs` row with a technical audit envelope.
3. Beehive sends an empty-body webhook request with S3 pointer headers.
4. n8n returns `beehive.s3_artifact_manifest.v1`.
5. Beehive validates schema, run id, source, output bucket, output route, and output key prefix.
6. Valid outputs become S3 artifact pointer rows.
7. Source state becomes `done`; child states become `pending`.

Local execution path remains the B0 payload-only behavior.

## Route Safety

S3 `save_path` is a logical route, not an OS path. Accepted forms:

- `main_dir/processed/raw_entities`;
- `/main_dir/processed/raw_entities`;
- `s3://steos-s3-data/main_dir/processed/raw_entities`.

Rejected forms include empty strings, `..`, Windows drive paths, UNC paths, absolute OS paths, unknown buckets, unknown prefixes, and ambiguous aliases. Invalid routes become blocked runtime states.

## B1 Boundaries

B1 does not call real S3, list buckets, manage credentials, poll async manifests, manage n8n workflows, or implement a scheduler. B2 should add real S3 reconciliation and a one-artifact n8n smoke pipeline.
