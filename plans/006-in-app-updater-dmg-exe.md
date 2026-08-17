# Deferred: automatic in-app updates

The backendless Polar v0.5.0 release uses manual downloads from Polar's hosted
customer portal. Execute
[release-money Plan 004](release-money/004-publish-signed-polar-downloads.md)
to make the in-app action open that channel and to keep
`uploadUpdaterJson: false`.

Paid automatic updates are a separate future architecture decision. They need
either public signed updater assets or a small authenticated manifest/asset
service. Do not reintroduce the rejected general commerce backend under the
name of an updater.
