import { useEffect, useMemo, useState } from "react";

import type { EntityFileAllowedActions, EntityFileRecord } from "../../types/domain";

interface EntityJsonPanelProps {
  selectedFile: EntityFileRecord | null;
  selectedJson: string | null;
  selectedFileActions: EntityFileAllowedActions | null;
  isSaving: boolean;
  onSave: (payloadJson: string, metaJson: string, comment: string) => Promise<void>;
}

function pretty(value: unknown) {
  return JSON.stringify(value, null, 2);
}

export function EntityJsonPanel({
  selectedFile,
  selectedJson,
  selectedFileActions,
  isSaving,
  onSave,
}: EntityJsonPanelProps) {
  const parsed = useMemo(() => {
    if (!selectedJson) return null;
    try {
      return JSON.parse(selectedJson) as { payload?: unknown; meta?: unknown };
    } catch {
      return null;
    }
  }, [selectedJson]);
  const [isEditing, setIsEditing] = useState(false);
  const [payloadText, setPayloadText] = useState("{}");
  const [metaText, setMetaText] = useState("{}");
  const [comment, setComment] = useState("");
  const [parseError, setParseError] = useState<string | null>(null);
  const canEditBusinessJson = Boolean(
    selectedFile && selectedFileActions?.can_edit_business_json,
  );
  const policyReason =
    selectedFileActions?.reasons.find((reason) => reason.trim().length > 0) ??
    (!selectedFile ? "Select a file instance to edit JSON." : null);

  useEffect(() => {
    setIsEditing(false);
    setParseError(null);
    setPayloadText(pretty(parsed?.payload ?? {}));
    setMetaText(pretty(parsed?.meta ?? {}));
    setComment("");
  }, [parsed, selectedFile?.id]);

  async function handleSave() {
    setParseError(null);
    if (!canEditBusinessJson) {
      setParseError(policyReason ?? "Business JSON editing is disabled for this file.");
      return;
    }
    try {
      const payload = JSON.parse(payloadText);
      const meta = JSON.parse(metaText);
      if (payload === null) {
        setParseError("Payload must not be null.");
        return;
      }
      if (!meta || typeof meta !== "object" || Array.isArray(meta)) {
        setParseError("Meta must be a JSON object.");
        return;
      }
      await onSave(pretty(payload), pretty(meta), comment);
      setIsEditing(false);
    } catch (error) {
      setParseError(error instanceof Error ? error.message : "JSON parse failed.");
    }
  }

  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h2>JSON Viewer / Editor</h2>
          <span className="muted">
            {selectedFile ? `File id ${selectedFile.id}` : "No file selected"}
          </span>
        </div>
        <button
          type="button"
          className="button secondary"
          disabled={!canEditBusinessJson || isSaving}
          onClick={() => setIsEditing((value) => !value)}
        >
          {isEditing ? "Cancel edit" : "Edit payload/meta"}
        </button>
      </div>
      {selectedFile && !canEditBusinessJson && policyReason ? (
        <p className="muted">{policyReason}</p>
      ) : null}
      {!selectedFile ? (
        <p className="empty-text">Select a file instance to inspect JSON.</p>
      ) : selectedJson ? (
        isEditing ? (
          <div className="json-editor-grid">
            <div className="form-row">
              <label htmlFor="payload-editor">Payload</label>
              <textarea
                id="payload-editor"
                value={payloadText}
                onChange={(event) => setPayloadText(event.target.value)}
              />
            </div>
            <div className="form-row">
              <label htmlFor="meta-editor">Meta</label>
              <textarea
                id="meta-editor"
                value={metaText}
                onChange={(event) => setMetaText(event.target.value)}
              />
            </div>
            <div className="form-row">
              <label htmlFor="save-comment">Operator comment</label>
              <input
                id="save-comment"
                value={comment}
                onChange={(event) => setComment(event.target.value)}
                placeholder="Optional"
              />
            </div>
            {parseError ? <p className="error-text">{parseError}</p> : null}
            <div className="button-row">
              <button
                type="button"
                className="button primary"
                disabled={isSaving || !canEditBusinessJson}
                onClick={() => void handleSave()}
              >
                {isSaving ? "Saving..." : "Save"}
              </button>
              <button type="button" className="button secondary" disabled={isSaving} onClick={() => setIsEditing(false)}>
                Cancel
              </button>
            </div>
          </div>
        ) : (
          <pre className="json-preview">{selectedJson}</pre>
        )
      ) : (
        <p className="empty-text">Selected JSON is not available. The file may be missing or invalid.</p>
      )}
    </section>
  );
}
