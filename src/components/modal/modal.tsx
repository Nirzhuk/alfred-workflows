import {
  useEffect,
  type HTMLAttributes,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

export type ModalSize = "sm" | "md" | "lg" | "settings" | "xl";

type ModalProps = {
  open?: boolean;
  onClose: () => void;
  size?: ModalSize;
  role?: "dialog" | "alertdialog";
  /** Extra class on the panel (size class is always applied). */
  className?: string;
  labelledBy?: string;
  /** Used as `aria-label` when `labelledBy` is omitted. */
  label?: string;
  describedBy?: string;
  closeOnBackdrop?: boolean;
  closeOnEscape?: boolean;
  children: ReactNode;
};

const SIZE_CLASS: Record<ModalSize, string> = {
  sm: "modal--sm",
  md: "modal--md",
  lg: "modal--lg",
  settings: "modal--settings",
  xl: "modal--xl",
};

/**
 * Shared modal shell: portal to `document.body`, backdrop dismiss, Escape,
 * and size variants used across Alfred dialogs.
 */
export function Modal({
  open = true,
  onClose,
  size = "lg",
  role = "dialog",
  className,
  labelledBy,
  label,
  describedBy,
  closeOnBackdrop = true,
  closeOnEscape = true,
  children,
}: ModalProps) {
  useEffect(() => {
    if (!open || !closeOnEscape) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, closeOnEscape, onClose]);

  useEffect(() => {
    if (!open) return;
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = previous;
    };
  }, [open]);

  if (!open) return null;

  const panelClass = ["modal", SIZE_CLASS[size], className]
    .filter(Boolean)
    .join(" ");

  return createPortal(
    <div
      className="modal-backdrop"
      role="presentation"
      onClick={closeOnBackdrop ? onClose : undefined}
    >
      <div
        className={panelClass}
        role={role}
        aria-modal="true"
        aria-labelledby={labelledBy}
        aria-label={labelledBy ? undefined : label}
        aria-describedby={describedBy}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}

type ModalHeaderProps = {
  eyebrow?: ReactNode;
  title: ReactNode;
  titleId?: string;
  titleAs?: "h2" | "h3";
  description?: ReactNode;
  /** Extra content under the title block (e.g. stats line). */
  children?: ReactNode;
  actions?: ReactNode;
  /** Use the denser / stronger header surface (Memories). */
  strong?: boolean;
  className?: string;
};

export function ModalHeader({
  eyebrow,
  title,
  titleId,
  titleAs = "h3",
  description,
  children,
  actions,
  strong = false,
  className,
}: ModalHeaderProps) {
  const TitleTag = titleAs;
  const headerClass = [
    "modal-header",
    strong ? "modal-header--strong" : null,
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <header className={headerClass}>
      <div>
        {eyebrow ? <p className="run-active-label">{eyebrow}</p> : null}
        <TitleTag id={titleId}>{title}</TitleTag>
        {description ? <p className="muted">{description}</p> : null}
        {children}
      </div>
      {actions ? <div className="modal-actions">{actions}</div> : null}
    </header>
  );
}

type ModalBodyProps = HTMLAttributes<HTMLDivElement> & {
  children: ReactNode;
};

/** Scrollable content region with standard modal body padding. */
export function ModalBody({ className, children, ...rest }: ModalBodyProps) {
  return (
    <div
      className={["modal-body", className].filter(Boolean).join(" ")}
      {...rest}
    >
      {children}
    </div>
  );
}
