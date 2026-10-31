# Dependency & supply-chain security tracking

Requirement: be able to answer "are we exposed to CVE X?" within minutes.

## What we depend on

- **Python runtime: nothing.** `requirements.txt` is empty — the collector,
  scrubber and inspector run on the XOA's stock `python3` (≥3.9), standard
  library only. No pip, no network needed on the appliance.
- **Python dev/test: `pytest>=7`** (`requirements-dev.txt`) — CI-only.
- **OS tools**: tar, find, cp, systemctl, journalctl, redis-cli, sos, python3
  — all from the XOA/Debian base image (see `tools/external_tools.txt`),
  covered by the XCP-ng/Debian update feed.
- **Upstream projects** (rules/license/security relevant, no code copied):
  `vatesfr/xen-orchestra` (AGPL/ISC), `sosreport/sos` (GPL-2.0),
  this repo `eveane/XCP-ng-toolbox`.

## How exposure is tracked

1. `tools/dependency_report.py` regenerates **`docs/DEPENDENCIES.md`** from
   the requirements files and `tools/external_tools.txt`. CI re-runs it and
   **fails if the committed file is stale**, so the inventory can never
   silently drift.
2. CI runs **`pip-audit`** against `requirements-dev.txt` on every push/PR.
3. **Dependabot** (`/.github/dependabot.yml`) watches GitHub Actions versions
   used in our workflows and opens PRs on security updates.
4. OS-level CVEs on the XOA are handled by the normal XOA update channel —
   out of scope for this repo, but the bundle collector records the XOA
   version in `meta/MANIFEST.txt` so support can correlate.
5. Upstream watch: xen-orchestra and sosreport releases are monitored via
   their GitHub release feeds (manually/Dependabot-adjacent); when sos
   changes its plugin API we adapt `sos-plugin/xoa.py`.

## When a new CVE is published

1. Check `docs/DEPENDENCIES.md` — is the affected component in our inventory?
2. If Python dep: bump the pin in `requirements-dev.txt`, let Dependabot/CI
   verify. If stdlib-only code is affected (rare), patch and release.
3. If OS tool: it ships with the XOA base image → advise updating the XOA;
   record in RELEASE_NOTES.md.
4. If sos plugin API: patch `sos-plugin/xoa.py`, add a test.
