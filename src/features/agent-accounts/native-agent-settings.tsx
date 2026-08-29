import { useEffect, useMemo, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ConfirmDialog } from "../../components/confirm-dialog";
import { AgentMark } from "../../components/agent-mark";
import { CursorNativeDisclosure } from "./cursor-native-disclosure";
import { GrokNativeDisclosure } from "./grok-native-disclosure";
import { NativeApiKeyConnect } from "./components/native-api-key-connect";
import {
  isManagedProductId,
  ManagedRuntimeSettings,
} from "./components/managed-runtime-settings";
import { OpenCodeNativeDisclosure } from "./opencode-native-disclosure";
import { useAgentAccountsStore } from "./store";
import type {
  ManagedRuntimeConnectionStatus,
  ManagedRuntimeProduct,
} from "./managed-runtime-types";
import type {
  AgentAccount,
  AgentAccountStatus,
  AgentAuthMethod,
  AgentProductId,
  AgentProviderRegistration,
} from "./types";
import { usesAlfredManagedApiKey } from "./types";

const STATUS_LABELS: Record<AgentAccountStatus, string> = {
  connected: "Connected",
  expired: "Reconnect needed",
  error: "Needs attention",
  revoked: "Revoked",
  disconnect_pending: "Disconnect incomplete",
};

const AUTH_LABELS: Record<AgentAuthMethod, string> = {
  oauth_pkce: "OAuth with PKCE",
  device_code: "Device authorization",
  api_key: "API key",
  runtime: "Provider runtime",
};

const GATE_MESSAGES: Record<string, string> = {
  native_provider_not_available:
    "Native account support is gated until this provider's public-client or isolated runtime integration ships.",
  claude_live_api_key_smoke_missing:
    "API-key setup is available. Native Claude runs remain blocked until the live API-key smoke gate passes.",
  gemini_live_api_key_smoke_missing:
    "API-key setup is available. Native Gemini runs remain blocked until the live API-key smoke gate passes.",
  grok_live_api_key_smoke_missing:
    "API-key setup is available. Native Grok runs remain blocked until the live API-key smoke gate passes.",
};

type NativeAgentSettingsProps = {
  snapshot?: {
    providers: AgentProviderRegistration[];
    accounts: AgentAccount[];
    managedRuntime?: {
      products: ManagedRuntimeProduct[];
      statuses?: ManagedRuntimeConnectionStatus[];
    };
  };
};

