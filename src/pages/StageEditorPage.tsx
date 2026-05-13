import { useCallback, useEffect, useMemo, useState } from "react";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { PipelineYamlPreview } from "../components/stage-editor/PipelineYamlPreview";
import { ProjectRuntimeForm } from "../components/stage-editor/ProjectRuntimeForm";
import { StageDraftForm } from "../components/stage-editor/StageDraftForm";
import { StageDraftList } from "../components/stage-editor/StageDraftList";
import { StageValidationPanel } from "../components/stage-editor/StageValidationPanel";
import {
  createS3Stage,
  getPipelineEditorState,
  savePipelineConfig,
  validatePipelineConfigDraft,
} from "../lib/runtimeApi";
import type {
  CommandErrorInfo,
  ConfigValidationResult,
  CreateS3StagePayload,
  CreateS3StageRequest,
  PipelineConfigDraft,
  PipelineEditorState,
  StageDefinitionDraft,
  StageUsageSummary,
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

const defaultS3StageCreationForm: S3StageCreationForm = {
  stage_id: "",
  workflow_url: "",
  next_stage: "",
  max_attempts: 3,
  retry_delay_sec: 30,
  allow_empty_outputs: false,
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
  const [editorState, setEditorState] = useState<PipelineEditorState | null>(null);
  const [draft, setDraft] = useState<PipelineConfigDraft | null>(null);
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

  const workdirPath = state.selected_workdir_path;
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

  const loadEditor = useCallback(async () => {
    if (!canEdit || !workdirPath) {
      setEditorState(null);
      setDraft(null);
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
  }, [canEdit, workdirPath]);

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
    if (!state.selected_workspace_id) {
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
      const result = await createS3Stage(state.selected_workspace_id, input);
      setErrors(result.errors);
      if (result.payload) {
        setCreatedS3Stage(result.payload);
        setS3StageForm(defaultS3StageCreationForm);
        await reloadCurrentWorkdir();
        await loadEditor();
        setActionMessage(`S3 stage ${result.payload.stage.id} created.`);
      } else {
        setActionMessage("S3 stage creation was rejected.");
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

      {!canEdit ? (
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
            selectedWorkspaceId={state.selected_workspace_id}
            stages={draft.stages}
            onChange={setS3StageForm}
            onCreate={() => void handleCreateS3Stage()}
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
          <StageValidationPanel issues={validation.issues} />
          <PipelineYamlPreview yaml={yamlPreview} />
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
  stages: StageDefinitionDraft[];
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

function boundedNumber(value: string, min: number, max: number, fallback: number) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.min(max, Math.max(min, Math.trunc(parsed)));
}
