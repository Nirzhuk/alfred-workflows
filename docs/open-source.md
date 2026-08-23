# Open-source and distribution policy

Alfred uses an **open-source, paid-distribution** model.

## What is free

The complete source code in this repository is licensed under the
[GNU General Public License, version 3 or later](../LICENSE). You may use,
inspect, modify, compile, and redistribute it under that license. Alfred does
not require a purchase to compile or run a build you make from this repository.

**A build you compile yourself is free, fully featured, and stays that way
forever.** It is not a trial, not a reduced edition, and not time-limited. No
feature is withheld from a source build, and none ever will be — under the GPL
a source build is unlocked by design.

The GPL permits commercial redistribution. Someone may charge for an
unofficial copy or fork, but they must preserve the recipients' GPL rights and
provide the corresponding source. They may not present that copy as an
official, maintainer-signed Alfred release.

The AI agent tools that Alfred launches are separate products. Their
providers may require their own subscriptions, accounts, or usage payments.

## What the official products pay for

The maintainers sell **two products, both one-time purchases**:

| Product | Sold as |
| --- | --- |
| **Alfred License** | one payment, one named user, not seat-based |
| **Alfred Teams** | one payment **per claimed seat**, bought on the Alfred website |

Every claimed Teams seat receives its own license key and its own downloads.
There is no subscription, no annual renewal, and no recurring charge.

Payment buys the release and hosted services around the source code — **not
capability**:

- maintainer-built and smoke-tested installers;
- Apple Developer ID signing, notarization, and stapling for both macOS builds;
- published SHA-256 checksums beside every official download;
- the convenience of installing without a local Rust/Tauri toolchain;
- **one year of updates** from the date of purchase; and
- continued maintenance, support, and release engineering.

Teams seats may additionally fund organization workspaces, managed
sharing/collaboration, hosted relays, administrative controls, and contracted
support as those features ship.

### One year of updates, permanent features

- Paying once unlocks every paid feature **permanently**. Nothing you paid for
  is ever taken away.
- The purchase covers every Alfred build released within **one year** of buying.
- After that year, the build you own keeps every feature **forever**. It does
  not degrade, expire, phone home, or stop working.
- Builds released after your year still install and run. All of your workflows,
  memories, schedules, triggers, and files stay intact and usable in them; only
  their paid features stay locked until you buy another year.
- **What lapsing does not do**: it does not disable the build you have, does
  not remove a feature you already paid for, and does not touch your data.
- A refunded, revoked, or disabled license is a different thing, and it does
  end access.
- Buying another year is a fresh purchase of the same product. There is no
  separate renewal SKU and no price you are locked into.

Because Alfred is GPL, a paid feature switch is exactly that — a switch, and
what you are buying is the convenience of not flipping it yourself. Compiling
from source gives you everything, by design and by licence.

### Polar is the merchant of record

[Polar](https://polar.sh) sells, bills, and delivers the official builds. Polar
owns checkout, payment collection, tax and VAT handling, receipts, customer
email authentication, billing self-service, seat invitations, license-key
issuance, and download authorization.

Alfred therefore ships **no** commerce backend: no Alfred payment gateway, no
account service, no license server, no webhook receiver, no email service, no
server-side database, and no server backups. Alfred calls Polar's public
customer-portal license endpoints directly and never carries a Polar access
token or webhook secret. Your complete license key stays in the operating
system credential store.

Official builds for v0.5.0 update **manually** through Polar. Alfred has no
automatic updater and never fetches an installer for you; the in-app
**Download Latest Version…** action only opens Polar's customer portal in your
browser, where you sign in by email to reach your personal downloads.

The macOS disk images are signed and notarized. The Windows NSIS installer is an
**unsigned beta**: Windows reports an unknown publisher and SmartScreen may
warn. It is never advertised as signed or warning-free. Linux packages remain
best-effort and are not a supported paid download.

A license activates on at most three devices. It refreshes after 7 days when the
network allows and tolerates at most 30 days offline; a confirmed revocation
applies immediately.

### What paying does not do

Buying an official build does not reduce the rights granted by the GPL,
including the right to redistribute it. Payment never disables local workflows,
runs, schedules, memories, or any other data on your machine, and a lapsed or
absent license does not lock you out of a build you compiled yourself. Building
from source does not include official binary support, signing, notarization, or
an assurance that a local modification will interoperate with future official
builds.

## Unofficial builds

A self-built or third-party build must not claim to be an official release.
Personal builds of an unmodified checkout may keep the default name and icon.
If you distribute a modified build, make the changes clear and follow
[BRANDING.md](../BRANDING.md), including renaming it when the presentation
could otherwise confuse users about its origin.

Never copy signing certificates, Polar access tokens, or other maintainer
secrets into a source build. They are not required to compile Alfred, and no
such secret ships in an official build either.

## Contributions

Contributions are accepted under GPL-3.0-or-later. See
[CONTRIBUTING.md](../CONTRIBUTING.md) for the development workflow and
[SECURITY.md](../SECURITY.md) for private vulnerability reporting.

## Policy changes

The license on an existing version cannot be retroactively withdrawn. A future
version may change its distribution or support terms, but previously published
GPL versions retain their existing rights.
