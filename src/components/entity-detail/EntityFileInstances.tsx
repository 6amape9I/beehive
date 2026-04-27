import { formatDateTime, shortChecksum } from "../../lib/formatters";
import type { EntityFileAllowedActions, EntityFileRecord } from "../../types/domain";
import { StatusBadge } from "../StatusBadge";

interface EntityFileInstancesProps {
  files: EntityFileRecord[];
  fileAllowedActions: EntityFileAllowedActions[];
  selectedFileId: number | null;
  loadingFileAction: string | null;
  onSelectFile: (fileId: number) => void;
  onOpenFile: (fileId: number) => void;
  onOpenFolder: (fileId: number) => void;
}

export function EntityFileInstances({
  files,
  fileAllowedActions,
  selectedFileId,
  loadingFileAction,
  onSelectFile,
  onOpenFile,
  onOpenFolder,
}: EntityFileInstancesProps) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>File Instances</h2>
        <span className="muted">{files.length} file record(s)</span>
      </div>
      {files.length === 0 ? (
        <p className="empty-text">No file instances were recorded for this entity.</p>
      ) : (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Selected</th>
                <th>Stage</th>
                <th>Path</th>
                <th>Status</th>
                <th>Validation</th>
                <th>Presence</th>
                <th>Checksum</th>
                <th>Size</th>
                <th>Modified</th>
                <th>Managed copy</th>
                <th>Edit</th>
                <th>Open</th>
              </tr>
            </thead>
            <tbody>
              {files.map((file) => {
                const busy = loadingFileAction?.endsWith(`:${file.id}`) ?? false;
                const actions = fileAllowedActions.find(
                  (item) => item.entity_file_id === file.id,
                );
                return (
                  <tr key={file.id} className={selectedFileId === file.id ? "selected-row" : ""}>
                    <td>
                      <button
                        type="button"
                        className="button secondary"
                        onClick={() => onSelectFile(file.id)}
                      >
                        Select
                      </button>
                    </td>
                    <td>{file.stage_id}</td>
                    <td>
                      <code>{file.file_path}</code>
                    </td>
                    <td>
                      <StatusBadge status={file.status} />
                    </td>
                    <td>
                      <StatusBadge status={file.validation_status} />
                    </td>
                    <td>
                      {file.file_exists
                        ? "Present"
                        : `Missing since ${formatDateTime(file.missing_since)}`}
                    </td>
                    <td>
                      <code>{shortChecksum(file.checksum)}</code>
                    </td>
                    <td>{file.file_size}</td>
                    <td>{formatDateTime(file.file_mtime)}</td>
                    <td>{file.is_managed_copy ? "Yes" : "No"}</td>
                    <td title={actions?.reasons.join(" ")}>
                      {actions?.can_edit_business_json ? "Allowed" : "Locked"}
                    </td>
                    <td>
                      <div className="button-row">
                        <button
                          type="button"
                          className="button secondary"
                          disabled={busy || !(actions?.can_open_file ?? file.file_exists)}
                          onClick={() => onOpenFile(file.id)}
                        >
                          File
                        </button>
                        <button
                          type="button"
                          className="button secondary"
                          disabled={busy || actions?.can_open_folder === false}
                          onClick={() => onOpenFolder(file.id)}
                        >
                          Folder
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
