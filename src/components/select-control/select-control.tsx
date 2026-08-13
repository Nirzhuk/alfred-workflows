import {
  forwardRef,
  type SelectHTMLAttributes,
} from "react";

function cx(...parts: Array<string | false | null | undefined>) {
  return parts.filter(Boolean).join(" ");
}

export type SelectControlProps = SelectHTMLAttributes<HTMLSelectElement> & {
  containerClassName?: string;
  density?: "compact" | "default";
};

/**
 * Native select semantics with Alfred's shared field chrome.
 *
 * The browser still owns the option menu and keyboard behavior. The wrapper
 * only normalizes the field surface and replaces platform-specific arrows.
 */
export const SelectControl = forwardRef<
  HTMLSelectElement,
  SelectControlProps
>(function SelectControl(
  {
    children,
    className,
    containerClassName,
    density = "default",
    ...props
  },
  ref,
) {
  return (
    <span
      className={cx(
        "ui-select",
        density === "compact" && "is-compact",
        containerClassName,
      )}
    >
      <select ref={ref} className={cx("ui-select-input", className)} {...props}>
        {children}
      </select>
      <span className="ui-select-chevron" aria-hidden>
        <svg viewBox="0 0 16 16" fill="none">
          <path
            d="m5.25 6.5 2.75 3 2.75-3"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </span>
    </span>
  );
});
