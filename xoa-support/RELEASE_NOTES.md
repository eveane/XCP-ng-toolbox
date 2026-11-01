# xoa-support release notes

Semver `x.y.z`. Follows this repository's versioning rules: every
commit/patch bumps `z`; a feature bumps `y` only with explicit confirmation;
`x` stays 0 until the first release-ready version.

## 0.1.6 (unreleased)

- feat: `-V`/`--version` on all three tools (xoa-support-collect,
  xoa-scrub, xoa-bundle-inspect)
- fix(collector): embedded sos call used `-o` (which is --only-plugins in
  sos 4.x, so it looked for a plugin named after the path) — now runs
  `sos report --batch --enable-plugins=xoa` in a work dir and moves the
  sos-report archive into the bundle
- fix(sos-plugin): resolve xoa_support from source tree, deb payload
  (/usr/share/xoa-support/src), or beside the sos plugins dir — fixes
  "No module named 'xoa_support'" when installed by postinst

## 0.1.4 (unreleased)

- feat: `packaging/build-deb.sh` now builds on non-Debian hosts (Manjaro/Arch,
  any Unix): falls back to plain `ar` + `tar` when no working `dpkg-deb` is
  found (a .deb is an ar archive of debian-binary + control.tar.gz +
  data.tar.gz); verified by building the deb with dpkg-deb hidden from PATH
- tests: packaging tests read the deb via dpkg-deb or ar+tar, and tolerate
  both listing formats
- docs: COLLECTION.md install section documents the Manjaro/Arch build path

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
