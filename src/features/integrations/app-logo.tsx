import slackLogo from "../../assets/apps/slack.svg?no-inline";
import telegramLogo from "../../assets/apps/telegram.svg?no-inline";
import whatsappLogo from "../../assets/apps/whatsapp.svg?no-inline";
import microsoftLogo from "../../assets/apps/microsoft.svg?no-inline";
import gmailLogo from "../../assets/apps/gmail.svg?no-inline";
import githubLogo from "../../assets/apps/github.svg?no-inline";
import linearLogo from "../../assets/apps/linear.svg?no-inline";
import sentryLogo from "../../assets/apps/sentry.svg?no-inline";
import notionLogo from "../../assets/apps/notion.svg?no-inline";
import obsidianLogo from "../../assets/apps/obsidian.svg?no-inline";
import googleDriveLogo from "../../assets/apps/google-drive.svg?no-inline";
import sharepointLogo from "../../assets/apps/sharepoint.svg?no-inline";

export type AppLogoConfig = {
  source: string;
};

/**
 * Shipped provider marks must be local, optimized SVGs. Every mark, including
 * the unknown-provider fallback, sits on the same always-white card so the
 * connected-apps list reads as one family of tiles.
 */
export const APP_LOGOS: Readonly<Record<string, AppLogoConfig>> = {
  slack: { source: slackLogo },
  telegram: { source: telegramLogo },
  whatsapp: { source: whatsappLogo },
  microsoft: { source: microsoftLogo },
  gmail: { source: gmailLogo },
  github: { source: githubLogo },
  linear: { source: linearLogo },
  sentry: { source: sentryLogo },
  notion: { source: notionLogo },
  obsidian: { source: obsidianLogo },
  google_drive: { source: googleDriveLogo },
  sharepoint: { source: sharepointLogo },
};

type Props = {
  providerId: string;
  providerName: string;
  size?: number;
};

/** Offline-safe provider logo with a deterministic fallback for future apps. */
export function AppLogo({ providerId, providerName, size = 32 }: Props) {
  const logo = APP_LOGOS[providerId];
  const initial = providerName.trim().charAt(0).toLocaleUpperCase() || "?";

  return (
    <span
      className={`app-logo${logo ? ` app-logo-${providerId}` : " is-fallback"}`}
      role="img"
      aria-label={`${providerName} logo`}
      style={{ width: size, height: size }}
    >
      {logo ? (
        <img src={logo.source} alt="" aria-hidden draggable={false} />
      ) : (
        <span aria-hidden>{initial}</span>
      )}
    </span>
  );
}