export function NativeAgentSettings({ snapshot }: NativeAgentSettingsProps = {}) {
  const providers = useAgentAccountsStore((state) => state.providers);
  const accounts = useAgentAccountsStore((state) => state.accounts);
  const attempts = useAgentAccountsStore((state) => state.attempts);
  const loading = useAgentAccountsStore((state) => state.loading);
  const busyId = useAgentAccountsStore((state) => state.busyId);
  const error = useAgentAccountsStore((state) => state.error);
  const load = useAgentAccountsStore((state) => state.load);
  const start = useAgentAccountsStore((state) => state.start);
  const complete = useAgentAccountsStore((state) => state.complete);
  const cancel = useAgentAccountsStore((state) => state.cancel);
  const refresh = useAgentAccountsStore((state) => state.refresh);
  const disconnect = useAgentAccountsStore((state) => state.disconnect);
  const clearError = useAgentAccountsStore((state) => state.clearError);
  const [pendingDisconnect, setPendingDisconnect] = useState<AgentAccount | null>(
    null,
  );
  const [pendingCleanup, setPendingCleanup] = useState<AgentAccount | null>(
    null,
  );
  const [pendingApiKey, setPendingApiKey] = useState<{
    providerId: string;
    providerName: string;
    productId: AgentProductId;
    accountId?: string;
  } | null>(null);
  const [apiKeysOpen, setApiKeysOpen] = useState(false);

  useEffect(() => {
    void load();
  }, [load]);

  const rows = useMemo(
    () => {
      return (snapshot?.providers ?? providers)
        .filter((provider) => isManagedProductId(provider.productId) === false)
        .map((provider) => ({
          provider,
          accounts: (snapshot?.accounts ?? accounts).filter(
            (account) =>
              isManagedProductId(account.productId) === false &&
              account.providerId === provider.providerId &&
              account.productId === provider.productId,
          ),
        }));
    },
    [accounts, providers, snapshot],
  );

  const begin = async (providerId: string, productId: AgentProductId) => {
    const attempt = await start(providerId, productId);
    if (
      attempt?.authorizationUrl &&
      attempt.authorizationUrl.startsWith("https://")
    ) {
      await openUrl(attempt.authorizationUrl);
    }
  };

  const confirmDisconnect = async () => {
    if (!pendingDisconnect) return;
    const account = pendingDisconnect;
    setPendingDisconnect(null);
    const removed = await disconnect(account.id);
    if (!removed) {
      const latest = useAgentAccountsStore
        .getState()
        .accounts.find((item) => item.id === account.id);
      if (latest?.status === "disconnect_pending") setPendingCleanup(latest);
    }
  };

  return (
    <section className="settings-section" aria-label="Native agent accounts">
      <p className="settings-value native-agent-intro">
        Sign in with Claude or ChatGPT here. Alfred opens the provider sign-in.
      </p>

      <ManagedRuntimeSettings
        accounts={snapshot?.accounts ?? accounts}
        snapshot={snapshot?.managedRuntime}
      />

      {error ? (
        <div className="integrations-error" role="alert">
          <span>{error.message}</span>
          <button type="button" className="settings-link" onClick={clearError}>
            Dismiss
          </button>
        </div>
      ) : null}

      {rows.length > 0 || loading ? (
      <section className="native-agent-api-keys" aria-labelledby="native-agent-api-keys-heading">
        <div className="settings-section-heading">
          <div>
            <h2 id="native-agent-api-keys-heading">API keys</h2>
            <p className="settings-section-copy">
              Optional developer keys. Claude and ChatGPT above do not need these.
            </p>
          </div>
          <button
            type="button"
            className="ghost settings-header-action"
            aria-expanded={apiKeysOpen}
            onClick={() => setApiKeysOpen(apiKeysOpen === false)}
          >
            {apiKeysOpen ? "Hide" : "Show"}
          </button>
        </div>
        <div className="settings-card" hidden={apiKeysOpen === false}>
        {rows.length === 0 && loading ? (
          <div className="settings-row">
            <p className="settings-value">Loading API key providers...</p>
          </div>
        ) : null}

        {rows.map(({ provider, accounts: providerAccounts }) => {
          const attempt = attempts[provider.productId];
          const providerBusy = busyId === provider.productId;
          return (
            <div
              className="settings-row integration-provider-row"
              key={provider.productId}
            >
              <div className="integration-provider-copy">
                <span className="native-agent-provider-mark" aria-hidden>
                  <AgentMark
                    provider={provider.providerId}
                    label={provider.providerName}
                    size={20}
                  />
                </span>
                <div className="integration-provider-text">
                  <p className="settings-label">{provider.productName}</p>
                  <p className="settings-value">
                    {providerAuthLabel(provider)}
                  </p>
                  {provider.providerId === "cursor" ? (
                    <CursorNativeDisclosure
                      connectAvailable={provider.connectAvailable}
                    />
                  ) : null}
                  {provider.providerId === "grok" ? (
                    <GrokNativeDisclosure
                      connectAvailable={provider.connectAvailable}
                    />
                  ) : null}
                  {provider.providerId === "opencode" ? (
                    <OpenCodeNativeDisclosure
                      connectAvailable={provider.connectAvailable}
                    />
                  ) : null}
                  {GATE_MESSAGES[provider.gateCode ?? ""] ? (
                    <p className="settings-value native-agent-gate">
                      {GATE_MESSAGES[provider.gateCode ?? ""]}
                    </p>
                  ) : null}
                </div>
              </div>

              {providerAccounts.length === 0 ? (
                attempt ? (
                  <div className="integration-actions native-agent-attempt-actions">
                    {attempt.userCode ? (
                      <code className="native-agent-user-code">
                        {attempt.userCode}
                      </code>
                    ) : null}
                    <button
                      type="button"
                      className="integration-action integration-connect"
                      disabled={providerBusy}
                      onClick={() => void complete(provider.productId)}
                    >
                      {providerBusy ? "Finishing..." : "Finish"}
                    </button>
                    <button
                      type="button"
                      className="ghost integration-action"
                      onClick={() => void cancel(provider.productId)}
                    >
                      Cancel
                    </button>
                  </div>
                ) : provider.connectAvailable ? (
                  <button
                    type="button"
                    className="integration-action integration-connect"
                    disabled={providerBusy}
                    onClick={() => {
                      if (
                        usesAlfredManagedApiKey(
                          provider.authMethods,
                          provider.credentialCustody,
                        )
                      ) {
                        clearError();
                        setPendingApiKey({
                          providerId: provider.providerId,
                          providerName: provider.providerName,
                          productId: provider.productId,
                        });
                      } else {
                        void begin(provider.providerId, provider.productId);
                      }
                    }}
                  >
                    {providerBusy
                      ? "Starting..."
                      : usesAlfredManagedApiKey(
                            provider.authMethods,
                            provider.credentialCustody,
                          )
                        ? "Add key"
                        : "Connect"}
                  </button>
                ) : null
              ) : (
                <div className="integration-connections">
                  {providerAccounts.map((account) => (
                    <AgentAccountRow
                      key={account.id}
                      account={account}
                      busy={busyId === account.id || providerBusy}
                      reconnectAvailable={provider.connectAvailable}
                      reconnect={() => {
                        if (
                          usesAlfredManagedApiKey(
                            account.authMethod,
                            account.custodyMode,
                          )
                        ) {
                          clearError();
                          setPendingApiKey({
                            providerId: provider.providerId,
                            providerName: provider.providerName,
                            productId: provider.productId,
                            accountId: account.id,
                          });
                        } else {
                          void begin(provider.providerId, provider.productId);
                        }
                      }}
                      refresh={() => void refresh(account.id)}
                      disconnect={() => setPendingDisconnect(account)}
                      cleanup={() => setPendingCleanup(account)}
                    />
                  ))}
                </div>
              )}
            </div>
          );
        })}
        </div>
      </section>
      ) : null}

      {pendingDisconnect ? (
        <ConfirmDialog
          title={`Disconnect ${accountLabel(pendingDisconnect)}?`}
          message={disconnectMessageFor(pendingDisconnect)}
          confirmLabel="Disconnect"
          danger
          onCancel={() => setPendingDisconnect(null)}
          onConfirm={() => void confirmDisconnect()}
        />
      ) : null}

      {pendingCleanup ? (
        <ConfirmDialog
          title="Remove local metadata only?"
          message={cleanupMessageFor(pendingCleanup)}
          confirmLabel="Remove local data"
          danger
          onCancel={() => setPendingCleanup(null)}
          onConfirm={() => {
            const account = pendingCleanup;
            setPendingCleanup(null);
            void disconnect(account.id, true);
          }}
        />
      ) : null}

      {pendingApiKey ? (
        <NativeApiKeyConnect
          providerId={pendingApiKey.providerId}
          providerName={pendingApiKey.providerName}
          productId={pendingApiKey.productId}
          accountId={pendingApiKey.accountId}
          onClose={() => setPendingApiKey(null)}
        />
      ) : null}
    </section>
  );
}

