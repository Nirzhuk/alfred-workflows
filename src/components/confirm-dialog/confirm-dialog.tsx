import { Modal, ModalHeader } from "../modal";

type Props = {
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
};

export function ConfirmDialog({
  title,
  message,
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  danger = false,
  onConfirm,
  onCancel,
}: Props) {
  return (
    <Modal
      size="sm"
      role="alertdialog"
      className={danger ? "confirm-modal is-danger" : "confirm-modal"}
      onClose={onCancel}
      labelledBy="confirm-dialog-title"
      describedBy="confirm-dialog-message"
    >
      <ModalHeader
        eyebrow="Confirm"
        title={title}
        titleId="confirm-dialog-title"
      />
      <div className="confirm-modal-body">
        <p id="confirm-dialog-message" className="muted">
          {message}
        </p>
        <div className="schedule-actions">
          <button type="button" className="ghost" onClick={onCancel}>
            {cancelLabel}
          </button>
          <button
            type="button"
            className={danger ? "primary danger-solid" : "primary"}
            onClick={onConfirm}
            autoFocus
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </Modal>
  );
}
