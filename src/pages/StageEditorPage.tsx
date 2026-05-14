import { useCallback, useEffect, useMemo, useState } from "react";
import { useParams } from "react-router-dom";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { PipelineYamlPreview } from "../components/stage-editor/PipelineYamlPreview";
import { ProjectRuntimeForm } from "../components/stage-editor/ProjectRuntimeForm";
import { StageDraftForm } from "../components/stage-editor/StageDraftForm";
import { StageDraftList } from "../components/stage-editor/StageDraftList";
import { StageValidationPanel } from "../components/stage-editor/StageValidationPanel";
import {
  createS3Stage,
  deleteS3Stage,
  getWorkspaceExplorerById,
  getPipelineEditorState,
  restoreS3Stage,
  savePipelineConfig,
  updateS3Stage,
  updateStageNextStage,
  validatePipelineConfigDraft,
} from "../lib/runtimeApi";
import type {
  CommandErrorInfo,
  ConfigValidationResult,
  CreateS3StagePayload,
  CreateS3StageRequest,
  PipelineConfigDraft,
  PipelineEditorState,
  S3StageMutationPayload,
  StageDefinitionDraft,
  StageUsageSummary,
  UpdateS3StageRequest,
  UpdateStageNextStagePayload,
  WorkspaceStageTree,
} from "../types/domain";

const emptyValidation: ConfigValidationResult = { is_valid: true, issues: [] };

interface S3StageCreationForm {
  stage_id: string;
  workflow_url: string;
  next_stage: string;
  max_attempts: number;
  retry_delay_sec: number;
  allow_empty_outputs: boolean;
}

interface StageLinkForm {
  source_stage_id: string;
  next_stage: string;
}

interface StageOption {
  id: string;
  next_stage: string | null;
}

const defaultS3StageCreationForm: S3StageCreationForm = {
  stage_id: "",
  workflow_url: "",
  next_stage: "",
  max_attempts: 3,
  retry_delay_sec: 30,
  allow_empty_outputs: false,
};

const defaultStageLinkForm: StageLinkForm = {
  source_stage_id: "",
  next_stage: "",
};

function makeNewStage(existingIds: string[]): StageDefinitionDraft {
  let suffix = 1;
  let id = "new_stage";
  while (existingIds.includes(id)) {
    suffix += 1;
    id = `new_stage_${suffix}`;
  }
  return {
    id,
    input_folder: `stages/${id}`,
    input_uri: null,
    output_folder: `stages/${id}_out`,
    workflow_url: `http://localhost:5678/webhook/${id}`,
    max_attempts: 3,
    retry_delay_sec: 10,
    next_stage: null,
    save_path_aliases: [],
    allow_empty_outputs: false,
    original_stage_id: null,
    is_new: true,
  };
}

function findUsage(usages: StageUsageSummary[], stage: StageDefinitionDraft | null) {
  if (!stage) return null;
  return usages.find((usage) => usage.stage_id === (stage.original_stage_id ?? stage.id)) ?? null;
}

function removeBlockedReason(draft: PipelineConfigDraft | null, stage: StageDefinitionDraft | null) {
  if (!draft || !stage) return null;
  const refs = draft.stages
    .filter((candidate) => candidate.id !== stage.id && candidate.next_stage === stage.id)
    .map((candidate) => candidate.id);
  return refs.length > 0
    ? `Clear next_stage references from ${refs.join(", ")} before removing this stage.`
    : null;
}

