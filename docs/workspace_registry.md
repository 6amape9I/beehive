# Workspace Registry

## File

Default registry:

```text
config/workspaces.yaml
```

Override:

```text
BEEHIVE_WORKSPACES_CONFIG=/absolute/path/workspaces.yaml
```

## Schema

```yaml
workspaces:
  - id: smoke
    name: Smoke Test Workspace
    provider: s3
    bucket: steos-s3-data
    workspace_prefix: beehive-smoke/test_workflow
    region: ru-1
    endpoint: https://s3.ru-1.storage.selcloud.ru
    workdir_path: /tmp/beehive-web-workspaces/smoke
    pipeline_path: /tmp/beehive-web-workspaces/smoke/pipeline.yaml
    database_path: /tmp/beehive-web-workspaces/smoke/app.db
```

The registry must not contain S3 access keys, secret keys, n8n credentials, or tokens.

## Browser Contract

Browser requests send only `workspace_id`. The backend resolves:

- workdir path;
- pipeline path;
- database path;
- bucket/prefix;
- endpoint/region.

Public workspace descriptors expose only:

- id;
- name;
- provider;
- bucket;
- workspace prefix;
- region;
- endpoint.

Server filesystem paths and credentials are not returned to the browser.

## Backend Behavior

The registry loader validates unique safe workspace IDs, absolute server-side paths, S3 bucket/prefix presence, and that pipeline/database paths are inside the registered workspace workdir.

Unknown workspace IDs are rejected. Browser-facing workspace-ID commands do not accept arbitrary `workdir_path` or `database_path`.
