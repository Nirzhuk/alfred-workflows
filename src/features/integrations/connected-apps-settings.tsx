import { useEffect, useMemo, useState } from "react";
import { ConfirmDialog } from "../../components/confirm-dialog";
import { AppLogo } from "./app-logo";
import { PROVIDER_UI, type ActiveConnect } from "./provider-ui";
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
  const getUsage = useIntegrationsStore((state) => state.getUsage);
  const disconnect = useIntegrationsStore((state) => state.disconnect);
  const clearError = useIntegrationsStore((state) => state.clearError);
  const [preparingId, setPreparingId] = useState<string | null>(null);
  const [pending, setPending] = useState<PendingDisconnect | null>(null);
  const [metadataCleanup, setMetadataCleanup] =
    useState<AppConnection | null>(null);
  const [activeConnect, setActiveConnect] = useState<ActiveConnect | null>(
    null,
  );

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

  const ActiveDialog = activeConnect
    ? PROVIDER_UI[activeConnect.providerId]?.Dialog
    : undefined;

  return (
    <section className="settings-section">
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
        {rows.map(({ provider, connections: providerConnections }) => {
          const ui = PROVIDER_UI[provider.id];
          return (
            <div className="settings-row integration-provider-row" key={provider.id}>
              <div className="integration-provider-copy">
                <AppLogo
                  providerId={provider.id}
                  providerName={provider.name}
                  size={36}
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
                provider.connectAvailable && ui ? (
                  <button
                    type="button"
                    className="integration-action integration-connect"
                    title={`Connect ${provider.name}`}
                    onClick={() =>
                      setActiveConnect({ providerId: provider.id })
                    }
                  >
                    Connect
                  </button>
                ) : (
                  <span
                    className="integration-pending"
                    title="Authorization arrives in the provider plan"
                  >
                    Coming next
                  </span>
                )
              ) : (
                <div className="integration-connections">
                  {providerConnections.map((connection) => (
                    <div className="integration-connection" key={connection.id}>
                      <div className="integration-connection-copy">
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
                        <div className="integration-connection-meta">
                          <span
                            className={`integration-status is-${connection.status}`}
                          >
                            {STATUS_LABELS[connection.status]}
                          </span>
                          {connection.scopes.length > 0 ? (
                            <span
                              className="integration-scopes"
                              title={`Access: ${connection.scopes.join(", ")}`}
                            >
                              {connection.scopes.length} scope
                              {connection.scopes.length === 1 ? "" : "s"}
                            </span>
                          ) : null}
                        </div>
                      </div>
                      <div className="integration-actions">
                        {ui?.supportsReconnect ? (
                          <button
                            type="button"
                            className="ghost integration-action"
                            title={`Reconnect ${provider.name}`}
                            onClick={() =>
                              setActiveConnect({
                                providerId: provider.id,
                                reconnectConnectionId: connection.id,
                              })
                            }
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
          );
        })}
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

      {ActiveDialog ? (
        <ActiveDialog
          reconnectConnectionId={activeConnect?.reconnectConnectionId ?? null}
          onClose={() => setActiveConnect(null)}
        />
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
