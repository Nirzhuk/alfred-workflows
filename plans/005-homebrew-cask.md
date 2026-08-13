# Plan 005: Publish Alfred via Homebrew Cask

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan in
> `plans/README.md`.
>
> **Drift check (run first)**: Confirm `.github/workflows/release.yml` still
> builds both macOS `--bundles app,dmg` jobs and uploads them to a GitHub
> Release. If either architecture or DMG output was removed, STOP.

## Status

- **Priority**: P1 (may follow the direct DMG/EXE release)
- **Effort**: M
- **Risk**: LOW for a personal tap; MED for `homebrew/cask` review
- **Depends on**: plan 004 and a published, notarized GitHub Release containing
  both ARM64 and Intel DMGs
- **Category**: direction
- **Planned at**: unversioned snapshot 2026-08-11

## Why this matters

Homebrew gives macOS users a familiar install and upgrade path while consuming
the same immutable DMGs as direct downloads. The first release should use a
personal or organization tap unless Alfred already satisfies the official
Homebrew notability and repository-age requirements.

## Current state

- Release CI is configured for separate `aarch64-apple-darwin` and
  `x86_64-apple-darwin` DMGs, but no public release exists yet.
- A local ARM64 build produced `Alfred_0.1.0_aarch64.dmg`; the Intel asset
  filename must be taken from CI rather than guessed.
- There is no tap repository or cask file.
- README/install docs mention Homebrew as a future channel but cannot provide a
  real tap command or verified URL yet.
- Product ID is `com.nirzhuk.alfred`; product name is `Alfred`.
- The Rust data directory currently derives from
  `com.nirzhuk.workflows-local-agents`, so zap paths must retain the legacy
  location until identity migration is complete.

## Commands you will need

| Purpose | Command | Expected |
| --- | --- | --- |
| Check Homebrew | `brew --version` | version printed |
| Create starting cask | `brew create --cask <arm64-dmg-url>` | template created |
| Install local cask | `brew install --cask ./Casks/alfred.rb` | app installed |
| Uninstall local cask | `brew uninstall --cask alfred` | app removed |
| Audit new cask | `brew audit --new --cask alfred` | no errors |
| Style cask | `brew style --fix --cask alfred` | clean style |
| Final online check | `brew lgtm --online` | checks pass |

## Scope

**In scope**:

- Personal/organization tap by default, or an eligible `homebrew/cask` PR
- One architecture-aware cask supporting both published macOS DMGs
- SHA-256 verification, install/uninstall tests, and current Homebrew audits
- README install/upgrade commands and release-maintenance documentation
- A stable receipt-based signal that plan 006 can use to recognize the cask

**Out of scope**:

- Linux packages in Homebrew
- Winget/Chocolatey
- Implementing the in-app updater (plan 006)
- Bypassing Gatekeeper or distributing an unsigned/unnotarized DMG

## Steps

### Step 1: Choose cask hosting

Default to a personal/organization tap such as `<owner>/homebrew-alfred` or
`<owner>/homebrew-tap`, containing `Casks/alfred.rb`.

Use the official `Homebrew/homebrew-cask` repository only after confirming the
current package acceptance policy. At plan time, a self-submission normally
needs at least one of 90 forks, 90 watchers, or 225 stars; a repository less
than 30 days old is normally ineligible. Equivalent public evidence or a
documented exception may be considered but is not guaranteed.

**Verify**:

- Personal tap: the public tap repository exists and can be added with
  `brew tap <owner>/<tap>`.
- Official cask: record evidence that current acceptance requirements are met
  before doing cask work in the official repository.

### Step 2: Publish and inspect both DMGs

Publish a non-draft GitHub Release containing notarized ARM64 and Intel DMGs.
Download both assets and record their exact immutable URLs, filenames, sizes,
and SHA-256 digests.

**Verify**:

```bash
gh release view v0.1.0 --json assets,isDraft,url
shasum -a 256 path/to/Alfred-arm64.dmg
shasum -a 256 path/to/Alfred-intel.dmg
xcrun stapler validate path/to/Alfred-arm64.dmg
xcrun stapler validate path/to/Alfred-intel.dmg
```

Both architectures are required while README advertises Intel support. If only
one DMG is available, STOP or explicitly narrow the supported platforms first.