export function StageEditorPage() {
  const { state, reloadCurrentWorkdir } = useBootstrap();
  const { workspaceId: routeWorkspaceId } = useParams();
  const [editorState, setEditorState] = useState<PipelineEditorState | null>(null);
  const [draft, setDraft] = useState<PipelineConfigDraft | null>(null);
  const [workspaceStages, setWorkspaceStages] = useState<WorkspaceStageTree[]>([]);
  const [validation, setValidation] = useState<ConfigValidationResult>(emptyValidation);
  const [yamlPreview, setYamlPreview] = useState("");
  const [stageUsages, setStageUsages] = useState<StageUsageSummary[]>([]);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [isValidating, setIsValidating] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [operatorComment, setOperatorComment] = useState("");
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [confirmRemoveStageId, setConfirmRemoveStageId] = useState<string | null>(null);
  const [s3StageForm, setS3StageForm] = useState<S3StageCreationForm>(
    defaultS3StageCreationForm,
  );
  const [createdS3Stage, setCreatedS3Stage] = useState<CreateS3StagePayload | null>(null);
  const [stageLinkForm, setStageLinkForm] = useState<StageLinkForm>(defaultStageLinkForm);
  const [updatedStageLink, setUpdatedStageLink] = useState<UpdateStageNextStagePayload | null>(
    null,
  );
  const [stageMutation, setStageMutation] = useState<S3StageMutationPayload | null>(null);

  const workdirPath = state.selected_workdir_path;
  const workspaceId = routeWorkspaceId ?? state.selected_workspace_id;
  const isWorkspaceHttpFlow = !!workspaceId && !workdirPath;
  const canEdit = state.phase === "fully_initialized" && !!workdirPath;
  const selectedStage =
    draft && selectedIndex !== null ? draft.stages[selectedIndex] ?? null : null;
  const selectedUsage = useMemo(
    () => findUsage(stageUsages, selectedStage),
    [selectedStage, stageUsages],
  );
  const blockedRemoveReason = removeBlockedReason(draft, selectedStage);
  const isDirty = useMemo(
    () => JSON.stringify(draft) !== JSON.stringify(editorState?.draft ?? null),
    [draft, editorState],
  );
  const stageOptions = useMemo<StageOption[]>(
    () =>
      draft
        ? draft.stages.map((stage) => ({ id: stage.id, next_stage: stage.next_stage }))
        : workspaceStages.map((stage) => ({ id: stage.stage_id, next_stage: stage.next_stage })),
    [draft, workspaceStages],
  );

  const loadEditor = useCallback(async () => {
    if (workspaceId && !workdirPath) {
      setIsLoading(true);
      setActionMessage(null);
      try {
        const result = await getWorkspaceExplorerById(workspaceId);
        setWorkspaceStages(result.stages);
        setErrors(result.errors);
        setEditorState(null);
        setDraft(null);
        setValidation(emptyValidation);
        setYamlPreview("");
        setStageUsages([]);
        setSelectedIndex(null);
      } finally {
        setIsLoading(false);
      }
      return;
    }
    if (!canEdit || !workdirPath) {
      setEditorState(null);
      setDraft(null);
      setWorkspaceStages([]);
      setValidation(emptyValidation);
      setYamlPreview("");
      setStageUsages([]);
      setErrors([]);
      return;
    }
    setIsLoading(true);
    setActionMessage(null);
    try {
      const result = await getPipelineEditorState(workdirPath);
      setErrors(result.errors);
      setEditorState(result.state);
      setDraft(result.state?.draft ?? null);
      setValidation(result.state?.validation ?? emptyValidation);
      setYamlPreview(result.state?.yaml_preview ?? "");
      setStageUsages(result.state?.stage_usages ?? []);
      setSelectedIndex(result.state?.draft?.stages[0] ? 0 : null);
    } finally {
      setIsLoading(false);
    }
  }, [canEdit, workdirPath, workspaceId]);

  useEffect(() => {
    void loadEditor();
  }, [loadEditor]);

  function updateDraft(nextDraft: PipelineConfigDraft) {
    setDraft(nextDraft);
    setConfirmRemoveStageId(null);
  }

  function updateStage(index: number, stage: StageDefinitionDraft) {
    if (!draft) return;
    updateDraft({
      ...draft,
      stages: draft.stages.map((item, itemIndex) => (itemIndex === index ? stage : item)),
    });
  }

  function handleAddStage() {
    if (!draft) return;
    const stage = makeNewStage(draft.stages.map((item) => item.id));
    updateDraft({ ...draft, stages: [...draft.stages, stage] });
    setSelectedIndex(draft.stages.length);
  }

  function handleRemoveStage() {
    if (!draft || selectedIndex === null || !selectedStage || blockedRemoveReason) return;
    const hasHistory =
      selectedUsage &&
      (selectedUsage.entity_count > 0 ||
        selectedUsage.entity_file_count > 0 ||
        selectedUsage.stage_state_count > 0 ||
        selectedUsage.run_count > 0);
    if (hasHistory && confirmRemoveStageId !== selectedStage.id) {
      setConfirmRemoveStageId(selectedStage.id);
      setActionMessage(
        `Stage ${selectedStage.id} has history. Click Remove from config again to archive it from active YAML.`,
      );
      return;
    }
    const stages = draft.stages.filter((_, index) => index !== selectedIndex);
    updateDraft({ ...draft, stages });
    setSelectedIndex(stages.length === 0 ? null : Math.min(selectedIndex, stages.length - 1));
    setActionMessage(`Removed ${selectedStage.id} from draft config.`);
  }

  async function handleValidate() {
    if (!workdirPath || !draft) return;
    setIsValidating(true);
    setActionMessage(null);
    try {
      const result = await validatePipelineConfigDraft(workdirPath, draft);
      setValidation(result.validation);
      setYamlPreview(result.yaml_preview ?? yamlPreview);
      setStageUsages(result.stage_usages);
      setErrors(result.errors);
      setActionMessage(result.validation.is_valid ? "Draft is valid." : "Draft has validation errors.");
    } finally {
      setIsValidating(false);
    }
  }

  async function handleSave() {
    if (!workdirPath || !draft) return;
    setIsSaving(true);
    setActionMessage(null);
    try {
      const result = await savePipelineConfig(
        workdirPath,
        draft,
        operatorComment.trim() || null,
      );
      setErrors(result.errors);
      if (result.state) {
        setEditorState(result.state);
        setDraft(result.state.draft);
        setValidation(result.state.validation);
        setYamlPreview(result.state.yaml_preview);
        setStageUsages(result.state.stage_usages);
        setSelectedIndex(result.state.draft?.stages[0] ? 0 : null);
        setOperatorComment("");
        await reloadCurrentWorkdir();
        setActionMessage(
          result.backup_path
            ? `Pipeline config saved. Backup: ${result.backup_path}`
            : "Pipeline config saved.",
        );
      } else if (result.errors.length > 0) {
        setActionMessage("Save rejected. Fix validation errors before saving.");
      }
    } finally {
      setIsSaving(false);
    }
  }

  async function handleCreateS3Stage() {
    if (!workspaceId) {
      setActionMessage("Select a registered workspace before creating an S3 stage.");
      return;
    }
    const input: CreateS3StageRequest = {
      stage_id: s3StageForm.stage_id.trim(),
      workflow_url: s3StageForm.workflow_url.trim(),
      next_stage: s3StageForm.next_stage.trim() || null,
      max_attempts: s3StageForm.max_attempts,
      retry_delay_sec: s3StageForm.retry_delay_sec,
      allow_empty_outputs: s3StageForm.allow_empty_outputs,
    };
    setIsSaving(true);
    setActionMessage(null);
    setCreatedS3Stage(null);
    try {
      const result = await createS3Stage(workspaceId, input);
      setErrors(result.errors);
      if (result.payload) {
        setCreatedS3Stage(result.payload);
        setS3StageForm(defaultS3StageCreationForm);
        if (workdirPath) {
          await reloadCurrentWorkdir();
        }
        await loadEditor();
        setActionMessage(`S3 stage ${result.payload.stage.id} created.`);
      } else {
        setActionMessage("S3 stage creation was rejected.");
      }
    } finally {
      setIsSaving(false);
    }
  }

  async function handleUpdateStageLink() {
    if (!workspaceId || !stageLinkForm.source_stage_id) {
      setActionMessage("Select a workspace and source stage before connecting stages.");
      return;
    }
    setIsSaving(true);
    setActionMessage(null);
    setUpdatedStageLink(null);
    try {
      const result = await updateStageNextStage(workspaceId, stageLinkForm.source_stage_id, {
        next_stage: stageLinkForm.next_stage || null,
      });
      setErrors(result.errors);
      if (result.payload) {
        setUpdatedStageLink(result.payload);
        setActionMessage(
          result.payload.stage.next_stage
            ? `${result.payload.stage.id} now points to ${result.payload.stage.next_stage}.`
            : `${result.payload.stage.id} is now terminal.`,
        );
        await loadEditor();
      } else {
        setActionMessage("Stage link update was rejected.");
      }
    } finally {
      setIsSaving(false);
    }
  }

  async function handleUpdateS3Stage(stageId: string, input: UpdateS3StageRequest) {
    if (!workspaceId) return;
    setIsSaving(true);
    setActionMessage(null);
    setStageMutation(null);
    try {
      const result = await updateS3Stage(workspaceId, stageId, input);
      setErrors(result.errors);
      if (result.payload?.stage) {
        setStageMutation(result.payload);
        setActionMessage(`Stage ${result.payload.stage.id} updated.`);
        await loadEditor();
      } else {
        setActionMessage("Stage update was rejected.");
      }
    } finally {
      setIsSaving(false);
    }
  }

  async function handleDeleteS3Stage(stageId: string) {
    if (!workspaceId) return;
    setIsSaving(true);
    setActionMessage(null);
    setStageMutation(null);
    try {
      const result = await deleteS3Stage(workspaceId, stageId);
      setErrors(result.errors);
      if (result.payload) {
        setStageMutation(result.payload);
        setActionMessage(
          result.payload.hard_deleted
            ? `Stage ${stageId} removed from active pipeline.`
            : `Stage ${stageId} archived from active pipeline.`,
        );
        await loadEditor();
      } else {
        setActionMessage("Stage archive/delete was rejected.");
      }
    } finally {
      setIsSaving(false);
    }
  }

  async function handleRestoreS3Stage(stageId: string) {
    if (!workspaceId) return;
    setIsSaving(true);
    setActionMessage(null);
    setStageMutation(null);
    try {
      const result = await restoreS3Stage(workspaceId, stageId);
      setErrors(result.errors);
      if (result.payload?.stage) {
        setStageMutation(result.payload);
        setActionMessage(`Stage ${result.payload.stage.id} restored.`);
        await loadEditor();
      } else {
        setActionMessage("Stage restore was rejected.");
      }
    } finally {
      setIsSaving(false);
    }
  }

  const disabled = isLoading || isSaving || isValidating;

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Configuration</span>
          <h1>Stage Editor</h1>
          {workspaceId ? <span className="muted">Workspace {workspaceId}</span> : null}
        </div>
        <div className="button-row">
          <button type="button" className="button secondary" disabled={disabled} onClick={() => void loadEditor()}>
            Reload
          </button>
          <button type="button" className="button secondary" disabled={!isDirty || disabled} onClick={() => {
            setDraft(editorState?.draft ?? null);
            setValidation(editorState?.validation ?? emptyValidation);
            setYamlPreview(editorState?.yaml_preview ?? "");
            setActionMessage("Discarded unsaved changes.");
          }}>
            Discard
          </button>
          <button type="button" className="button secondary" disabled={!draft || disabled} onClick={() => void handleValidate()}>
            {isValidating ? "Validating..." : "Validate"}
          </button>
          <button type="button" className="button primary" disabled={!draft || !isDirty || disabled} onClick={() => void handleSave()}>
            {isSaving ? "Saving..." : "Save pipeline config"}
          </button>
        </div>
      </div>

      <CommandErrorsPanel title="Stage Editor Errors" errors={errors} />
      {actionMessage ? <section className="compact-panel panel">{actionMessage}</section> : null}

      {isWorkspaceHttpFlow ? (
        <>
          <S3StageCreationPanel
            disabled={disabled}
            form={s3StageForm}
            payload={createdS3Stage}
            selectedWorkspaceId={workspaceId}
            stages={stageOptions}
            onChange={setS3StageForm}
            onCreate={() => void handleCreateS3Stage()}
          />
          <StageLinkPanel
            disabled={disabled}
            form={stageLinkForm}
            payload={updatedStageLink}
            stages={stageOptions}
            onChange={setStageLinkForm}
            onSave={() => void handleUpdateStageLink()}
          />
          <StageCrudPanel
            disabled={disabled}
            mutation={stageMutation}
            stages={workspaceStages}
            onDelete={(stageId) => void handleDeleteS3Stage(stageId)}
            onRestore={(stageId) => void handleRestoreS3Stage(stageId)}
            onUpdate={(stageId, input) => void handleUpdateS3Stage(stageId, input)}
          />
        </>
      ) : !canEdit ? (
        <section className="panel">
          <h2>Open a workdir</h2>
          <p className="empty-text">Open a fully initialized workdir to edit pipeline.yaml.</p>
        </section>
      ) : !draft ? (
        <section className="panel">
          <h2>{isLoading ? "Loading..." : "Pipeline draft unavailable"}</h2>
          <p className="empty-text">The current pipeline.yaml could not be converted into an editable draft.</p>
        </section>
      ) : (
        <>
          <section className="compact-panel panel">
            <div className="inline-meta">
              <span>workdir {workdirPath}</span>
              <span>loaded {editorState?.loaded_at ?? "not loaded"}</span>
              <span>{isDirty ? "unsaved changes" : "clean"}</span>
              <span>{validation.is_valid ? "valid" : "invalid"}</span>
            </div>
          </section>

          <ProjectRuntimeForm draft={draft} disabled={disabled} onChange={updateDraft} />

          <S3StageCreationPanel
            disabled={disabled}
            form={s3StageForm}
            payload={createdS3Stage}
            selectedWorkspaceId={workspaceId}
            stages={stageOptions}
            onChange={setS3StageForm}
            onCreate={() => void handleCreateS3Stage()}
          />

          <StageLinkPanel
            disabled={disabled}
            form={stageLinkForm}
            payload={updatedStageLink}
            stages={stageOptions}
            onChange={setStageLinkForm}
            onSave={() => void handleUpdateStageLink()}
          />

          <section className="panel">
            <div className="panel-heading">
              <h2>Stage Actions</h2>
              <div className="button-row">
                <button type="button" className="button secondary" disabled={disabled} onClick={handleAddStage}>
                  Add stage
                </button>
              </div>
            </div>
            <div className="form-row">
              <label htmlFor="operator-comment">Operator comment</label>
              <input
                id="operator-comment"
                value={operatorComment}
                disabled={disabled}
                onChange={(event) => setOperatorComment(event.target.value)}
                placeholder="Optional save note"
              />
            </div>
          </section>

          <StageDraftList
            stages={draft.stages}
            usages={stageUsages}
            issues={validation.issues}
            selectedIndex={selectedIndex}
            onSelect={setSelectedIndex}
          />
          <StageDraftForm
            stage={selectedStage}
            stages={draft.stages}
            usage={selectedUsage}
            disabled={disabled}
            removeBlockedReason={blockedRemoveReason}
            onChange={(stage) => selectedIndex !== null && updateStage(selectedIndex, stage)}
            onRemove={handleRemoveStage}
          />
          <details className="panel">
            <summary>
              <strong>Advanced</strong>
              <span className="muted">Validation and YAML preview</span>
            </summary>
            <StageValidationPanel issues={validation.issues} />
            <PipelineYamlPreview yaml={yamlPreview} />
          </details>
        </>
      )}
    </div>
  );
}

