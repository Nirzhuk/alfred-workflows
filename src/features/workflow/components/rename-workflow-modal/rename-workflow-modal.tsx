import { useEffect, useRef, useState } from "react";
import { Icon } from "../../../../components/icon";
import { Modal, ModalHeader } from "../../../../components/modal";
import { useWorkflowStore } from "../../store";

type Props = {
  workflowId: string;
  workflowName: string;
  onClose: () => void;
};

export function RenameWorkflowModal({
  workflowId,
  workflowName,
  onClose,
}: Props) {
  const loading = useWorkflowStore((s) => s.loading);
  const renameWorkflow = useWorkflowStore((s) => s.renameWorkflow);
  const [name, setName] = useState(workflowName);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setName(workflowName);
  }, [workflowName]);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const trimmed = name.trim();
  const canSave = trimmed.length > 0 && trimmed !== workflowName && !loading;

  const submit = () => {
    if (!canSave) return;
    void (async () => {
      await renameWorkflow(workflowId, trimmed);
      if (!useWorkflowStore.getState().error) onClose();
    })();
  };

  return (
    <Modal
      size="md"
      className="compact-form-modal"
      onClose={onClose}
      labelledBy="rename-modal-title"
      describedBy="rename-modal-description"
    >
      <ModalHeader
        leading={
          <span className="modal-identity-icon">
            <Icon name="pencil-simple" size={20} />
          </span>
        }
        title="Rename workflow"
        titleId="rename-modal-title"
        description="Update the name shown in your workflow library."
        descriptionId="rename-modal-description"
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
        onSubmit={(e) => {
          e.preventDefault();
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
              maxLength={120}
              placeholder="Workflow name"
              onChange={(e) => setName(e.target.value)}
            />
          </label>
        </div>

        <footer className="compact-form-modal-footer">
          <button type="submit" className="primary" disabled={!canSave}>
            Save name
          </button>
        </footer>
      </form>
    </Modal>
  );
}
