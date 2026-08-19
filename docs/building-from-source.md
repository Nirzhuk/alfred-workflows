# Build Alfred from source

You can compile and run Alfred from this repository without buying the
official binary. Source builds use the same checked-in application code but are
unsigned unless you configure your own platform signing identity.

## 1. Install prerequisites

All platforms need:

- [Bun](https://bun.sh/) 1.3.8 (the version recorded in `package.json`);
- the stable Rust toolchain installed through
  [rustup](https://rustup.rs/); and
- Git.

Tauri also needs platform development tools. The canonical, current list is in
the [Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/).

### macOS

Install Xcode Command Line Tools:

```bash
xcode-select --install
```

### Windows

Install Microsoft C++ Build Tools with **Desktop development with C++** and
Microsoft Edge WebView2. Use Rust's stable MSVC toolchain. MSI packaging also
requires the Windows VBSCRIPT optional feature; NSIS does not.

### Debian or Ubuntu Linux

Install Tauri's desktop libraries:

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

For another distribution, use the matching package list in Tauri's
prerequisites guide.

## 2. Get the source and dependencies

```bash
git clone <alfred-repository-url>
cd alfred
bun install --frozen-lockfile
```

Replace `<alfred-repository-url>` with this repository's public clone URL.
The final public repository URL has not been configured yet.

No Alfred API key, purchase key, signing certificate, or maintainer secret
is needed. Install and authenticate at least one supported agent CLI separately
if you want to run a real workflow.

## 3. Run a development build

```bash
bun run dev
```

This starts the Tauri desktop application and its Vite frontend. Alfred is
not a standalone website; do not use the Vite server as a deployed app.

## 4. Run the checks

```bash
bun run check
```

That command runs the frontend tests, TypeScript/Vite production build, and
Rust tests. You can run the parts independently:

```bash
bun run test:frontend
bun run build:frontend
bun run test:rust
```

## 5. Build an installer

```bash
bun run build
```

Tauri creates bundles for the current host OS under
`src-tauri/target/release/bundle/`. Build each operating system's installers on
that operating system unless you have deliberately configured and tested a
cross-compilation toolchain.

Local packages normally will not carry the maintainers' Apple notarization,
Windows Authenticode signature, or updater signature. Operating systems may
therefore warn before launching them. Do not use or request the project's
private signing material.

## Troubleshooting

| Problem | Check |
| --- | --- |
| Tauri cannot find a native library | Revisit the platform packages in the Tauri prerequisites guide |
| An agent CLI is not found | Confirm its command works in a normal terminal, then fully restart Alfred |
| The app builds but workflows cannot authenticate | Sign in using that agent CLI's own login flow |
| macOS or Windows warns about the app | Expected for an unsigned self-build; sign it with your own identity if you distribute it |
| A clean checkout behaves differently | Use `bun install --frozen-lockfile` and the recorded Bun version |

For project conventions and pull-request expectations, read
[CONTRIBUTING.md](../CONTRIBUTING.md).
