import {
  forwardRef,
  type ButtonHTMLAttributes,
  type CSSProperties,
  type HTMLAttributes,
  type ReactNode,
} from "react";

function cx(...parts: Array<string | false | null | undefined>) {
  return parts.filter(Boolean).join(" ");
}

export type MenuProps = HTMLAttributes<HTMLDivElement> & {
  /**
   * Positioning mode:
   * - `inline` — in normal flow (use inside DropdownMenuContent)
   * - `fixed` — viewport coordinates (ContextMenu)
   * - `absolute` — positioned by the caller
   */
  placement?: "inline" | "fixed" | "absolute";
  /** Enable enter/exit scale+fade (driven by `data-state`). */
  animated?: boolean;
  /** Open state for animated menus (`closed` | `open` | `closing`). */
  state?: "closed" | "open" | "closing";
};

/**
 * Shared menu surface — change `.ui-menu` styles once to restyle every menu.
 * Compose with `MenuLabel`, `MenuItem`, and `MenuSeparator` (shadcn-style).
 */
export const Menu = forwardRef<HTMLDivElement, MenuProps>(function Menu(
  {
    className,
    placement = "inline",
    animated = false,
    state = "open",
    children,
    onContextMenu,
    ...rest
  },
  ref,
) {
  return (
    <div
      ref={ref}
      role="menu"
      data-placement={placement}
      data-animated={animated ? "true" : "false"}
      data-state={state}
      className={cx("ui-menu", className)}
      onContextMenu={(e) => {
        e.preventDefault();
        onContextMenu?.(e);
      }}
      {...rest}
    >
      {children}
    </div>
  );
});

export type MenuLabelProps = HTMLAttributes<HTMLParagraphElement>;

export function MenuLabel({ className, ...rest }: MenuLabelProps) {
  return <p className={cx("ui-menu-label", className)} {...rest} />;
}

export type MenuDescriptionProps = HTMLAttributes<HTMLParagraphElement>;

export function MenuDescription({ className, ...rest }: MenuDescriptionProps) {
  return <p className={cx("ui-menu-description", className)} {...rest} />;
}

export type MenuSeparatorProps = HTMLAttributes<HTMLDivElement>;

export function MenuSeparator({ className, ...rest }: MenuSeparatorProps) {
  return (
    <div
      role="separator"
      className={cx("ui-menu-separator", className)}
      {...rest}
    />
  );
}

export type MenuItemProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  danger?: boolean;
  icon?: ReactNode;
  /** Called before the click handler; useful for closing the menu. */
  onSelect?: () => void;
};

export function MenuItem({
  className,
  danger = false,
  icon,
  onSelect,
  onClick,
  children,
  type = "button",
  ...rest
}: MenuItemProps) {
  return (
    <button
      type={type}
      role="menuitem"
      data-danger={danger ? "true" : undefined}
      className={cx("ui-menu-item", className)}
      onClick={(e) => {
        onSelect?.();
        onClick?.(e);
      }}
      {...rest}
    >
      {icon ? (
        <span className="ui-menu-icon" aria-hidden>
          {icon}
        </span>
      ) : null}
      {children}
    </button>
  );
}

export type MenuContentStyle = CSSProperties;
