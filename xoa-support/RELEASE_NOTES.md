# xoa-support release notes

Semver `x.y.z`. Follows this repository's versioning rules: every
commit/patch bumps `z`; a feature bumps `y` only with explicit confirmation;
`x` stays 0 until the first release-ready version.

## 0.1.0 (unreleased)

Initial scaffold:

- sos plugin `sos-plugin/xoa.py` (DebianPlugin) + `install-plugin.sh`
- standalone airgapped collector `collector/xoa-support-collect.sh`
- shared redaction patterns `src/xoa_support/secret_patterns.py`
- scrubber `bin/xoa_scrub.py` (redaction, `--anonymise`, `--strict` gate,
  machine-readable scrub report)
- inspector PoC `bin/xoa_bundle_inspect.py` (text + `--json`)
- deterministic anonymiser `src/xoa_support/anonymise.py`
- docs, tests, CI, dependency tracking (`tools/dependency_report.py`)
- License: GPL-2.0-only (same as sosreport)
