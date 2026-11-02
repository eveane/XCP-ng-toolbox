# AGENTS.md — guidance for AI coding agents working on xoa-support

## Project

Support-bundle toolkit for Xen Orchestra Appliance (XOA): collect (sos plugin
or standalone shell collector), scrub (redaction + anonymisation + strict
gate), inspect (text/JSON summary). GPL-2.0-only.

## Rule precedence (mandatory)

1. xen-orchestra (vatesfr/xen-orchestra) rules — AGPL/ISC; we integrate via
   interfaces, never copy its code.
2. This repository (eveane/XCP-ng-toolbox) rules — see root README:
   semver `x.y.z`, **every commit bumps `z`**, minor `y` only after asking the
   user, major `x` stays 0 until release-ready; conventional commits
   (`feat:`, `fix:`, `docs:`, `test:`, `chore:`); RELEASE_NOTES.md entry per
   bump; PR template sections must be filled.
3. sosreport (sosreport/sos) rules — plugin API, GPL-2.0.

## Architecture boundaries

- `src/xoa_support/secret_patterns.py` is the **single source of truth** for
  redaction. sos plugin and scrubber both import it. Never duplicate patterns.
- `src/xoa_support/anonymise.py` — deterministic salted anonymiser.
- `sos-plugin/xoa.py` — sos `Plugin, DebianPlugin`; collection + `postproc()`.
- `collector/xoa-support-collect.sh` — bash, coreutils only; forbidden paths
  for private keys; self-check for PEM blocks before archiving.
- `bin/xoa_scrub.py` — security gate: redact → optional anonymise → strict
  verify → `*.scrub-report.json`. Exit 2 on strict violation.
- `bin/xoa_bundle_inspect.py` — read-only consumer; text or `--json`.
- Never add runtime dependencies: the XOA runs these with stock python3.
  Dev-only deps go in `requirements-dev.txt`.
- Docs live in `docs/` and must be updated in the same commit as the change.

## File headers (mandatory)

Every script and core file under `xoa-support/` carries the standard
open-source header defined in the root README ("File headers"): one-line
purpose, `Author: eveane & Vibe Mistral`, date, and the GPL-2.0-only license
statement. Agents must add it to every new file and never strip or alter it
on existing ones.

## Commands (run in this order, stop at first failure)

```
python3 -m compileall -q src bin tools tests
python3 -m pytest tests/ -q
bash -n collector/xoa-support-collect.sh && bash -n sos-plugin/install-plugin.sh
bash -n packaging/build-deb.sh && ./packaging/build-deb.sh dist   # needs dpkg-deb
python3 tools/dependency_report.py && git diff --exit-code docs/DEPENDENCIES.md
```

## Debian packaging

- `packaging/build-deb.sh` builds a self-contained `Architecture: all` deb
  (`dist/xoa-support_<ver>_all.deb`) — the primary install path on the XOA,
  airgap-friendly (ship one file, `dpkg -i` locally, no repo clone).
- `packaging/control|postinst|prerm` — postinst auto-registers the sos plugin
  if sos is present; prerm removes it.
- The deb must never contain `__pycache__`/`*.pyc` (build script strips them;
  `tests/test_packaging.py` enforces it).

## Tests

- `tests/build_fixtures.py` regenerates `tests/fixtures/` — do NOT hand-edit
  fixtures.
- Every new secret pattern needs a test in `tests/test_secret_patterns.py`.
- The scrubber must be tested for: redaction, anonymise determinism, strict
  failure on residual secrets, PEM drop.

## Security

- Never print/log/commit real secrets; fixtures use obviously fake values.
- `anonymisation-map.json` is secret; docs must say so.
- Update `tools/external_tools.txt` when a new OS tool is invoked.

## Branching policy (mandatory)

- **Never commit or push to `main` or release branches.** Work only on a
  task branch (`vibe/<slug>-43fef9`, `fix/…`) and open a draft PR; a human
  reviews and merges. See `.github/BRANCHING.md` (enforced by the
  `human-only-main` ruleset + CODEOWNERS).
- Releases are automatic: on merge to main, `xoa-support-release` publishes
  `xoa-support_<VERSION>_all.deb` as release `xoa-support-v<VERSION>`.
  Always bump `xoa-support/VERSION` in the same PR (enforced by the
  `xoa-support-version-guard` workflow) — that is what triggers the release.
