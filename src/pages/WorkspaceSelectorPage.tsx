import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { isHttpApiMode } from "../lib/apiClient";
import {
  createRegisteredWorkspace,
  deleteRegisteredWorkspace,
  listRegisteredWorkspaces,
  restoreRegisteredWorkspace,
  updateRegisteredWorkspace,
} from "../lib/bootstrapApi";
import type {
  CommandErrorInfo,
  CreateWorkspaceRequest,
  UpdateWorkspaceRequest,
  WorkspaceDescriptor,
} from "../types/domain";

interface WorkspaceFormState {
  id: string;
  name: string;
  bucket: string;
  workspace_prefix: string;
  region: string;
  endpoint: string;
}

const emptyWorkspaceForm: WorkspaceFormState = {
  id: "",
  name: "",
  bucket: "",
  workspace_prefix: "",
  region: "",
  endpoint: "",
};

export function WorkspaceSelectorPage() {
  const { state, isBusy, openRegisteredWorkspace, selectRegisteredWorkspace } = useBootstrap();
  const navigate = useNavigate();
  const [workspaces, setWorkspaces] = useState<WorkspaceDescriptor[]>([]);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [openingWorkspaceId, setOpeningWorkspaceId] = useState<string | null>(null);
  const [showArchived, setShowArchived] = useState(false);
  const [createForm, setCreateForm] = useState<WorkspaceFormState>(emptyWorkspaceForm);
  const [editingWorkspaceId, setEditingWorkspaceId] = useState<string | null>(null);
  const [editForm, setEditForm] = useState<WorkspaceFormState>(emptyWorkspaceForm);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const selectedWorkspaceId = state.selected_workspace_id;

  async function loadWorkspaces(includeArchived = showArchived) {
    setIsLoading(true);
    try {
      const result = await listRegisteredWorkspaces(includeArchived);
      setWorkspaces(result.workspaces);
      setErrors(result.errors);
    } finally {
      setIsLoading(false);
    }
  }

  useEffect(() => {
    void loadWorkspaces(showArchived);
  }, [showArchived]);

  const sortedWorkspaces = useMemo(
    () => [...workspaces].sort((left, right) => left.name.localeCompare(right.name)),
    [workspaces],
  );

  async function selectWorkspace(workspaceId: string) {
    const workspace = workspaces.find((item) => item.id === workspaceId);
    if (!workspace || workspace.is_archived) return;
    setOpeningWorkspaceId(workspaceId);
    try {
      if (isHttpApiMode) {
        selectRegisteredWorkspace(workspace);
        navigate(`/workspaces/${encodeURIComponent(workspaceId)}/workspace`);
      } else {
        await openRegisteredWorkspace(workspaceId);
        navigate("/workspace");
      }
    } finally {
      setOpeningWorkspaceId(null);
    }
  }

  async function handleCreateWorkspace() {
    const input: CreateWorkspaceRequest = {
      id: createForm.id.trim() || null,
      name: createForm.name.trim(),
      bucket: createForm.bucket.trim(),
      workspace_prefix: createForm.workspace_prefix.trim(),
      region: createForm.region.trim() || null,
      endpoint: createForm.endpoint.trim() || null,
    };
    setActionMessage(null);
    const result = await createRegisteredWorkspace(input);
    setErrors(result.errors);
    if (result.payload?.workspace) {
      setCreateForm(emptyWorkspaceForm);
      setActionMessage(`Workspace ${result.payload.workspace.id} created.`);
      await loadWorkspaces(showArchived);
    } else {
      setActionMessage("Workspace creation was rejected.");
    }
  }

  function startEdit(workspace: WorkspaceDescriptor) {
    setEditingWorkspaceId(workspace.id);
    setEditForm({
      id: workspace.id,
      name: workspace.name,
      bucket: workspace.bucket ?? "",
      workspace_prefix: workspace.workspace_prefix ?? "",
      region: workspace.region ?? "",
      endpoint: workspace.endpoint ?? "",
    });
  }

  async function handleUpdateWorkspace() {
    if (!editingWorkspaceId) return;
    const input: UpdateWorkspaceRequest = {
      name: editForm.name.trim(),
      bucket: editForm.bucket.trim(),
      workspace_prefix: editForm.workspace_prefix.trim(),
      region: editForm.region.trim() || null,
      endpoint: editForm.endpoint.trim() || null,
    };
    setActionMessage(null);
    const result = await updateRegisteredWorkspace(editingWorkspaceId, input);
    setErrors(result.errors);
    if (result.payload?.workspace) {
      setActionMessage(`Workspace ${result.payload.workspace.id} updated.`);
      setEditingWorkspaceId(null);
      await loadWorkspaces(showArchived);
    } else {
      setActionMessage("Workspace update was rejected.");
    }
  }

  async function handleDeleteWorkspace(workspace: WorkspaceDescriptor) {
    setActionMessage(null);
    const result = await deleteRegisteredWorkspace(workspace.id);
    setErrors(result.errors);
    if (result.payload) {
      setActionMessage(
        result.payload.hard_deleted
          ? `Workspace ${workspace.id} deleted from registry.`
          : `Workspace ${workspace.id} archived.`,
      );
      await loadWorkspaces(showArchived);
    } else {
      setActionMessage("Workspace archive/delete was rejected.");
    }
  }

  async function handleRestoreWorkspace(workspace: WorkspaceDescriptor) {
    setActionMessage(null);
    const result = await restoreRegisteredWorkspace(workspace.id);
    setErrors(result.errors);
    if (result.payload?.workspace) {
      setActionMessage(`Workspace ${workspace.id} restored.`);
      await loadWorkspaces(true);
    } else {
      setActionMessage("Workspace restore was rejected.");
    }
  }

  const createDisabled =
    isLoading ||
    !createForm.name.trim() ||
    !createForm.bucket.trim() ||
    !createForm.workspace_prefix.trim();
  const editDisabled =
    isLoading ||
    !editingWorkspaceId ||
    !editForm.name.trim() ||
    !editForm.bucket.trim() ||
    !editForm.workspace_prefix.trim();

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Operator Registry</span>
          <h1>Workspaces</h1>
          <span className="muted">Create, select, edit, archive, and restore S3 workspaces.</span>
        </div>
        <div className="button-row">
          <label className="checkbox-row">
            <input
              type="checkbox"
              checked={showArchived}
              onChange={(event) => setShowArchived(event.target.checked)}
            />
            Show archived
          </label>
          <button
            type="button"
            className="button secondary"
            disabled={isLoading}
            onClick={() => void loadWorkspaces()}
          >
            {isLoading ? "Refreshing..." : "Refresh"}
          </button>
        </div>
      </div>

      <CommandErrorsPanel title="Workspace Registry Errors" errors={errors} />
      {actionMessage ? <section className="compact-panel panel">{actionMessage}</section> : null}

      <section className="panel">
        <div className="panel-heading">
          <div>
            <h2>Create workspace</h2>
            <span className="muted">Server paths are generated by Beehive.</span>
          </div>
        </div>
        <WorkspaceForm
          form={createForm}
          disabled={isLoading}
          idReadOnly={false}
          onChange={setCreateForm}
        />
        <div className="button-row">
          <button
            type="button"
            className="button primary"
            disabled={createDisabled}
            onClick={() => void handleCreateWorkspace()}
          >
            Create workspace
          </button>
        </div>
      </section>

      {editingWorkspaceId ? (
        <section className="panel">
          <div className="panel-heading">
            <div>
              <h2>Edit workspace</h2>
              <span className="muted">{editingWorkspaceId}</span>
            </div>
            <button
              type="button"
              className="button secondary"
              onClick={() => setEditingWorkspaceId(null)}
            >
              Cancel
            </button>
          </div>
          <WorkspaceForm
            form={editForm}
            disabled={isLoading}
            idReadOnly
            onChange={setEditForm}
          />
          <p className="field-hint">
            Bucket and prefix changes are accepted only while the workspace has no stages, artifacts, or run history.
          </p>
          <div className="button-row">
            <button
              type="button"
              className="button primary"
              disabled={editDisabled}
              onClick={() => void handleUpdateWorkspace()}
            >
              Save workspace
            </button>
          </div>
        </section>
      ) : null}

      {sortedWorkspaces.length === 0 ? (
        <section className="panel">
          <p className="empty-text">
            {isLoading ? "Loading workspaces..." : "No registered workspaces were found."}
          </p>
        </section>
      ) : (
        <div className="workspace-card-grid">
          {sortedWorkspaces.map((workspace) => {
            const isSelected = selectedWorkspaceId === workspace.id;
            const isOpening = openingWorkspaceId === workspace.id;
            return (
              <section key={workspace.id} className="panel workspace-card">
                <div className="panel-heading">
                  <div>
                    <h2>{workspace.name}</h2>
                    <span className="muted">{workspace.id}</span>
                  </div>
                  <span
                    className={
                      workspace.is_archived
                        ? "status-pill status-blocked"
                        : isSelected
                          ? "status-pill status-done"
                          : "status-pill"
                    }
                  >
                    {workspace.is_archived ? "archived" : isSelected ? "selected" : "active"}
                  </span>
                </div>
                <div className="inline-meta">
                  <span>{workspace.bucket ?? "no bucket"}</span>
                  <span>{workspace.workspace_prefix ?? "no prefix"}</span>
                  <span>{workspace.stage_count} stages</span>
                </div>
                <div className="button-row">
                  <button
                    type="button"
                    className="button primary"
                    disabled={isBusy || isOpening || workspace.is_archived}
                    onClick={() => void selectWorkspace(workspace.id)}
                  >
                    {isOpening ? "Opening..." : isSelected ? "Open workspace" : "Select workspace"}
                  </button>
                  <button
                    type="button"
                    className="button secondary"
                    disabled={isLoading}
                    onClick={() => startEdit(workspace)}
                  >
                    Edit
                  </button>
                  {workspace.is_archived ? (
                    <button
                      type="button"
                      className="button secondary"
                      disabled={isLoading}
                      onClick={() => void handleRestoreWorkspace(workspace)}
                    >
                      Restore
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="button secondary"
                      disabled={isLoading}
                      onClick={() => void handleDeleteWorkspace(workspace)}
                    >
                      Archive/Delete
                    </button>
                  )}
                </div>
              </section>
            );
          })}
        </div>
      )}
    </div>
  );
}

