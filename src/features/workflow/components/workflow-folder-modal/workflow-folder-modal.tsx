import { useEffect, useRef, useState } from "react";
import { Modal, ModalHeader } from "../../../../components/modal";
import { useWorkflowStore } from "../../store";

type Props = {
  folder?: { id: string; name: string } | null;
  onClose: () => void;
};

export function WorkflowFolderModal({ folder, onClose }: Props) {
  const loading = useWorkflowStore((state) => state.loading);
  const createFolder = useWorkflowStore((state) => state.createWorkflowFolder);
  const renameFolder = useWorkflowStore((state) => state.renameWorkflowFolder);
  const [name, setName] = useState(folder?.name ?? "");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const trimmed = name.trim();
  const canSave =
    trimmed.length > 0 && trimmed !== folder?.name.trim() && !loading;

  const submit = () => {
    if (!canSave) return;
    void (async () => {
      if (folder) await renameFolder(folder.id, trimmed);
      else await createFolder(trimmed);
      if (!useWorkflowStore.getState().error) onClose();
    })();
  };

  const title = folder ? "Rename folder" : "New folder";

  return (
    <Modal size="md" onClose={onClose} labelledBy="workflow-folder-modal-title">
      <ModalHeader
        eyebrow="Workflows"
        title={title}
        titleId="workflow-folder-modal-title"
        actions={
          <button type="button" className="ghost" onClick={onClose}>
            Cancel
          </button>
        }
      />

      <form
        className="rename-modal-body"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <label className="field">
          <span>Name</span>
          <input
            ref={inputRef}
            type="text"
            value={name}
            maxLength={80}
            placeholder="e.g. Client projects"
            onChange={(event) => setName(event.target.value)}
          />
        </label>

        <div className="schedule-actions">
          <button type="submit" className="primary" disabled={!canSave}>
            {folder ? "Save name" : "Create folder"}
          </button>
        </div>
      </form>
    </Modal>
  );
}
