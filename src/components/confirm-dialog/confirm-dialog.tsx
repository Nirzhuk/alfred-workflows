import { Icon } from "../icon";
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
        className="confirm-modal-header"
        leading={
          <span
            className={
              danger
                ? "confirm-modal-icon is-danger"
                : "confirm-modal-icon"
            }
          >
            <Icon name={danger ? "trash" : "question"} size={18} />
          </span>
        }
        title={title}
        titleId="confirm-dialog-title"
        description={message}
        descriptionId="confirm-dialog-message"
      />
      <footer className="confirm-modal-footer">
        <button
          type="button"
          className="ghost"
          onClick={onCancel}
          autoFocus
        >
          {cancelLabel}
        </button>
        <button
          type="button"
          className={danger ? "primary danger-solid" : "primary"}
          onClick={onConfirm}
        >
          {confirmLabel}
        </button>
      </footer>
    </Modal>
  );
}
