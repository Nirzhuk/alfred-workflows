# Open-source and distribution policy

Alfred uses an **open-source, paid-distribution** model.

## What is free

The complete source code in this repository is licensed under the
[GNU General Public License, version 3 or later](../LICENSE). You may use,
inspect, modify, compile, and redistribute it under that license. Alfred does
not require a purchase to compile or run a build you make from this repository.

The GPL permits commercial redistribution. Someone may charge for an
unofficial copy or fork, but they must preserve the recipients' GPL rights and
provide the corresponding source. They may not present that copy as an
official, maintainer-signed Alfred release.

The AI agent tools that Alfred launches are separate products. Their
providers may require their own subscriptions, accounts, or usage payments.

## What the official products pay for

The maintainers sell a Desktop License per named user and a Company subscription
per active member; every Company seat includes Desktop. These products pay for
the release and hosted services around the source code:

- maintainer-built and smoke-tested installers;
- Apple notarization and platform code signing where available;
- release checksums and, when shipped, an official update channel;
- the convenience of installing without a local Rust/Tauri toolchain; and
- continued maintenance and release engineering.

Company/Enterprise seats may additionally fund organization workspaces,
managed sharing/collaboration, hosted relays, administrative controls, and
contracted support as those features ship. Prices and billing intervals come
from the server-side Stripe catalog, not the desktop binary.

Buying an official build does not reduce the rights granted by the GPL,
including the right to redistribute it. Building from source does not include
official binary support, signing, notarization, or an assurance that a local
modification will interoperate with future official updates.

## Unofficial builds

A self-built or third-party build must not claim to be an official release.
Personal builds of an unmodified checkout may keep the default name and icon.
If you distribute a modified build, make the changes clear and follow
[BRANDING.md](../BRANDING.md), including renaming it when the presentation
could otherwise confuse users about its origin.

Never copy signing certificates, private updater keys, storefront credentials,
or other maintainer secrets into a source build. They are not required to
compile Alfred.

## Contributions

Contributions are accepted under GPL-3.0-or-later. See
[CONTRIBUTING.md](../CONTRIBUTING.md) for the development workflow and
[SECURITY.md](../SECURITY.md) for private vulnerability reporting.

## Policy changes

The license on an existing version cannot be retroactively withdrawn. A future
version may change its distribution or support terms, but previously published
GPL versions retain their existing rights.