type RowProps = {
  account: AgentAccount;
  busy: boolean;
  reconnectAvailable: boolean;
  reconnect: () => void;
  refresh: () => void;
  disconnect: () => void;
  cleanup: () => void;
};

function AgentAccountRow({
  account,
  busy,
  reconnectAvailable,
  reconnect,
  refresh,
  disconnect,
  cleanup,
}: RowProps) {
  return (
    <div className="integration-connection">
      <div className="integration-connection-copy">
        <p className="integration-account">{accountLabel(account)}</p>
        <div className="integration-connection-meta">
          <span className={`integration-status is-${account.status}`}>
            {STATUS_LABELS[account.status]}
          </span>
          <span className="integration-scopes">
            {accountAuthLabel(account)}
          </span>
          {account.expiresAt ? (
            <span title={account.expiresAt}>
              Expires {safeDate(account.expiresAt)}
            </span>
          ) : null}
          {account.lastCheckedAt ? (
            <span title={account.lastCheckedAt}>
              Checked {safeDate(account.lastCheckedAt)}
            </span>
          ) : null}
        </div>
      </div>
      <div className="integration-actions">
        {reconnectAvailable && account.status !== "disconnect_pending" ? (
          <button
            type="button"
            className="ghost integration-action"
            disabled={busy}
            onClick={reconnect}
          >
            Reconnect
          </button>
        ) : null}
        {reconnectAvailable &&
        account.authMethod !== "api_key" &&
        !(["revoked", "disconnect_pending"] as AgentAccountStatus[]).includes(
          account.status,
        ) ? (
          <button
            type="button"
            className="ghost integration-action"
            disabled={busy}
            onClick={refresh}
          >
            {busy ? "Refreshing..." : "Refresh"}
          </button>
        ) : null}
        {account.status === "disconnect_pending" ? (
          <>
            <button
              type="button"
              className="ghost integration-action"
              disabled={busy}
              onClick={disconnect}
            >
              Retry disconnect
            </button>
            <button
              type="button"
              className="ghost danger integration-action"
              disabled={busy}
              onClick={cleanup}
            >
              Remove local data
            </button>
          </>
        ) : (
          <button
            type="button"
            className="ghost danger integration-action"
            disabled={busy}
            onClick={disconnect}
          >
            Disconnect
          </button>
        )}
      </div>
    </div>
  );
}

