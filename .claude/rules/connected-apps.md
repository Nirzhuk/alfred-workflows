# Connected Apps and logo rules

## Provider connections

- Give every provider a stable `snake_case` ID in the Rust catalog, a narrow capability summary, explicit connection modes, and a recovery/revocation path.
- Keep provider calls and credential validation in Rust. Store credentials only in the OS credential store; SQLite, React state, workflow JSON, logs, descriptors, errors, and reports may contain only redacted metadata.
- Add scope handling, disconnect dependencies, reconnect behavior, and recovery cases to automated tests before marking a provider connectable.
- Preserve the accessible frontend fallback for unknown stored providers; do not drop persisted connections merely because their integration was removed or is newer than the app.

## Provider logos

- Every shipped catalog provider needs a matching `APP_LOGOS` entry and a local SVG in `src/assets/apps/`.
- Import each mark with `?no-inline`. Never fetch a logo at runtime, add a Logo.dev key, use a raster image, or include remote assets/scripts in the SVG.
- Optimize SVGs to 5 KiB raw or less. The app-logo test enforces this budget and catalog coverage.
- Transparent artwork is the default. Check the mark against both Alfred themes and recolor it when this preserves its identity and provides at least 3:1 non-text contrast.
- Set `requiresSurface: true` only when recoloring is not safe. Current exceptions are GitHub, Linear, and Notion; Sentry is intentionally recolored instead of receiving a white tile.
