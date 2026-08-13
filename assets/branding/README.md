# Branding / app icon

Source art: `Frame 30.jpg` → renamed and prepared as **icon**.

| Path | Role |
|------|------|
| `icon.png` / `icon-1024.png` | Full-bleed **opaque** masters (no baked corners) |
| `layers/` | Background + Foreground for Icon Composer |
| `AppIcon.icon/` | Icon Composer project (Liquid Glass) |
| `../../src-tauri/app-icon.png` | Input for `bunx tauri icon` |
| `../../src-tauri/icons/*` | Flat platform icons (icns/ico/png) |
| `AppIcon-liquid.icns` | Squircle/Liquid Glass ICNS source for the macOS bundle |
| `../../src-tauri/Assets.car` | Compiled Liquid Glass catalog (macOS 26+) |
| `../../public/icon.png` | Web / favicon |

**Do not** bake rounded corners into the master. macOS/iOS apply the real squircle mask; pre-rounded transparent icons skip that mask and look larger/squarer in the Dock.

## Regenerate flat icons

```bash
bunx tauri icon ./src-tauri/app-icon.png
```

The checked-in `src-tauri/icons/icon.icns` is the squircle/Liquid Glass export, generated from the full-bleed master with `scripts/make-macos-squircle-icon.swift` and `scripts/package-macos-icns.swift`. Do **not** run that after fine-tuning Liquid Glass, because it overwrites the bundle icon with a flat icns.

## Regenerate Liquid Glass (`Assets.car`)

1. Open `AppIcon.icon` in **Icon Composer** and tweak if needed.
2. Compile:

```bash
ACTOOL="/Applications/Xcode.app/Contents/Developer/usr/bin/actool"
"$ACTOOL" assets/branding/AppIcon.icon --compile /tmp/agentflow-iconcar \
  --platform macosx --minimum-deployment-target 15.0 \
  --app-icon AppIcon --include-all-app-icons \
  --output-partial-info-plist /tmp/agentflow-iconcar/partial.plist
cp /tmp/agentflow-iconcar/Assets.car src-tauri/Assets.car
```

`src-tauri/Info.plist` sets `CFBundleIconName=AppIcon`; `tauri.conf.json` bundles `Assets.car`.
