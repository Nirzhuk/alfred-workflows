import { useEffect, type RefObject } from "react";

/** Close on outside pointer, Escape, resize, or scroll. */
export function useDismissable(
  enabled: boolean,
  rootRef: RefObject<HTMLElement | null>,
  onDismiss: () => void,
  options?: { closeOnScroll?: boolean; closeOnResize?: boolean },
) {
  const closeOnScroll = options?.closeOnScroll ?? true;
  const closeOnResize = options?.closeOnResize ?? true;

  useEffect(() => {
    if (!enabled) return;

    const onPointer = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) onDismiss();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onDismiss();
    };

    window.addEventListener("pointerdown", onPointer);
    window.addEventListener("keydown", onKey);
    if (closeOnResize) window.addEventListener("resize", onDismiss);
    if (closeOnScroll) window.addEventListener("scroll", onDismiss, true);

    return () => {
      window.removeEventListener("pointerdown", onPointer);
      window.removeEventListener("keydown", onKey);
      if (closeOnResize) window.removeEventListener("resize", onDismiss);
      if (closeOnScroll) {
        window.removeEventListener("scroll", onDismiss, true);
      }
    };
  }, [enabled, rootRef, onDismiss, closeOnScroll, closeOnResize]);
}