### Step 3: Author one architecture-aware cask

Start from `brew create --cask`, then adapt it to the actual asset names. When
the only filename difference is the architecture suffix, prefer Homebrew's
`arch` substitution and per-architecture checksums:

```ruby
cask "alfred" do
  arch arm: "aarch64", intel: "x64"

  version "0.1.0"
  sha256 arm:   "<arm64-dmg-sha256>",
         intel: "<intel-dmg-sha256>"

  url "https://github.com/<owner>/<repo>/releases/download/v#{version}/Alfred_#{version}_#{arch}.dmg"
  name "Alfred"
  desc "Local multi-agent workflow automations"
  homepage "https://github.com/<owner>/<repo>"

  depends_on macos: ">= :big_sur"

  app "Alfred.app"

  zap trash: [
    "~/Library/Application Support/com.nirzhuk.workflows-local-agents",
    "~/Library/Application Support/com.nirzhuk.alfred",
    "~/Library/Caches/com.nirzhuk.alfred",
    "~/Library/Preferences/com.nirzhuk.alfred.plist",
  ]
end
```

The `aarch64`/`x64` values are examples based on expected Tauri naming. Replace
them with the exact CI asset suffixes. If URLs differ in more than the suffix,
use `on_arm`/`on_intel` blocks instead. Do not submit an ARM-only cask while
advertising Intel support.

Do not add `auto_updates true`: Homebrew installs must be upgraded by Homebrew,
and plan 006 will suppress self-updating for that channel.

### Step 4: Test both architecture branches

From the tap checkout:

```bash
brew install --cask ./Casks/alfred.rb
brew list --cask --versions alfred
brew uninstall --cask alfred
brew audit --new --cask alfred
brew style --fix --cask alfred
```

Test the ARM branch on Apple Silicon and the Intel branch on Intel hardware or
a suitable Intel CI runner. On each architecture:

1. Confirm Homebrew downloads the expected DMG and checksum.
2. Confirm `Alfred.app` lands in the configured Applications directory.
3. Launch the app and pass Gatekeeper.
4. Confirm `brew upgrade --cask alfred` works with a second test version.
5. Confirm uninstall removes the app and `--zap` targets only documented user
   data.

For an official submission, also run `brew lgtm --online` from the contribution
checkout and complete the current pull-request template.

### Step 5: Document install and release maintenance

For a personal tap, document:

```bash
brew tap <owner>/<tap>
brew install --cask alfred
brew upgrade --cask alfred
```

For the official cask, omit the tap command. Update `docs/releasing.md` with a
post-publish task to update the cask version, both SHA-256 values, and both
architecture URLs.

### Step 6: Define the updater's Homebrew signal

Record the stable cask token `alfred` and test how installed receipts are
reported on Intel and Apple Silicon:

```bash
/opt/homebrew/bin/brew list --cask --versions alfred
/usr/local/bin/brew list --cask --versions alfred
```

Only one path normally exists. Plan 006 should query an available known Homebrew
binary and verify an installed `alfred` receipt. It must not infer Homebrew
ownership solely from the running executable path, because the `app` stanza
places the executable under `/Applications`.

## Done criteria

- [ ] A notarized public ARM64 DMG and Intel DMG exist
- [ ] One cask selects the correct URL and SHA-256 on both architectures
- [ ] ARM64 and Intel install, launch, upgrade, and uninstall tests pass
- [ ] Current Homebrew audit/style checks pass
- [ ] README documents the correct install and upgrade commands
- [ ] Release docs include both-architecture cask bump steps
- [ ] Receipt behavior needed by plan 006 is recorded and tested
- [ ] `plans/README.md` status row for 005 is `DONE`

## STOP conditions

- No public notarized DMG URLs are available
- Either advertised macOS architecture lacks a working DMG
- The actual `.app` bundle name cannot be determined from the artifacts
- The operator wants direct DMG only and rejects Homebrew as a channel
- The official cask route is chosen but current acceptance requirements are not met
- The cask requires bypassing Gatekeeper or certificate verification

## Maintenance notes

- Every release: update version, both SHA-256 values, and both URLs.
- Test the newest supported macOS version and both architectures the cask declares.
- Keep legacy and current data paths in `zap` until an identity/data migration is complete.
- If official cask eligibility changes, the personal tap remains a valid fallback.
