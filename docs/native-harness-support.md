# Native harness support and recovery

This runbook is for failures involving an agent node's harness selection. The
Provider CLI harness remains supported; an unavailable Alfred-native provider
does not disable CLI workflows or another provider.

## Identify the execution path

Check the node or run history for both fields:

- `provider`: the provider selected by the workflow;
- `harness`: `cli` or `alfred`.

Never infer the harness from the provider name or from an installed CLI. A
native failure is terminal for that attempt. There is no silent native-to-CLI
retry and no automatic credential migration.

## Safe diagnostics

Alfred's versioned harness diagnostic reports only:

- provider, harness, manifest status, and stable block-reason code;
- declared runtime version and `external_cli`, `not_registered`, or
  `registered_idle` runtime state;
- last runtime exit state when one is safely known;
- account auth method, account status, a shortened opaque account label, and a
  stable error code;
- platform and development/packaged build kind.

It never includes tokens, cookies, authorization URLs, credential references,
email addresses, workspace identifiers, prompts, outputs, provider payloads,
stderr, or private filesystem paths. Ask users to copy the bounded diagnostic,
not screenshots of provider login callbacks or raw log files.

## Native account recovery

No provider can establish, refresh, or reconnect a native account in the
current zero-native release. Those actions remain blocked by the same manifest
that blocks execution.

1. Open **Settings → Native agent accounts** and find the exact provider.
2. Use **Disconnect** to request provider/runtime revocation and remove the
   local credential and metadata.
3. If cleanup remains `disconnect_pending`, revoke or rotate the credential in
   the provider's own console. Only then use **Remove local data** to discard
   the recovery record.

For API-key providers, local disconnect cannot revoke or rotate the provider's
key. For runtime-managed providers, provider-side sessions may remain when the
isolated runtime cannot complete logout. These cases must remain visible until
the user chooses local cleanup.

## Runtime/package recovery

Native runtime artifacts are independent of workflow data. A runtime lookup
must verify its versioned resource path, SHA-256, required licence/notice files,
signing status, and rollback metadata before it is executable. Missing or
mismatched artifacts remain blocked.

Do not replace a failed runtime in place or delete workflow data. Restore a
previous signed, manifest-listed runtime only through an explicit release
rollback. Automatic fallback—including to a CLI—is prohibited. Cancelled or
crashed runtimes must release child processes and temporary account-scoped
state before another start is attempted.

## Explicit CLI fallback

If the user wants to continue with the CLI, have them verify the provider CLI
works in a terminal, edit the node, and select **Provider CLI**. Call out that
the CLI may use a different account, billing plan, model catalogue, session,
and permission policy. Preserve the original failed run history as native.

## Escalation checklist

Include the safe diagnostic, Alfred version, platform, build kind, provider,
and whether the failure occurred during connect, model discovery, start,
approval, cancellation, or disconnect. Do not attach raw database files,
credential-store exports, full provider responses, prompts, outputs, or home
directory paths.
