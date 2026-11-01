# xoa-support release notes

Semver `x.y.z`. Follows this repository's versioning rules: every
commit/patch bumps `z`; a feature bumps `y` only with explicit confirmation;
`x` stays 0 until the first release-ready version.

## 0.1.3 (unreleased)

- fix: `packaging/build-deb.sh` no longer pipes `dpkg-deb --info` into `head`
  (SIGPIPE/141 under `set -o pipefail` made the CI build flaky); packaging
  test retries the build once for runner robustness

## 0.1.2 (unreleased)

- feat: self-contained Debian package — `packaging/build-deb.sh` builds
  `xoa-support_<ver>_all.deb` (commands on PATH, sos plugin auto-registered
  by postinst, LICENSE+docs in /usr/share). The XOA no longer needs to clone
  the repo: ship one .deb, `dpkg -i` locally (airgap-safe)
- docs: install section now leads with the .deb for both online and
  airgapped XOAs; raw-file copy kept as fallback
- tests: `tests/test_packaging.py` builds the deb and asserts metadata,
  payload, and no `__pycache__`/`.pyc`
- CI builds the deb on every push/PR

## 0.1.1 (unreleased)

- docs: explicit install instructions for online and airgapped XOAs
  (docs/COLLECTION.md "Installing on the XOA", README quick start)

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
