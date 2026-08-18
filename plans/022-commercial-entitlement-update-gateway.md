# Rejected: custom commercial control plane

The Bun/Stripe entitlement and update gateway was rejected on 2026-08-15 in
favor of the canonical [Polar paid-release roadmap](release-money/README.md).

- Polar hosts checkout, taxes, receipts, customer email authentication,
  subscriptions, license keys, Company seats, portal, and authorized files.
- [Plan 001](release-money/archive/001-connect-desktop-polar-licensing.md)
  (DONE, archived) makes Alfred call Polar's public license
  activate/validate/deactivate endpoints with no provider credential;
  [Plan 003](release-money/003-configure-polar-commerce.md)
  later binds the approved public sandbox IDs.
- v0.5.0 updates are manual through Polar; an authenticated update gateway is
  not on the release path.

The existing custom Stripe implementation is abandoned reference work. Do not
deploy, extend, or integrate it. Deleting it is a separate explicit cleanup
task.
