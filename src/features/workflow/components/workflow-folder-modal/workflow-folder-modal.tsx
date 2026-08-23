import { useEffect, useRef, useState } from "react";
import { Icon } from "../../../../components/icon";
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
    <Modal
      size="md"
      className="compact-form-modal"
      onClose={onClose}
      labelledBy="workflow-folder-modal-title"
      describedBy="workflow-folder-modal-description"
    >
      <ModalHeader
        leading={
          <span className="modal-identity-icon">
            <Icon name={folder ? "folder" : "folder-plus"} size={20} />
          </span>
        }
        title={title}
        titleId="workflow-folder-modal-title"
        description={
          folder
            ? "Update this folder name across the workflow library."
            : "Create a folder to keep related workflows together."
        }
        descriptionId="workflow-folder-modal-description"
        actions={
          <button
            type="button"
            className="ghost modal-close-button"
            aria-label="Close"
            onClick={onClose}
          >
            <Icon name="x" size={16} />
          </button>
        }
      />

      <form
        className="compact-form-modal-form"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <div className="compact-form-modal-body">
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
        </div>

        <footer className="compact-form-modal-footer">
          <button type="submit" className="primary" disabled={!canSave}>
            {folder ? "Save name" : "Create folder"}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
