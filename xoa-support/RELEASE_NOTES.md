# xoa-support release notes

Semver `x.y.z`. Follows this repository's versioning rules: every
commit/patch bumps `z`; a feature bumps `y` only with explicit confirmation;
`x` stays 0 until the first release-ready version.

## 0.1.22 (unreleased)

- fix(collector): temp-account export now uses the correct xo-cli flow —
  `--url https://user:pass@host/` is NOT valid (xo-cli signIn({token}) only
  accepts a real token; password-in-URL gives JSON-RPC "invalid credentials",
  as seen in the 0.1.21 field run). Now: after recover-account sets the
  random password, the collector runs
  `xo-cli register --allowUnauthorized <local-url> xoa-support <pw>`
  (signIn({email,password}) + token.create per xo-cli source), exports with
  the fresh token, then `xo-cli unregister` so no credential persists.
  Password still scrubbed from captured output, never logged.
- fix(sos plugin): sos drops the exec bit on collected directories
  (/etc/xo-server arrived as drw-r--r--) — postproc now chmod a+x on all
  dirs under the collected tree; also a setup-time find command as belt &
  braces. chmod logic unit-tested standalone.
- bump 0.1.22

## 0.1.21 (unreleased)

- fix(collector): on ANY failure the execution log is now preserved to
  /tmp/xoa-support-runlog-<UTC>.txt (EXIT trap copies meta/RUNLOG.txt before
  cleaning the staging dir) and a stderr note points to it — verified with a
  failing-run harness
- fix(collector): the whole temp-account (recover-account) strategy runs in a
  subshell with `|| log …`, so nothing inside can abort the run under set -e;
  the bundle is always finished
- chore: verified PR #74's header requirement still holds repo-wide
  (0 .sh/.py/.rs/postinst/prerm files missing the Author/Date/Open-source
  header)
- bump 0.1.21

## 0.1.20 (unreleased)

- fix(collector): recover-account discovery hardened — searches PATH
  (xo-recover-account, xo-server-recover-account), /usr/local/bin, /usr/bin,
  /opt/xo/{xo-server,packages}/…/dist, /opt/xen-orchestra/…, then the
  /opt/xo/xo-builds/* glob and a find fallback; runs it with stdin=/dev/null
  and `timeout 60` so it can never hang on a password prompt; extra local
  URLs tried (https/http x 127.0.0.1/localhost); every failure path logs to
  RUNLOG and the run always continues to bundle creation.
- bump 0.1.20

## 0.1.19 (unreleased)

- fix(collector): a failing xo-cli export no longer aborts the whole run
  (set -e + trap deleted the staging dir -> "bundle not created"): xo_export
  now catches the rc, logs it to xo-cli.out, and the collector falls through
  to the temp-account strategy and always finishes the bundle. Verified with
  a failing fake xo-cli under set -euo pipefail (run continues, rc=0).
- bump 0.1.19

## 0.1.18 (unreleased)


- feat(collector): on-demand XO config backup works without prior xo-cli
  registration: if no {server,token} is found in xo-cli's config, the local
  'xoa-support' XO admin account is reset with a random password via
  xo-server's recover-account CLI (xo-recover-account / xo-server-recover-account
  / /opt/xo/.../recover-account-cli.mjs; 2-arg form is non-interactive per
  recover-account-cli.mjs), then the local API is called as
  https://xoa-support:<pw>@127.0.0.1/ (https then http) with
  `xo-cli --url … xo.exportConfig compress:=true @=<bundle>/XO-config.json.gz`.
  Password is random (/dev/urandom), used once, scrubbed from captured output,
  never logged. Side effect (xoa-support admin account) documented in
  RUNLOG + COLLECTION.md.
- bump 0.1.18


- fix(collector): on-demand XO config backup auth — xo-cli reads
  {server,token} from the RUNNING user's ~/.config/xo-cli/config.json; as
  root that's empty ("Please use xo-cli register first"). The collector now
  harvests server+token from root's or $SUDO_USER's xo-cli config and passes
  them as `--url https://<token>@<host>/ --allowUnauthorized` (URL-encoded;
  token never logged). If no registration exists, RUNLOG says to run
  `xo-cli register <xoa-url> <user>` once as your user.
- fix(collector): --help now prints the full, current option list
  (-o/--no-sos/-s/--output-fd/-V/-h with examples)

## 0.1.15 (unreleased)

- fix(collector): xo-cli takes the export target as `@=<path>` — a bare path
  arg fails with "invalid arg" (parseParameters requires name=value; the
  $getFrom download goes to the @ param). Call is now
  `xo-cli xo.exportConfig compress:=true @=<bundle>/var/lib/xoa-backups/XO-config.json.gz`.
  Verified against xo-cli source + fake harness (bare arg rejected, @= works).
- bump 0.1.15

## 0.1.14 (unreleased)

- fix(collector): bundle filename now uses UTC (`date -u …Z`) instead of
  local XOA time: `xoa-support-<host>-YYYYMMDDTHHMMSSZ.tar.gz`
- bump 0.1.14

## 0.1.13 (unreleased)

- chore: document the mandatory open-source file header (purpose, authors
  eveane & Vibe Mistral, date, license statement) in the root README and
  both AGENTS.md files so every agent applies it to scripts and core files
- bump 0.1.13

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
