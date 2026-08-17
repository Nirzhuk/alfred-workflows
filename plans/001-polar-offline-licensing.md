# Moved: direct Polar desktop licensing

This legacy plan was reconciled on 2026-08-15. Polar is now the approved
commerce, license-key, seat, portal, and download provider.

Use the canonical [paid-release roadmap](release-money/README.md):

- [Plan 001](release-money/archive/001-connect-desktop-polar-licensing.md)
  (DONE, archived) implements direct public activate/validate/deactivate,
  secure key storage, and the 7-day/30-day offline policy using local mocks
  and injected configuration.
- [Plan 002](release-money/archive/002-build-polar-license-settings.md) (DONE,
  archived) builds the customer-facing License & Billing experience with
  fixture links.
- [Plan 003](release-money/003-configure-polar-commerce.md) configures Polar's
  products, benefits, seats, checkout, portal, and downloads in sandbox, then
  binds the public IDs/URLs to the completed client.

No Alfred commerce/license backend or Polar access token is part of the
desktop integration. Public GPL source and local workflows remain usable
without commercial validation.