function WorkspaceForm({
  disabled,
  form,
  idReadOnly,
  onChange,
}: {
  disabled: boolean;
  form: WorkspaceFormState;
  idReadOnly: boolean;
  onChange: (form: WorkspaceFormState) => void;
}) {
  function update(key: keyof WorkspaceFormState, value: string) {
    onChange({ ...form, [key]: value });
  }

  return (
    <div className="stage-editor-form-grid">
      <div className="form-row">
        <label htmlFor={`${idReadOnly ? "edit" : "create"}-workspace-id`}>ID</label>
        <input
          id={`${idReadOnly ? "edit" : "create"}-workspace-id`}
          value={form.id}
          disabled={disabled || idReadOnly}
          onChange={(event) => update("id", event.target.value)}
          placeholder="optional, e.g. pilot"
        />
      </div>
      <div className="form-row">
        <label htmlFor={`${idReadOnly ? "edit" : "create"}-workspace-name`}>Name</label>
        <input
          id={`${idReadOnly ? "edit" : "create"}-workspace-name`}
          value={form.name}
          disabled={disabled}
          onChange={(event) => update("name", event.target.value)}
          placeholder="Pilot workspace"
        />
      </div>
      <div className="form-row">
        <label htmlFor={`${idReadOnly ? "edit" : "create"}-workspace-bucket`}>Bucket</label>
        <input
          id={`${idReadOnly ? "edit" : "create"}-workspace-bucket`}
          value={form.bucket}
          disabled={disabled}
          onChange={(event) => update("bucket", event.target.value)}
          placeholder="steos-s3-data"
        />
      </div>
      <div className="form-row">
        <label htmlFor={`${idReadOnly ? "edit" : "create"}-workspace-prefix`}>Workspace prefix</label>
        <input
          id={`${idReadOnly ? "edit" : "create"}-workspace-prefix`}
          value={form.workspace_prefix}
          disabled={disabled}
          onChange={(event) => update("workspace_prefix", event.target.value)}
          placeholder="beehive-smoke/test_workflow"
        />
      </div>
      <div className="form-row">
        <label htmlFor={`${idReadOnly ? "edit" : "create"}-workspace-region`}>Region</label>
        <input
          id={`${idReadOnly ? "edit" : "create"}-workspace-region`}
          value={form.region}
          disabled={disabled}
          onChange={(event) => update("region", event.target.value)}
          placeholder="ru-1"
        />
      </div>
      <div className="form-row">
        <label htmlFor={`${idReadOnly ? "edit" : "create"}-workspace-endpoint`}>Endpoint</label>
        <input
          id={`${idReadOnly ? "edit" : "create"}-workspace-endpoint`}
          value={form.endpoint}
          disabled={disabled}
          onChange={(event) => update("endpoint", event.target.value)}
          placeholder="https://s3.example"
        />
      </div>
    </div>
  );
}
