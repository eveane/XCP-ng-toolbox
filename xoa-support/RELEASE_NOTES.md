# xoa-support release notes

Semver `x.y.z`. Follows this repository's versioning rules: every
commit/patch bumps `z`; a feature bumps `y` only with explicit confirmation;
`x` stays 0 until the first release-ready version.

## 0.1.12 (unreleased)

- chore: add a standard open-source header (authors eveane & Vibe Mistral, date,
  one-line purpose, license statement) to every script and core source file
  (shell, Python, Rust) in this project and the rest of the repository
- bump 0.1.12

## 0.1.10 (unreleased)

- feat(collector): generate a fresh XO config backup on demand before
  collecting — `xo-cli xo.exportConfig compress:=true` (xo-server API,
  admin) writes `XO-config_<date>.json.gz` into var/lib/xoa-backups/ in the
  bundle (token read from xo-cli's own config, never printed; file is
  scrubbed like everything else). Existing backups from the candidate dirs
  are collected as before.
- feat(collector): the tar.gz now extracts into its own directory
  (`xoa-support-<host>-<ts>/...`) via --transform; inspector strips that
  top-level dir when summarizing; scrubber preserves it
- feat(inspect): report lists xoa-backups files
- bump 0.1.10

## 0.1.9 (unreleased)

- fix(collector): search several XOA backup dirs (/var/lib/xoa-backups,
  /var/lib/xoa/backup, /var/backups/xoa, /var/lib/xo-server/backups) and
  log each collected metadata backup; sos plugin widened the same way
- fix(collector): restore exec bit on all staged directories before tar
  (cp -a of e.g. /etc/xo-server could lose it -> drw-r--r-- in the bundle)
- feat(collector): when run via sudo, chown the bundle to $SUDO_USER;
  print the bundle's md5sum at the end

## 0.1.8 (unreleased)

- fix(collector): sos 4.0 prints the archive path on the line AFTER
  "saved in:" (tab-indented) — the same-line parse missed it and the
  ls|head fallback could SIGPIPE under pipefail, aborting before the move
  (staging dir then removed by the EXIT trap -> empty sos/). Now: capture
  full sos output to a log, parse same-line then next-line, then glob
  tmp-dir/cwd; move archive + .sha256/.md5 into sos/. Verified with a fake
  sos 4.0 harness.
- fix(packaging): postinst/install-plugin.sh dropped xoa_support/*.py into
  sos's plugin dir, where sos 4.0 imports every *.py as a plugin
  ("plugin anonymise/secret_patterns does not install"). The package now
  goes to /usr/lib/python3/dist-packages/xoa_support (prerm cleans it);
  xoa.py resolves it there.

## 0.1.7 (unreleased)

- fix(collector): embedded sos archive was never moved into the bundle —
  sos 4.x writes `sosreport-<host>-<ts>.tar.xz` (+ `.sha256`) to `--tmp-dir`,
  not `sos-report-*` in cwd. The collector now parses sos's own
  "saved in:" line (fallback: glob tmp-dir), and puts the archive in `sos/`
- feat(collector + sos plugin): collect latest XOA metadata backups from
  `/var/lib/xoa-backups/` (`*.xo.json.gz`, `*metadata*.gz`, …) into
  `var/lib/xoa-backups/` in the bundle (scrubbed by xoa_scrub.py)
- docs(CONSUMING): new "Bundle map" table — where every key element lives
  (config, leveldb, rpu-traces, sdn-controller, xoa-updater, xoa-backups,
  logs, commands, sos archive, manifest, anonymisation map)

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