interface S3StageCreationPanelProps {
  disabled: boolean;
  form: S3StageCreationForm;
  payload: CreateS3StagePayload | null;
  selectedWorkspaceId: string | null;
  stages: StageOption[];
  onChange: (form: S3StageCreationForm) => void;
  onCreate: () => void;
}

function S3StageCreationPanel({
  disabled,
  form,
  payload,
  selectedWorkspaceId,
  stages,
  onChange,
  onCreate,
}: S3StageCreationPanelProps) {
  const canCreate =
    !!selectedWorkspaceId && !!form.stage_id.trim() && !!form.workflow_url.trim() && !disabled;

  function update(key: keyof S3StageCreationForm, value: string | number | boolean) {
    onChange({ ...form, [key]: value });
  }

  async function copyAlias(alias: string) {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(alias);
    }
  }

  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h2>Create S3 Stage</h2>
          <span className="muted">
            {selectedWorkspaceId
              ? `Workspace ${selectedWorkspaceId}`
              : "Select a registered workspace first"}
          </span>
        </div>
      </div>
      <div className="stage-editor-form-grid">
        <div className="form-row">
          <label htmlFor="s3-stage-id">Stage ID</label>
          <input
            id="s3-stage-id"
            value={form.stage_id}
            disabled={disabled}
            onChange={(event) => update("stage_id", event.target.value)}
            placeholder="semantic_rich"
          />
        </div>
        <div className="form-row">
          <label htmlFor="s3-stage-webhook">n8n webhook URL</label>
          <input
            id="s3-stage-webhook"
            value={form.workflow_url}
            disabled={disabled}
            onChange={(event) => update("workflow_url", event.target.value)}
            placeholder="https://n8n.example/webhook/semantic_rich"
          />
        </div>
        <div className="form-row">
          <label htmlFor="s3-stage-next">Next stage</label>
          <select
            id="s3-stage-next"
            value={form.next_stage}
            disabled={disabled}
            onChange={(event) => update("next_stage", event.target.value)}
          >
            <option value="">Terminal or routed by save_path</option>
            {stages.map((stage) => (
              <option key={stage.id} value={stage.id}>
                {stage.id}
              </option>
            ))}
          </select>
        </div>
        <div className="form-row">
          <label htmlFor="s3-stage-attempts">Max attempts</label>
          <input
            id="s3-stage-attempts"
            type="number"
            min={1}
            value={form.max_attempts}
            disabled={disabled}
            onChange={(event) => update("max_attempts", boundedNumber(event.target.value, 1, 20, 3))}
          />
        </div>
        <div className="form-row">
          <label htmlFor="s3-stage-retry">Retry delay sec</label>
          <input
            id="s3-stage-retry"
            type="number"
            min={0}
            value={form.retry_delay_sec}
            disabled={disabled}
            onChange={(event) =>
              update("retry_delay_sec", boundedNumber(event.target.value, 0, 3600, 30))
            }
          />
        </div>
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={form.allow_empty_outputs}
            disabled={disabled}
            onChange={(event) => update("allow_empty_outputs", event.target.checked)}
          />
          Allow empty outputs
        </label>
      </div>
      <div className="button-row">
        <button type="button" className="button primary" disabled={!canCreate} onClick={onCreate}>
          Create S3 stage
        </button>
      </div>
      {payload ? (
        <div className="route-hints">
          <div className="form-row">
            <label>Input URI</label>
            <code>{payload.route_hints.input_uri}</code>
          </div>
          <div className="route-alias-list">
            {payload.route_hints.save_path_aliases.map((alias) => (
              <button
                key={alias}
                type="button"
                className="button secondary route-alias"
                onClick={() => void copyAlias(alias)}
              >
                {alias}
              </button>
            ))}
          </div>
          <p className="field-hint">
            n8n manifest outputs must use one of these save_path aliases.
          </p>
        </div>
      ) : null}
    </section>
  );
}

