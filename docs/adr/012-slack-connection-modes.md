# ADR 012: Slack connection modes and default UX

- **Status:** Accepted for private bot actions and local Socket Mode;
  native user-action decision pending
- **Date:** 2026-08-13
- **Related plans:** 008, 009, 010, 011, 012

## Context

Slack offers three materially different ways to connect Alfred. They cannot be
presented as interchangeable:

1. A private/BYO Slack app can provide bot actions locally and later provide
   Socket Mode events while Alfred is open.
2. Native desktop PKCE can authorize public-client user scopes without an
   embedded client secret, but messages act as the signed-in user and bot scopes
   are unavailable to desktop redirects.
3. A public Alfred bot needs a confidential server-side installation flow and
   verified event ingress through the optional cloud relay.

Official references: [Slack desktop PKCE](https://docs.slack.dev/authentication/using-pkce/),
[OAuth v2](https://api.slack.com/authentication/oauth-v2), and
[Socket Mode](https://api.slack.com/apis/connections/socket).

## Decision

Ship private/BYO bot **actions** as the explicit advanced local beta. Do not
present it as the public Alfred bot. Do not enable Incoming Webhooks until their
workspace identity can be verified.

Keep native PKCE disabled until Product explicitly accepts “Send as you” as a
first-class Slack mode and owns the reconnect experience for expiring refresh
tokens. If approved, implement it through Plan 008's loopback/state/PKCE and
refresh boundaries with user scopes only; never request or claim bot/event
capabilities.

Keep the public bot and HTTP Events API blocked until ADR 011 has named product,
security, privacy/support, operations, budget, Entra, and CI/DNS approvals and
the relay passes its production gates.

Local `app_mention` is approved and implemented as a separate phase. It uses one
Socket Mode connection per installation, acknowledges envelopes before payload
processing, fans events to every matching enabled trigger, and preserves Plan
010's receipt-before-run and bounded-overrun behavior. It does not poll Slack
history or create competing WebSockets per trigger.

## Consequences

- The current Connect button opens only an honestly labeled private-app form.
- Users must create and administer their own Slack app for the local beta.
- Actions send as that app's bot and require only `chat:write` plus the minimum
  conversation selector scope.
- Mention events, one-click installation, offline delivery, and Marketplace
  distribution are not claimed by this phase.

## Approval record

| Decision | Owner | Result | Date |
| --- | --- | --- | --- |
| Native PKCE “Send as you” is an acceptable product mode | **TBD** | Pending | — |
| Socket Mode local event UX and support boundary | Product user | Approved | 2026-08-13 |
| Public bot/relay | See ADR 011 | Blocked | — |
