# Connected-app event framework

Alfred can run a workflow from a connected app while the desktop process is
open. Provider connectors register declarative event descriptors and Rust
adapters; they do not add provider-specific trigger tables, runner branches,
or executable frontend code.

## Event contract

Every provider event is reduced to the versioned `NormalizedAppEvent` shape
before SQLite or `run://event` sees it:

- provider, event type, connection, external event ID, and occurrence time;
- bounded subject, actor, HTTPS resource URL, and 1,000-character preview;
- a descriptor allow-listed map of at most 16 scalar attributes.

The complete serialized event is limited to 16 KiB. Provider bodies, request
headers, webhook signatures, tokens, attachments, thread history, and full
mail/message/document bodies are forbidden. The runner labels the payload as
untrusted external data and tells the agent not to interpret it as workflow
instructions or authorization.

## Registering a provider event

1. Add an `AppEventDescriptor` with a stable namespaced `event_type`, exact
   scopes, delivery modes, filter fields, attribute allow-list, descriptor
   version, poll interval, and pending cap.
2. Implement `AppEventAdapter`. Provider HTTP/socket code stays in Rust and
   obtains credentials only through `TokenAccessCapability`.
3. Register the descriptor and adapter in `IntegrationsState` construction.
   Duplicate IDs and secret filter fields are rejected.
4. Normalize identifiers and short previews in the adapter. Let the framework
   validate size, timestamp, URL, allow-list, and credential leakage again.
5. Map provider failures to stable `AppEventErrorCode` values. Never preserve a
   raw provider response, subject, preview, email address, or token in logs.
6. Add sanitized adapter fixtures for dedupe, pagination/checkpoints, retry,
   renewal, cancellation, payload minimization, and revocation.

## Delivery and recovery

Receipts and queue rows are inserted in one SQLite transaction. Polling cursors
advance only after the whole accepted batch is durable. Promoting a queued
event creates its pending run and marks its receipt `enqueued` in another
transaction, so restart recovery cannot create a second run.

Each workflow retains its existing single-active-run slot. Accepted events wait
in FIFO order. Replayable polling stops at the pending cap without advancing
the cursor; non-replayable socket/push delivery records the newest event as
`dropped_overrun` and increments visible health. Failed or interrupted runs are
terminal and are not silently re-enqueued.

The runtime bounds provider concurrency, persists jitter-ready exponential
backoff and `Retry-After` deadlines, renews expiring subscriptions, and cancels
in-flight adapters when trigger configuration reloads. It makes no offline
delivery promise: local polling and sockets stop when Alfred exits.

## Security checklist

- Keep provider API and normalization code in Rust.
- Never put a credential or secret filter kind in a descriptor or React state.
- Pass a connection ID—not a token—to selector commands.
- Cache only bounded resource IDs/labels, never provider content.
- Use exact scopes and verify connection/provider compatibility in Rust.
- Keep metrics to counts, timings, stable error codes, and correlation IDs.
- Test SQLite, run payloads, emitted events, and logs against credential and raw
  body fixtures before enabling a real provider.
