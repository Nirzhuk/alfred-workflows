import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type HTMLAttributes,
  type ReactNode,
} from "react";

function cx(...parts: Array<string | false | null | undefined>) {
  return parts.filter(Boolean).join(" ");
}

type MenuSubContextValue = {
  open: boolean;
  setOpen: (open: boolean) => void;
  triggerId: string;
  contentId: string;
  triggerRef: React.RefObject<HTMLButtonElement | null>;
  contentRef: React.RefObject<HTMLDivElement | null>;
  side: "left" | "right";
};

const MenuSubContext = createContext<MenuSubContextValue | null>(null);

function useMenuSub() {
  const ctx = useContext(MenuSubContext);
  if (!ctx) {
    throw new Error("MenuSub components must be used within MenuSub");
  }
  return ctx;
}

/** Coordinates sibling submenus so only one is open at a time. */
type MenuSubGroupContextValue = {
  openId: string | null;
  openSub: (id: string) => void;
  closeSub: (id: string) => void;
};

const MenuSubGroupContext = createContext<MenuSubGroupContextValue | null>(
  null,
);

export function MenuSubGroup({ children }: { children: ReactNode }) {
  const [openId, setOpenId] = useState<string | null>(null);
  const openSub = useCallback((id: string) => setOpenId(id), []);
  const closeSub = useCallback(
    (id: string) => setOpenId((current) => (current === id ? null : current)),
    [],
  );
  const value = useMemo(
    () => ({ openId, openSub, closeSub }),
    [openId, openSub, closeSub],
  );
  return (
    <MenuSubGroupContext.Provider value={value}>
      {children}
    </MenuSubGroupContext.Provider>
  );
}

const OPEN_DELAY_MS = 100;
const CLOSE_DELAY_MS = 150;

type MenuSubProps = {
  children: ReactNode;
  className?: string;
};

export function MenuSub({ children, className }: MenuSubProps) {
  const id = useId();
  const triggerId = `${id}-trigger`;
  const contentId = `${id}-content`;
  const triggerRef = useRef<HTMLButtonElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const openTimer = useRef<number | null>(null);
  const closeTimer = useRef<number | null>(null);
  const group = useContext(MenuSubGroupContext);

  const [localOpen, setLocalOpen] = useState(false);
  const isOpen = group ? group.openId === id : localOpen;

  const setOpen = useCallback(
    (next: boolean) => {
      if (group) {
        if (next) group.openSub(id);
        else group.closeSub(id);
      } else {
        setLocalOpen(next);
      }
    },
    [group, id],
  );

  const clearTimers = useCallback(() => {
    if (openTimer.current != null) {
      window.clearTimeout(openTimer.current);
      openTimer.current = null;
    }
    if (closeTimer.current != null) {
      window.clearTimeout(closeTimer.current);
      closeTimer.current = null;
    }
  }, []);

  const scheduleOpen = useCallback(() => {
    clearTimers();
    openTimer.current = window.setTimeout(() => setOpen(true), OPEN_DELAY_MS);
  }, [clearTimers, setOpen]);

  const scheduleClose = useCallback(() => {
    clearTimers();
    closeTimer.current = window.setTimeout(
      () => setOpen(false),
      CLOSE_DELAY_MS,
    );
  }, [clearTimers, setOpen]);

  useEffect(() => () => clearTimers(), [clearTimers]);

  // Escape closes this submenu first (capture), before the root menu.
  useEffect(() => {
    if (!isOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopImmediatePropagation();
      e.preventDefault();
      setOpen(false);
      triggerRef.current?.focus();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [isOpen, setOpen]);

  const [side, setSide] = useState<"left" | "right">("right");

  useLayoutEffect(() => {
    if (!isOpen) return;
    const trigger = triggerRef.current;
    const content = contentRef.current;
    if (!trigger) return;

    const triggerRect = trigger.getBoundingClientRect();
    const contentWidth = content?.offsetWidth || 180;
    const pad = 8;
    const spaceRight = window.innerWidth - triggerRect.right - pad;
    setSide(spaceRight >= contentWidth ? "right" : "left");

    // Remeasure after paint once content size is accurate.
    const frame = requestAnimationFrame(() => {
      const width = contentRef.current?.offsetWidth || contentWidth;
      const right = window.innerWidth - trigger.getBoundingClientRect().right - pad;
      setSide(right >= width ? "right" : "left");
    });
    return () => cancelAnimationFrame(frame);
  }, [isOpen]);

  const value = useMemo(
    () => ({
      open: isOpen,
      setOpen,
      triggerId,
      contentId,
      triggerRef,
      contentRef,
      side,
    }),
    [isOpen, setOpen, triggerId, contentId, side],
  );

  return (
    <MenuSubContext.Provider value={value}>
      <div
        className={cx("ui-menu-sub", className)}
        data-state={isOpen ? "open" : "closed"}
        data-side={side}
        onMouseEnter={scheduleOpen}
        onMouseLeave={scheduleClose}
      >
        {children}
      </div>
    </MenuSubContext.Provider>
  );
}

type SubTriggerProps = ButtonHTMLAttributes<HTMLButtonElement>;

export function MenuSubTrigger({
  className,
  children,
  onClick,
  ...rest
}: SubTriggerProps) {
  const { open, setOpen, triggerId, contentId, triggerRef } = useMenuSub();

  return (
    <button
      ref={triggerRef}
      type="button"
      id={triggerId}
      role="menuitem"
      aria-haspopup="menu"
      aria-expanded={open}
      aria-controls={contentId}
      className={cx("ui-menu-item", "ui-menu-sub-trigger", className)}
      data-state={open ? "open" : "closed"}
      onClick={(e) => {
        setOpen(!open);
        onClick?.(e);
      }}
      onKeyDown={(e) => {
        if (e.key === "ArrowRight" || e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          setOpen(true);
        }
        if (e.key === "ArrowLeft") {
          e.preventDefault();
          setOpen(false);
        }
      }}
      {...rest}
    >
      <span className="ui-menu-sub-trigger-label">{children}</span>
      <span className="ui-menu-sub-chevron" aria-hidden>
        ›
      </span>
    </button>
  );
}

type SubContentProps = HTMLAttributes<HTMLDivElement>;

export function MenuSubContent({
  className,
  children,
  ...rest
}: SubContentProps) {
  const { open, contentId, triggerId, contentRef, side } = useMenuSub();

  if (!open) return null;

  return (
    <div
      ref={contentRef}
      id={contentId}
      role="menu"
      aria-labelledby={triggerId}
      data-side={side}
      className={cx("ui-menu", "ui-menu-sub-content", className)}
      onMouseDown={(e) => e.stopPropagation()}
      {...rest}
    >
      {children}
    </div>
  );
}
