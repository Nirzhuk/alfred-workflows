# Plan 011: Specify and build the optional cloud relay

> **Executor instructions**: Treat the architecture gate in Step 1 as
> mandatory. This plan changes Agentflow's trust model and operating cost. Do
> not deploy, collect user data, or introduce an account system until the ADR is
> approved. Update `plans/README.md` only after all approved phases are done.
>
> **Drift check (run first)**: re-read `specs.md` sections describing local-only
> desktop execution and trigger lifecycle. Confirm Plans 008 and 010 interfaces
> if present. At planning time there was no website, user account, backend, or
> public callback service, and triggers ran only while the app was open.

## Status

- **Priority**: P1 (P0 prerequisite for Copilot/public webhook delivery)
- **Effort**: XL
- **Risk**: CRITICAL
- **Depends on**: Plan 008; Plan 010 before provider event delivery
- **Category**: product direction / cloud architecture
- **Planned at**: unversioned snapshot, 2026-08-11

## Why this matters

Several “real” integrations cannot reach a loopback-only desktop app:

- Slack's branded bot installation and HTTP Events API require server-side bot
  OAuth/webhook infrastructure. Slack also offers desktop PKCE for user tokens,
  but desktop redirects cannot request bot scopes.
- Microsoft Graph webhooks require a public validation/delivery endpoint.
- Gmail push uses Google Cloud Pub/Sub rather than a desktop callback.
- Microsoft Copilot needs a remotely reachable authenticated API.
- Events that arrive while Agentflow is closed need a durable queue.

This is optional infrastructure. Local polling, native PKCE, and user-owned
Slack credentials can ship without it, but Copilot cannot.

## Recommended v1 boundary

Build a **thin relay; keep workflow execution local**:

- The relay handles identity/device pairing, confidential provider callbacks/
  webhook ingress,
  signature verification, short-lived queues, and Copilot commands.
- The desktop maintains an outbound authenticated WebSocket or long poll; no
  inbound port is exposed.
- Provider access/refresh tokens remain in the OS credential store whenever
  the provider permits native PKCE. For provider flows requiring a confidential
  client, the relay exchanges the code and transfers the grant once in an
  encrypted, short-lived envelope to the paired desktop.
- The relay does not run agents, read local workflow definitions, or receive
  full run logs. It stores only allow-listed routing metadata and encrypted,
  TTL-bound command/event envelopes.
- Copilot sees workflow IDs explicitly published by the user, plus coarse run
  status; it does not receive arbitrary local workflows or CLI output.

Cloud token custody is a separate future decision. Do not add it opportunistically.

## Commands you will need

The relay stack is deliberately selected by the Step 1 ADR; record its exact
install, lint, test, migration, contract-test, and local-run commands in this
section before Step 2. Desktop gates remain:

