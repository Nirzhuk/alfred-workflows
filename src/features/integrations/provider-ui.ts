import type { ComponentType } from "react";
import type { ConnectDialogProps } from "./connect-dialog";
import { GitHubConnect } from "./github-connect";
import { GmailConnect } from "./gmail-connect";
import { LinearConnect } from "./linear-connect";
import { MicrosoftConnect } from "./microsoft-connect";
import { NotionPrivateConnect } from "./notion-private-connect";
import { ObsidianVaultConnect } from "./obsidian-vault-connect";
import { SentryConnect } from "./sentry-connect";
import { SlackPrivateConnect } from "./slack-private-connect";
import { TelegramConnect } from "./telegram-connect";
import { WhatsAppConnect } from "./whatsapp-connect";

export type { ConnectDialogProps } from "./connect-dialog";

export type ActiveConnect = {
  providerId: string;
  reconnectConnectionId?: string;
};

export type ProviderUi = {
  Dialog: ComponentType<ConnectDialogProps>;
  supportsReconnect: boolean;
};

export const PROVIDER_UI: Record<string, ProviderUi> = {
  slack: { Dialog: SlackPrivateConnect, supportsReconnect: true },
  telegram: { Dialog: TelegramConnect, supportsReconnect: false },
  whatsapp: { Dialog: WhatsAppConnect, supportsReconnect: false },
  notion: { Dialog: NotionPrivateConnect, supportsReconnect: true },
  linear: { Dialog: LinearConnect, supportsReconnect: true },
  sentry: { Dialog: SentryConnect, supportsReconnect: true },
  obsidian: { Dialog: ObsidianVaultConnect, supportsReconnect: true },
  github: { Dialog: GitHubConnect, supportsReconnect: true },
  gmail: { Dialog: GmailConnect, supportsReconnect: true },
  microsoft: { Dialog: MicrosoftConnect, supportsReconnect: true },
};
