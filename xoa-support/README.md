# xoa-support — XOA support bundle toolkit

Open-source (GPL-2.0-only) toolkit to **collect, scrub, optionally anonymise
and inspect support bundles from a Xen Orchestra Appliance (XOA)**, designed
for **airgapped** environments and easy consumption by **humans and machines
(including AI agents)**.

## Why

Troubleshooting an airgapped XOA today means manually gathering logs and
config, with the risk of leaking passwords/keys when sharing them. This
project makes the bundle:

- **complete** — default sos report + XOA-specific state (xo-server config,
  leveldb, rpu-traces, sdn-controller public certs, xoa-updater state,
  redis logs),
- **safe** — private keys never collected; secrets redacted by a single
  shared pattern list; a `--strict` gate fails the bundle if anything
  sensitive remains,
- **shareable** — optional deterministic `--anonymise` for IPs, UUIDs,
  emails, hostnames,
- **consumable** — plain `tar.gz` + JSON reports; `xoa_bundle_inspect.py`
  gives a text or `--json` summary for agents.

## Layout

| path | what |
|---|---|
| `sos-plugin/xoa.py` + `install-plugin.sh` | sos report plugin for XOA (Debian) |
| `collector/xoa-support-collect.sh` | standalone collector (no deps, airgap-safe) |
| `bin/xoa_scrub.py` | redaction + anonymisation + strict verification + JSON report |
| `bin/xoa_bundle_inspect.py` | bundle summary, text or `--json` |
| `src/xoa_support/` | shared python package (secret patterns, anonymiser) |
| `tools/` | dependency inventory generator |
| `docs/` | DESIGN, COLLECTION, SCRUBBING, ANONYMISATION, CONSUMING, DEPENDENCY-SECURITY, DEPENDENCIES (generated) |
| `tests/` | pytest suite + fixtures |

## Quick start

```sh
# on the XOA (as root) — either path:
sudo sos-plugin/install-plugin.sh && sudo sos report --batch -o xoa
# or, no-sos/airgap:
sudo collector/xoa-support-collect.sh -o /tmp/xoa.tar.gz

# then, always, before the bundle leaves the site:
python3 bin/xoa_scrub.py /tmp/xoa.tar.gz --strict --anonymise -o /tmp/xoa.out.tar.gz

# inspect (human or machine):
python3 bin/xoa_bundle_inspect.py /tmp/xoa.out.tar.gz [--json]
```

Full docs: [docs/DESIGN.md](docs/DESIGN.md) ·
[COLLECTION](docs/COLLECTION.md) · [SCRUBBING](docs/SCRUBBING.md) ·
[ANONYMISATION](docs/ANONYMISATION.md) · [CONSUMING](docs/CONSUMING.md) ·
[DEPENDENCY-SECURITY](docs/DEPENDENCY-SECURITY.md)

## Rules & licenses

Rule precedence: xen-orchestra → this repo (XCP-ng-toolbox) → sosreport.
Our own code is **GPL-2.0-only** (same as sosreport, whose plugin API we
target); we integrate with xen-orchestra (AGPL/ISC) via its config/plugin
interfaces and do not copy its code. See [LICENSE](LICENSE) and
[SECURITY.md](SECURITY.md).

## Versioning

`x.y.z` per this repository's rules (patch per commit; minor only with
explicit confirmation; major stays 0 until release-ready). See
[RELEASE_NOTES.md](RELEASE_NOTES.md) and `VERSION`.

## Tests

```sh
pip install -r requirements-dev.txt
python3 -m pytest tests/ -q
python3 tools/dependency_report.py   # must not change docs/DEPENDENCIES.md
bash -n collector/xoa-support-collect.sh
```
