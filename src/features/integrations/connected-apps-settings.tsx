import { useEffect, useMemo, useState } from "react";
import { ConfirmDialog } from "../../components/confirm-dialog";
import { AppLogo } from "./app-logo";
import { GitHubConnect } from "./github-connect";
import { NotionPrivateConnect } from "./notion-private-connect";
import { ObsidianVaultConnect } from "./obsidian-vault-connect";
import { SlackPrivateConnect } from "./slack-private-connect";
import { TelegramConnect } from "./telegram-connect";
import { WhatsAppConnect } from "./whatsapp-connect";
import { useIntegrationsStore } from "./store";
import type {
  AppConnection,
  AppConnectionStatus,
  AppConnectionUsage,
  AppProvider,
} from "./types";

const STATUS_LABELS: Record<AppConnectionStatus, string> = {
  connected: "Connected",
  expired: "Reconnect needed",
  error: "Needs attention",
  revoked: "Revoked locally",
};

type PendingDisconnect = {
  connection: AppConnection;
  usage: AppConnectionUsage;
};

export function ConnectedAppsSettings() {
  const providers = useIntegrationsStore((state) => state.providers);
  const connections = useIntegrationsStore((state) => state.connections);
  const loading = useIntegrationsStore((state) => state.loading);
  const disconnectingId = useIntegrationsStore(
    (state) => state.disconnectingId,
  );
  const error = useIntegrationsStore((state) => state.error);
  const load = useIntegrationsStore((state) => state.load);
  const refresh = useIntegrationsStore((state) => state.refresh);
  const getUsage = useIntegrationsStore((state) => state.getUsage);
  const disconnect = useIntegrationsStore((state) => state.disconnect);
  const clearError = useIntegrationsStore((state) => state.clearError);
  const [preparingId, setPreparingId] = useState<string | null>(null);
  const [pending, setPending] = useState<PendingDisconnect | null>(null);
  const [metadataCleanup, setMetadataCleanup] =
    useState<AppConnection | null>(null);
  const [slackConnectOpen, setSlackConnectOpen] = useState(false);
  const [telegramConnectOpen, setTelegramConnectOpen] = useState(false);
  const [notionConnectOpen, setNotionConnectOpen] = useState(false);
  const [obsidianConnectOpen, setObsidianConnectOpen] = useState(false);
  const [githubConnectOpen, setGithubConnectOpen] = useState(false);
  const [whatsappConnectOpen, setWhatsappConnectOpen] = useState(false);

  const connectHandlers: Record<string, () => void> = {
    slack: () => setSlackConnectOpen(true),
    telegram: () => setTelegramConnectOpen(true),
    whatsapp: () => setWhatsappConnectOpen(true),
    notion: () => setNotionConnectOpen(true),
    obsidian: () => setObsidianConnectOpen(true),
    github: () => setGithubConnectOpen(true),
  };

  useEffect(() => {
    void load();
  }, [load]);

  const rows = useMemo(() => {
    const knownIds = new Set(providers.map((provider) => provider.id));
    const unknownProviders = Array.from(
      new Map(
        connections
          .filter((connection) => !knownIds.has(connection.providerId))
          .map((connection) => [
            connection.providerId,
            {
              id: connection.providerId,
              name: connection.providerId,
              capabilitySummary: "Connected app",
              connectionModes: [connection.connectionMode],
              connectAvailable: false,
              experimental: false,
              singleConnection: false,
            } satisfies AppProvider,
          ]),
      ).values(),
    );
    return [...providers, ...unknownProviders].map((provider) => ({
      provider,
      connections: connections.filter(
        (connection) => connection.providerId === provider.id,
      ),
    }));
  }, [connections, providers]);

  const prepareDisconnect = async (connection: AppConnection) => {
    setPreparingId(connection.id);
    const usage = await getUsage(connection.id);
    setPreparingId(null);
    if (usage) setPending({ connection, usage });
  };

  const confirmDisconnect = async () => {
    if (!pending) return;
    const connection = pending.connection;
    setPending(null);
    const disconnectError = await disconnect(connection.id, false);
    if (disconnectError?.recoverable) setMetadataCleanup(connection);
  };

  const confirmMetadataCleanup = async () => {
    if (!metadataCleanup) return;
    const connection = metadataCleanup;
    setMetadataCleanup(null);
    await disconnect(connection.id, true);
  };

  return (
    <section className="settings-section">
      <div className="settings-section-heading">
        <div>
          <h2>Connected Apps</h2>
          <p className="settings-section-copy">
            Credentials stay in your system credential store. Workflow data
            remains local.
          </p>
        </div>
        <button
          type="button"
          className="ghost integrations-refresh"
          disabled={loading}
          onClick={() => void refresh()}
        >
          {loading ? "Refreshing…" : "Refresh"}
        </button>
      </div>

      {error ? (
        <div className="integrations-error" role="alert">
          <span>{error.message}</span>
          <button type="button" className="settings-link" onClick={clearError}>
            Dismiss
          </button>
        </div>
      ) : null}

      <div className="settings-card">
        {rows.length === 0 && loading ? (
          <div className="settings-row">
            <p className="settings-value">Loading connected apps…</p>
          </div>
        ) : null}
        {rows.map(({ provider, connections: providerConnections }) => (
          <div className="settings-row integration-provider-row" key={provider.id}>
            <div className="integration-provider-copy">
              <AppLogo
                providerId={provider.id}
                providerName={provider.name}
              />
              <div className="integration-provider-text">
                <p className="settings-label">
                  {provider.name}
                  {provider.experimental ? (
                    <span className="integration-experimental-badge">
                      Experimental
                    </span>
                  ) : null}
                </p>
                <p className="settings-value">{provider.capabilitySummary}</p>
              </div>
            </div>
            {providerConnections.length === 0 ? (
              <button
                type="button"
                className="ghost integration-action"
                disabled={
                  !provider.connectAvailable || !connectHandlers[provider.id]
                }
                title={
                  provider.connectAvailable
                    ? `Connect ${provider.name}`
                    : "Authorization arrives in the provider plan"
                }
                onClick={connectHandlers[provider.id]}
              >
                {provider.connectAvailable ? "Connect" : "Coming next"}
              </button>
            ) : (
              <div className="integration-connections">
                {providerConnections.map((connection) => (
                  <div className="integration-connection" key={connection.id}>
                    <div className="integration-connection-copy">
                      <div className="integration-connection-identity">
                        <span
                          className={`integration-status is-${connection.status}`}
                        >
                          {STATUS_LABELS[connection.status]}
                        </span>
                        <p
                          className="integration-account"
                          title={
                            connection.displayName ??
                            connection.externalAccountId ??
                            undefined
                          }
                        >
                          {connection.displayName ??
                            connection.externalAccountId ??
                            "Connected account"}
                        </p>
                      </div>
                      {connection.scopes.length > 0 ? (
                        <p className="settings-hint">
                          Access: {connection.scopes.join(", ")}
                        </p>
                      ) : null}
                    </div>
                    <div className="integration-actions">
                      {provider.id === "slack" ||
                      provider.id === "notion" ||
                      provider.id === "obsidian" ||
                      provider.id === "github" ? (
                        <button
                          type="button"
                          className="ghost integration-action"
                          title={`Reconnect ${provider.name}`}
                          onClick={connectHandlers[provider.id]}
                        >
                          Reconnect
                        </button>
                      ) : null}
                      <button
                        type="button"
                        className="ghost danger integration-action"
                        disabled={
                          disconnectingId === connection.id ||
                          preparingId === connection.id
                        }
                        onClick={() =>
                          connection.status === "revoked"
                            ? setMetadataCleanup(connection)
                            : void prepareDisconnect(connection)
                        }
                      >
                        {disconnectingId === connection.id ||
                        preparingId === connection.id
                          ? "Checking…"
                          : connection.status === "revoked"
                            ? "Remove local data"
                            : "Disconnect"}
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        ))}
      </div>

      {pending ? (
        <ConfirmDialog
          title={`Disconnect ${connectionLabel(pending.connection)}?`}
          message={disconnectMessage(pending)}
          confirmLabel="Disconnect"
          danger
          onCancel={() => setPending(null)}
          onConfirm={() => void confirmDisconnect()}
        />
      ) : null}

      {metadataCleanup ? (
        <ConfirmDialog
          title="Remove local metadata only?"
          message="Alfred could not remove the system credential. This removes only the local connection record and cannot revoke a remote provider grant. Revoke access at the provider and remove any stale keychain entry manually."
          confirmLabel="Remove local data"
          danger
          onCancel={() => setMetadataCleanup(null)}
          onConfirm={() => void confirmMetadataCleanup()}
        />
      ) : null}

      {slackConnectOpen ? (
        <SlackPrivateConnect onClose={() => setSlackConnectOpen(false)} />
      ) : null}
      {whatsappConnectOpen ? (
        <WhatsAppConnect onClose={() => setWhatsappConnectOpen(false)} />
      ) : null}
      {telegramConnectOpen ? (
        <TelegramConnect onClose={() => setTelegramConnectOpen(false)} />
      ) : null}
      {notionConnectOpen ? (
        <NotionPrivateConnect onClose={() => setNotionConnectOpen(false)} />
      ) : null}
      {obsidianConnectOpen ? (
        <ObsidianVaultConnect onClose={() => setObsidianConnectOpen(false)} />
      ) : null}
      {githubConnectOpen ? (
        <GitHubConnect onClose={() => setGithubConnectOpen(false)} />
      ) : null}
    </section>
  );
}

function connectionLabel(connection: AppConnection): string {
  return (
    connection.displayName ??
    connection.externalAccountId ??
    connection.providerId
  );
}

function dependencyMessage(usage: AppConnectionUsage): string {
  const allDependencies = [
    ...usage.workflows.map((item) => `workflow “${item.label}”`),
    ...usage.schedules.map(
      (item) => `${item.enabled ? "enabled" : "disabled"} schedule “${item.label}”`,
    ),
    ...usage.triggers.map(
      (item) => `${item.enabled ? "enabled" : "disabled"} trigger “${item.label}”`,
    ),
  ];
  if (allDependencies.length === 0) {
    return "No workflows, schedules, or triggers currently depend on this connection. Alfred will revoke it locally, remove its system credential, and then delete its metadata.";
  }
  const visible = allDependencies.slice(0, 8);
  const remaining = allDependencies.length - visible.length;
  const suffix = remaining > 0 ? ` and ${remaining} more` : "";
  return `This connection is used by ${visible.join(", ")}${suffix}. Those automations may stop working. Alfred will revoke it locally before removing its system credential and metadata.`;
}

function disconnectMessage(pending: PendingDisconnect): string {
  const dependencies = dependencyMessage(pending.usage);
  if (pending.connection.providerId !== "telegram") return dependencies;
  return `${dependencies} Alfred cannot revoke the BotFather token remotely. If the token should stop working, revoke or regenerate it with @BotFather after disconnecting.`;
}
