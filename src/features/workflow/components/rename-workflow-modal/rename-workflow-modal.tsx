import { useEffect, useRef, useState } from "react";
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
      onClose={onClose}
      labelledBy="rename-modal-title"
    >
      <ModalHeader
        eyebrow="Workflow"
        title="Rename"
        titleId="rename-modal-title"
        actions={
          <button type="button" className="ghost" onClick={onClose}>
            Cancel
          </button>
        }
      />

      <form
        className="rename-modal-body"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
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

        <div className="schedule-actions">
          <button type="submit" className="primary" disabled={!canSave}>
            Save name
          </button>
        </div>
      </form>
    </Modal>
  );
}
