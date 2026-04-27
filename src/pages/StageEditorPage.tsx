import { useCallback, useEffect, useMemo, useState } from "react";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { PipelineYamlPreview } from "../components/stage-editor/PipelineYamlPreview";
import { ProjectRuntimeForm } from "../components/stage-editor/ProjectRuntimeForm";
import { StageDraftForm } from "../components/stage-editor/StageDraftForm";
import { StageDraftList } from "../components/stage-editor/StageDraftList";
import { StageValidationPanel } from "../components/stage-editor/StageValidationPanel";
import {
  getPipelineEditorState,
  savePipelineConfig,
  validatePipelineConfigDraft,
} from "../lib/runtimeApi";
import type {
  CommandErrorInfo,
  ConfigValidationResult,
  PipelineConfigDraft,
  PipelineEditorState,
  StageDefinitionDraft,
  StageUsageSummary,
} from "../types/domain";

const emptyValidation: ConfigValidationResult = { is_valid: true, issues: [] };

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
    output_folder: `stages/${id}_out`,
    workflow_url: `http://localhost:5678/webhook/${id}`,
    max_attempts: 3,
    retry_delay_sec: 10,
    next_stage: null,
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