function providerAuthLabel(provider: AgentProviderRegistration): string {
  if (provider.authMethods.length === 0) return "No native auth method";
  return provider.authMethods
    .map((method) => manifestAuthLabel(provider.providerId, method))
    .join(" or ");
}

function readableManifestValue(value: string): string {
  return value.split("_").join(" ");
}

function manifestAuthLabel(providerId: string, method: string): string {
  if (method === "api_key" && providerId === "claude_code") {
    return "Anthropic API key";
  }
  if (method === "api_key" && providerId === "gemini") {
    return "Gemini API key";
  }
  if (method === "api_key" && providerId === "grok") return "xAI API key";
  if (method === "api_key" && providerId === "cursor") return "Cursor API key";
  return readableManifestValue(method);
}

function accountAuthLabel(account: AgentAccount): string {
  if (account.authMethod === "api_key" && account.providerId === "claude_code") {
    return "Anthropic API key";
  }
  if (account.authMethod === "api_key" && account.providerId === "gemini") {
    return "Gemini API key";
  }
  if (account.providerId === "cursor") return "Cursor API key";
  if (account.providerId === "grok") return "xAI API key";
  return AUTH_LABELS[account.authMethod];
}

export function disconnectMessageFor(account: AgentAccount): string {
  if (usesAlfredManagedApiKey(account.authMethod, account.custodyMode)) {
    return `Alfred will delete its locally stored ${account.providerName} API key and local account metadata. This does not revoke or rotate the provider API key; revoke or rotate it in the provider console. If local deletion fails, the account will remain visible with a recovery state.`;
  }
  if (account.custodyMode === "runtime_managed") {
    return "Alfred will ask the isolated provider runtime to sign out, then remove local account metadata. Provider-side sessions may remain active when that runtime cannot revoke them; if cleanup fails, the account will remain visible with a recovery state.";
  }
  return "Alfred will ask the provider to revoke OAuth or device authorization when supported, remove the local account credential, and then delete local metadata. If any step fails, the account will remain visible with a recovery state.";
}

export function cleanupMessageFor(account: AgentAccount): string {
  if (usesAlfredManagedApiKey(account.authMethod, account.custodyMode)) {
    return `Alfred could not remove its local ${account.providerName} API key or metadata. Revoke or rotate the key in the provider console and remove any stale Alfred credential before deleting this local recovery record.`;
  }
  if (account.custodyMode === "runtime_managed") {
    return "The isolated provider runtime could not finish sign-out or local metadata cleanup. Complete provider-side sign-out when the runtime supports it before deleting this local recovery record.";
  }
  return "Alfred could not finish provider revocation or local credential removal. Revoke OAuth or device authorization at the provider when supported and remove the stale Alfred credential before deleting this local recovery record.";
}

function accountLabel(account: AgentAccount): string {
  return (
    account.displayName ??
    account.externalAccountId ??
    `${account.providerName} account`
  );
}

function safeDate(value: string): string {
  const timestamp = Date.parse(value);
  return Number.isFinite(timestamp)
    ? new Date(timestamp).toLocaleString()
    : "unknown";
}
