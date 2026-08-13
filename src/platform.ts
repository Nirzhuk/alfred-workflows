export type DesktopPlatform = "macos" | "windows" | "linux" | "unknown";

export type PlatformNavigator = {
  platform?: string;
  userAgent?: string;
  userAgentData?: {
    platform?: string;
  };
};

export function detectDesktopPlatform(
  source: PlatformNavigator | undefined =
    typeof navigator === "undefined" ? undefined : navigator,
): DesktopPlatform {
  if (!source) return "unknown";

  const identity = (
    source.userAgentData?.platform ??
    source.platform ??
    source.userAgent ??
    ""
  ).toLocaleLowerCase();

  if (/mac|darwin/.test(identity)) return "macos";
  if (/win/.test(identity)) return "windows";
  if (/linux|x11/.test(identity)) return "linux";
  return "unknown";
}

export function applyDesktopPlatform(
  root: Pick<HTMLElement, "dataset">,
  source?: PlatformNavigator,
): DesktopPlatform {
  const platform = detectDesktopPlatform(source);
  root.dataset.platform = platform;
  return platform;
}