interface StageLinkPanelProps {
  disabled: boolean;
  form: StageLinkForm;
  payload: UpdateStageNextStagePayload | null;
  stages: StageOption[];
  onChange: (form: StageLinkForm) => void;
  onSave: () => void;
}

function StageLinkPanel({
  disabled,
  form,
  payload,
  stages,
  onChange,
  onSave,
}: StageLinkPanelProps) {
  const targetOptions = stages.filter((stage) => stage.id !== form.source_stage_id);
  const source = stages.find((stage) => stage.id === form.source_stage_id);

  useEffect(() => {
    if (!source) return;
    onChange({ ...form, next_stage: source.next_stage ?? "" });
  }, [source?.id]);

  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h2>Connect Stages</h2>
          <span className="muted">Set or clear next_stage between existing stages.</span>
        </div>
      </div>
      <div className="stage-editor-form-grid">
        <div className="form-row">
          <label htmlFor="stage-link-source">Source stage</label>
          <select
            id="stage-link-source"
            value={form.source_stage_id}
            disabled={disabled}
            onChange={(event) =>
              onChange({
                ...form,
                source_stage_id: event.target.value,
                next_stage:
                  stages.find((stage) => stage.id === event.target.value)?.next_stage ?? "",
              })
            }
          >
            <option value="">Select source</option>
            {stages.map((stage) => (
              <option key={stage.id} value={stage.id}>
                {stage.id}
              </option>
            ))}
          </select>
        </div>
        <div className="form-row">
          <label htmlFor="stage-link-target">Target stage</label>
          <select
            id="stage-link-target"
            value={form.next_stage}
            disabled={disabled || !form.source_stage_id}
            onChange={(event) => onChange({ ...form, next_stage: event.target.value })}
          >
            <option value="">Terminal</option>
            {targetOptions.map((stage) => (
              <option key={stage.id} value={stage.id}>
                {stage.id}
              </option>
            ))}
          </select>
        </div>
      </div>
      <div className="button-row">
        <button
          type="button"
          className="button primary"
          disabled={disabled || !form.source_stage_id}
          onClick={onSave}
        >
          Save stage link
        </button>
      </div>
      {payload ? (
        <p className="field-hint">
          {payload.stage.id} next_stage is {payload.stage.next_stage ?? "terminal"}.
        </p>
      ) : null}
    </section>
  );
}

