# Desktop only

Alfred is a **desktop** Tauri app for **macOS, Linux, and Windows** only.

## Do not

- Add Android or iOS targets (`tauri android`, `tauri ios`, mobile entry points, `gen/android`, `gen/apple`)
- Treat the Vite frontend as a deployable website (`preview`, static hosting, PWA)
- Change `dev` / `build` away from the desktop Tauri wrapper

## Do

- Use `bun run dev` → Tauri desktop app
- Use `bun run build` → desktop bundles (`app`/`dmg`, `deb`/`rpm`/`appimage`, `nsis`/`msi`)
- Keep Vite as `dev:frontend` / `build:frontend` for Tauri's before-commands only
