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
  name: string;
}

const emptyWorkspaceForm: WorkspaceFormState = {
  name: "",
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
      name: createForm.name.trim(),
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
      name: workspace.name,
    });
  }

  async function handleUpdateWorkspace() {
    if (!editingWorkspaceId) return;
    const input: UpdateWorkspaceRequest = {
      name: editForm.name.trim(),
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

  const createDisabled = isLoading || !createForm.name.trim();
  const editDisabled = isLoading || !editingWorkspaceId || !editForm.name.trim();

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
            <span className="muted">
              S3: steos-s3-data / ru-1 / https://s3.ru-1.storage.selcloud.ru
            </span>
          </div>
        </div>
        <WorkspaceForm
          form={createForm}
          disabled={isLoading}
          onChange={setCreateForm}
        />
        <p className="field-hint">Prefix will use the workspace name.</p>
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
            onChange={setEditForm}
          />
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
                  <span>{workspace.stage_count} stages</span>
                  <span>{workspace.is_archived ? "archived" : "active"}</span>
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
  onChange,
}: {
  disabled: boolean;
  form: WorkspaceFormState;
  onChange: (form: WorkspaceFormState) => void;
}) {
  function update(key: keyof WorkspaceFormState, value: string) {
    onChange({ ...form, [key]: value });
  }

  return (
    <div className="stage-editor-form-grid">
      <div className="form-row">
        <label htmlFor="workspace-name">Workspace name</label>
        <input
          id="workspace-name"
          value={form.name}
          disabled={disabled}
          onChange={(event) => update("name", event.target.value)}
          placeholder="Медицинские сущности тест"
        />
      </div>
    </div>
  );
}
