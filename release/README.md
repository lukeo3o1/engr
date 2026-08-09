# Release artifacts

`release-manifest.template.json` is the checked-in target contract. For a tagged release,
each packaging job emits its archive, SHA-256 checksum, CycloneDX SBOM, and a per-target
manifest. The publish job merges them into `release-manifest.json` and `TOOLING.lock.json`
before attaching them to the GitHub release. Generated release files are intentionally not
committed.
