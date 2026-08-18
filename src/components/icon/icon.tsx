import {
  PHOSPHOR_ICONS,
  type PhosphorIconName,
  type PhosphorIconPath,
} from "./phosphor";

export type IconProps = {
  name: PhosphorIconName;
  /** Square size in pixels. Prefer the design-system icon size tokens. */
  size?: number;
  className?: string;
  /** Accessible name for icon-only controls; the icon is aria-hidden without one. */
  label?: string;
};

/**
 * Shared renderer for the bundled Phosphor icon set. See docs/iconography.md
 * for the rules — component code never inlines its own SVG paths.
 */
export function Icon({ name, size = 16, className, label }: IconProps) {
  const icon = PHOSPHOR_ICONS[name];
  const paths: readonly PhosphorIconPath[] = icon.paths;
  return (
    <svg
      width={size}
      height={size}
      viewBox={icon.viewBox}
      fill="currentColor"
      className={className}
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      {paths.map((path, index) => (
        <path
          key={index}
          d={path.d}
          fillRule={path.fillRule as "evenodd" | undefined}
        />
      ))}
    </svg>
  );
}
