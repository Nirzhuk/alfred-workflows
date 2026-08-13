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
| `../../src-tauri/Assets.car` | Experimental Liquid Glass catalog; not bundled while its layered foreground renders incorrectly in Finder |
| `../../public/icon.png` | Web / favicon |

**Do not** bake rounded corners into the master. macOS/iOS apply the real squircle mask; pre-rounded transparent icons skip that mask and look larger/squarer in the Dock.

## Regenerate flat icons

```bash
bunx tauri icon ./src-tauri/app-icon.png
```

The checked-in `src-tauri/icons/icon.icns` is the production macOS bundle icon,
generated from the full-bleed master with
`scripts/make-macos-squircle-icon.swift` and
`scripts/package-macos-icns.swift`. Finder uses this ICNS directly. Do **not**
add `CFBundleIconName=AppIcon` or bundle the experimental `Assets.car` until
the compiled foreground has been verified at every Finder icon size; the
current catalog renders as a blank teal icon.

## Regenerate experimental Liquid Glass (`Assets.car`)

1. Open `AppIcon.icon` in **Icon Composer** and tweak if needed.
2. Compile:

```bash
ACTOOL="/Applications/Xcode.app/Contents/Developer/usr/bin/actool"
"$ACTOOL" assets/branding/AppIcon.icon --compile /tmp/alfred-iconcar \
  --platform macosx --minimum-deployment-target 15.0 \
  --app-icon AppIcon --include-all-app-icons \
  --output-partial-info-plist /tmp/alfred-iconcar/partial.plist
cp /tmp/alfred-iconcar/Assets.car src-tauri/Assets.car
```

Before enabling it in the bundle, build a DMG and confirm macOS resolves the
mustache foreground from the packaged `.app`, not only from the source ICNS.