interface StageCrudForm {
  stage_id: string;
  workflow_url: string;
  next_stage: string;
  max_attempts: number;
  retry_delay_sec: number;
  allow_empty_outputs: boolean;
}

interface StageCrudPanelProps {
  disabled: boolean;
  mutation: S3StageMutationPayload | null;
  stages: WorkspaceStageTree[];
  onDelete: (stageId: string) => void;
  onRestore: (stageId: string) => void;
  onUpdate: (stageId: string, input: UpdateS3StageRequest) => void;
}

function StageCrudPanel({
  disabled,
  mutation,
  stages,
  onDelete,
  onRestore,
  onUpdate,
}: StageCrudPanelProps) {
  const [selectedStageId, setSelectedStageId] = useState("");
  const selectedStage = stages.find((stage) => stage.stage_id === selectedStageId) ?? null;
  const [form, setForm] = useState<StageCrudForm>({
    stage_id: "",
    workflow_url: "",
    next_stage: "",
    max_attempts: 3,
    retry_delay_sec: 30,
    allow_empty_outputs: false,
  });

  useEffect(() => {
    if (!selectedStage) return;
    setForm({
      stage_id: selectedStage.stage_id,
      workflow_url: selectedStage.workflow_url ?? "",
      next_stage: selectedStage.next_stage ?? "",
      max_attempts: selectedStage.max_attempts,
      retry_delay_sec: selectedStage.retry_delay_sec,
      allow_empty_outputs: selectedStage.allow_empty_outputs,
    });
  }, [selectedStage?.stage_id]);

  function update(key: keyof StageCrudForm, value: string | number | boolean) {
    setForm({ ...form, [key]: value });
  }

  async function copyAlias(alias: string) {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(alias);
    }
  }

  const activeStages = stages.filter((stage) => stage.is_active);
  const canSave = !!selectedStage && selectedStage.is_active && !!form.workflow_url.trim() && !disabled;

  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h2>Manage stages</h2>
          <span className="muted">Edit runtime fields, connect stages, archive/delete, or restore.</span>
        </div>
      </div>
      <div className="stage-editor-form-grid">
        <div className="form-row">
          <label htmlFor="crud-stage-select">Stage</label>
          <select
            id="crud-stage-select"
            value={selectedStageId}
            disabled={disabled}
            onChange={(event) => setSelectedStageId(event.target.value)}
          >
            <option value="">Select stage</option>
            {stages.map((stage) => (
              <option key={stage.stage_id} value={stage.stage_id}>
                {stage.stage_id} {stage.is_active ? "" : "(archived)"}
              </option>
            ))}
          </select>
        </div>
        <div className="form-row">
          <label htmlFor="crud-stage-id">Stage ID</label>
          <input id="crud-stage-id" value={form.stage_id} disabled />
        </div>
        <div className="form-row">
          <label htmlFor="crud-workflow-url">Production n8n webhook URL</label>
          <input
            id="crud-workflow-url"
            value={form.workflow_url}
            disabled={disabled || !selectedStage?.is_active}
            onChange={(event) => update("workflow_url", event.target.value)}
          />
        </div>
        <div className="form-row">
          <label htmlFor="crud-next-stage">Next stage</label>
          <select
            id="crud-next-stage"
            value={form.next_stage}
            disabled={disabled || !selectedStage?.is_active}
            onChange={(event) => update("next_stage", event.target.value)}
          >
            <option value="">Terminal</option>
            {activeStages
              .filter((stage) => stage.stage_id !== selectedStage?.stage_id)
              .map((stage) => (
                <option key={stage.stage_id} value={stage.stage_id}>
                  {stage.stage_id}
                </option>
              ))}
          </select>
        </div>
        <div className="form-row">
          <label htmlFor="crud-max-attempts">Max attempts</label>
          <input
            id="crud-max-attempts"
            type="number"
            min={1}
            value={form.max_attempts}
            disabled={disabled || !selectedStage?.is_active}
            onChange={(event) => update("max_attempts", boundedNumber(event.target.value, 1, 20, 3))}
          />
        </div>
        <div className="form-row">
          <label htmlFor="crud-retry-delay">Retry delay sec</label>
          <input
            id="crud-retry-delay"
            type="number"
            min={0}
            value={form.retry_delay_sec}
            disabled={disabled || !selectedStage?.is_active}
            onChange={(event) =>
              update("retry_delay_sec", boundedNumber(event.target.value, 0, 3600, 30))
            }
          />
        </div>
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={form.allow_empty_outputs}
            disabled={disabled || !selectedStage?.is_active}
            onChange={(event) => update("allow_empty_outputs", event.target.checked)}
          />
          Allow empty outputs
        </label>
      </div>
      <div className="button-row">
        <button
          type="button"
          className="button primary"
          disabled={!canSave}
          onClick={() =>
            onUpdate(form.stage_id, {
              workflow_url: form.workflow_url.trim(),
              next_stage: form.next_stage.trim() || null,
              max_attempts: form.max_attempts,
              retry_delay_sec: form.retry_delay_sec,
              allow_empty_outputs: form.allow_empty_outputs,
            })
          }
        >
          Save stage
        </button>
        <button
          type="button"
          className="button secondary"
          disabled={disabled || !selectedStage?.is_active}
          onClick={() => selectedStage && onDelete(selectedStage.stage_id)}
        >
          Archive/Delete stage
        </button>
        <button
          type="button"
          className="button secondary"
          disabled={disabled || !selectedStage || selectedStage.is_active}
          onClick={() => selectedStage && onRestore(selectedStage.stage_id)}
        >
          Restore stage
        </button>
      </div>
      {selectedStage ? (
        <details className="route-hints">
          <summary>
            <strong>Copy save_path aliases</strong>
          </summary>
          <div className="route-alias-list">
            {selectedStage.save_path_aliases.map((alias) => (
              <button
                key={alias}
                type="button"
                className="button secondary route-alias"
                onClick={() => void copyAlias(alias)}
              >
                {alias}
              </button>
            ))}
          </div>
        </details>
      ) : null}
      {mutation?.stage ? (
        <p className="field-hint">
          Last changed stage: {mutation.stage.id}.{" "}
          {mutation.archived ? "Archived." : mutation.restored ? "Restored." : "Saved."}
        </p>
      ) : null}
    </section>
  );
}

function boundedNumber(value: string, min: number, max: number, fallback: number) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.min(max, Math.max(min, Math.trunc(parsed)));
}
