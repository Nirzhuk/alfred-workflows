# Legal package requirements

Every sealed runtime package must contain these exact legal inputs:

- `legal/openai-codex/LICENSE`, copied from commit
  `025a88adbd7ae4d448fc938b28d0446eb1753317` and SHA-256
  `d17f227e4df5da1600391338865ce0f3055211760a36688f816941d58232d8dc`.
- `legal/openai-codex/NOTICE`, copied from the same commit and SHA-256
  `9d71575ecfd9a843fc1677b0efb08053c6ba9fd686a0de1a6f5382fd3c220915`.
- License texts and notices for the embedded CPython distribution, Pydantic,
  pydantic-core, annotated-types, typing-extensions, typing-inspection, and the
  bundler/bootloader used to produce the executable.
- A final CycloneDX SBOM generated from the sealed target package. The checked
  in `sbom.cdx.json` is the minimum source-component expectation, not the final
  target SBOM.

The release lock is maintained in
`scripts/managed-runtimes/manifests/codex-python-sdk-0.147.0.json` and pins
CPython 3.11.16 from python-build-standalone release `20260825`, the SDK and
target CLI wheels, and target-specific `pydantic-core` wheels by exact URL and
SHA-256. The release input must additionally provide
`third-party-notices.json` with the expected license expression plus a license
and notice path/digest for every component in that manifest's legal inventory;
the checked-in source SBOM is never accepted as the sealed target SBOM.

The package manifest must declare these files as ordinary hashed resources.
The shared package verifier must reject a package when a legal file, SBOM,
Python runtime, SDK wheel, CLI wheel, or sidecar executable is undeclared or
does not match its trusted release digest.