- `bun test`
- `bun run build:frontend`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`

## Scope

**In scope**:

- Approved architecture/threat-model ADR and protocol/OpenAPI contracts.
- Authenticated user/tenant identity and explicit device pairing.
- Device key registration, outbound desktop channel, command/event queue,
  acknowledgement, idempotency, expiry, and revocation.
- OAuth callback exchange for confidential-client providers, provider webhook
  ingress, and remote workflow invocation endpoints.
- Data minimization, encryption, audit metadata, deletion, abuse/rate limits,
  deployment/runbook, and integration test environment.

**Out of scope**:

- Moving workflow or agent execution into the cloud.
- A general website/PWA, workflow sync, collaboration, billing, or analytics.
- Long-term raw event storage, email/message/document bodies, or run logs.
- Exposing the desktop loopback webhook through a tunnel.
- Embedding provider client secrets in the Tauri binary.

## Git workflow

Do the ADR/protocol work in a reviewable change before service implementation.
After approval, use separate changes for relay identity/queue, desktop pairing,
provider ingress, and remote invocation. Do not deploy, commit, or push unless
the user authorizes that step; preserve the local-only app path throughout.

## Implementation steps

### Step 1: Produce and approve the architecture gate

Add an ADR covering these explicit choices:

1. hosting/runtime, region strategy, managed database/queue, and KMS;
2. identity provider and whether Microsoft Entra is required for Copilot users;
3. local vs cloud provider-token custody (recommend local-first);
4. event/command retention (recommend hours, not days, with hard TTL);
5. whether encrypted queued events may contain message/mail previews;
6. tenancy keys: user, organization/tenant, device, provider installation;
7. privacy policy/support/security ownership and monthly cost ceiling;
8. availability behavior: queue until desktop reconnects, then expire visibly;
9. protocol versioning and backward-compatible desktop update policy.

Create a data-flow diagram and threat model for OAuth code interception,
device-pair hijack, webhook forgery, replay, tenant confusion, queue scraping,
stolen refresh tokens, prompt injection, and account/device revocation.

**Verify**: named product and security owners approve the ADR. If they do not,
stop here; local provider plans may proceed without remote phases.

### Step 2: Specify the desktop-relay protocol before code

Define versioned messages for:

- device pairing/challenge and public-key registration;
- connect/heartbeat/resume cursor;
- queued provider event;
- `start_run`, `get_run_status`, and `cancel_run` commands;
- accepted/started/completed/failed/expired acknowledgements;
- installation/subscription revoke and desktop unlink.

Every command has tenant/user/device IDs, an idempotency key, issued/expiry
timestamps, schema version, and signature/auth context. Encrypt content to the
device key when the relay does not need to inspect it. Specify queue ordering,
redelivery, duplicate handling, maximum size, and status polling semantics.

Add an OpenAPI contract for remote endpoints and JSON-schema fixtures shared
with desktop tests. No endpoint returns provider tokens or full workflow output.

**Verify**: contract tests reject wrong tenant/device, expired/replayed IDs,
unknown schema versions, oversized envelopes, and invalid state transitions.

### Step 3: Build identity and explicit device pairing

Implement browser sign-in using the approved OIDC provider. Pair from the
desktop with a short-lived, single-use challenge shown in both surfaces. Bind a
device-generated public key, human-readable device name, app version, user,
and tenant. Store only public key/fingerprint server-side.

Desktop refresh credentials belong in the OS keychain through Plan 008. Provide
Settings UI to list the current relay account/device, unlink it, and revoke all
remote sessions. Tenant switching must require explicit re-pairing.

**Verify**: tests cover challenge expiry/replay, wrong-user confirmation,
device revocation, tenant mismatch, and a lost-device server-side revoke.

### Step 4: Implement the durable queue and outbound desktop channel

Use an authenticated WebSocket with a long-poll fallback if the approved host
cannot guarantee desktop WebSockets. The desktop reconnects with cursor and
jittered backoff. Relay writes are idempotent; delivery is at-least-once;
desktop dedupe from Plan 010 provides exactly-once workflow enqueueing.

Commands/events expire to a visible terminal state. Never silently execute a
stale Copilot command days later. Apply per-user/device/provider rate and
concurrency limits. Do not log envelope bodies.

**Verify**: integration tests simulate disconnect/reconnect, relay restart,
duplicate delivery, out-of-order ack, expired command, revoked device, and two
devices on one account.

### Step 5: Add provider callback/webhook ingress

Implement provider routes behind adapters. Each must verify signature/state,
apply body/time limits, extract routing keys, dedupe provider delivery IDs, and
enqueue an allow-listed or device-encrypted event. OAuth callback state binds
provider + user + device + one connection attempt and expires quickly.

For confidential-client exchanges, keep client secrets in KMS/secret manager.
Transfer grants to the desktop only through a one-time encrypted envelope and
delete transient server copies after acknowledgement. Add provider disconnect
and deletion hooks.

**Verify**: negative fixtures cover forged signatures, old timestamps, reused
OAuth state, wrong installation/user, malformed content types, and large bodies.

### Step 6: Add remote workflow publication and invocation

Users explicitly mark workflows “Available to remote assistants.” Publish only
an opaque workflow ID, display name, safe description, allowed input schema,
and enabled state. The relay does not receive graph JSON. A remote start queues
the validated input to a paired online device and returns `202` + run request
ID; status is a coarse state machine with a sanitized failure code.

Require confirmation/policy for destructive workflows and cap remote
concurrency. V1 does not return agent output; a later opt-in result schema can
be separately reviewed.

**Verify**: unpublished workflow IDs return indistinguishable not-found,
invalid inputs never reach a device, duplicate idempotency keys return the same
request, and offline/expired behavior is explicit.

### Step 7: Operationalize before production

Add metrics that contain counts/latency/error codes only, alerting, KMS/key
rotation, backups, retention jobs, data export/deletion, provider secret
rotation, incident response, abuse controls, staging tenants, and a rollback
runbook. Pin callback domains and document DNS/certificate ownership.

## Test plan

- Relay unit/contract/integration suites from the selected stack.
- Desktop `bun test`, `bun run build:frontend`, and Rust tests.
- End-to-end staging: pair → disconnect → reconnect → event → one local run.
- OAuth state/signature replay and cross-tenant penetration tests.
- Search logs/database/queue snapshots for fixture provider tokens/raw bodies.
- Verify deletion and device revocation within documented time bounds.

## Done criteria

- [ ] ADR and threat model are approved before deployment.
- [ ] Desktop accepts only authenticated, unexpired, deduplicated envelopes.
- [ ] Workflow execution and graph data remain local.
- [ ] No confidential client secret ships in the desktop app.
- [ ] Remote workflows require explicit publication and bounded inputs.
- [ ] Retention, deletion, revocation, monitoring, and incident runbooks exist.

## STOP conditions

- Hosting, identity, tenant ownership, privacy policy, or on-call ownership is
  unassigned.
- The design requires raw long-term email/message/run-log storage.
- Provider grants cannot be transferred without exposing them to URLs/logs.
- Product expects remote runs with no user identity or workflow publication.
- A production deployment is requested before replay/cross-tenant tests pass.

## Maintenance notes

- This relay creates a security-sensitive online service and recurring cost.
- Rotate protocol versions additively; support a bounded desktop-version window.
- Revisit cloud token custody only with a separate ADR and migration plan.
