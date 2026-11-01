# xoa-support — design document

## Goal

Make airgapped troubleshooting of a Xen Orchestra Appliance (XOA) easy by
producing a single, **scrubbed**, optionally **anonymised** support bundle
that both humans and machines (including AI agents) can consume with
existing, well-known tooling.

## Non-goals

- Not a monitoring product, not a replacement for `xen-bugtool` on XCP-ng
  hosts (that side already exists in this repo: `bugtool_collector.sh`,
  `xcp-bundle-analyzer`).
- Not a remote-collection agent: everything runs on the XOA itself or on
  an operator laptop against an already-collected bundle.

## Principles

1. **Simple, boring technology.** Bash + tar + Python stdlib. No new
   runtime dependencies on the XOA (airgap-friendly). Existing knowledge
   (sos report, tarballs, JSON) is reused so humans and machines need no
   new tooling to read a bundle.
2. **Security by construction, then verified.** Collection *excludes*
   private key material by path; post-processing *redacts* secrets with one
   shared pattern list; a `--strict` gate *verifies* no secret pattern
   remains and fails the bundle otherwise. Defence in depth, in that order.
3. **One bundle, machine-consumable.** The bundle is a plain `tar.gz`
   containing the same directory layout as the XOA filesystem plus
   `meta/MANIFEST.txt`. Any tool that reads tar + text + JSON can consume it.
4. **Deterministic anonymisation.** Optional `--anonymise` replaces IPs,
   UUIDs, emails and hostnames with stable pseudonyms (SHA-256 based, salted
   by an operator-held secret) and writes `anonymisation-map.json` inside
   the bundle. The map is the only way to reverse it — treat it as secret.

## Architecture

```
XOA (airgapped)
  ├── sos report -o xoa            (sos-plugin/xoa.py, postproc redacts)
  └── collector/xoa-support-collect.sh   (no-deps fallback, same layout)
                │
                ▼  bundle.tar.gz (still on site)
  bin/xoa_scrub.py --strict [--anonymise]   ← security gate, always run
                │
                ▼  bundle.scrubbed.tar.gz + scrub-report.json
        (transfer out of the airgap, e.g. support ticket upload)
                │
  operator / support / agent
  └── bin/xoa_bundle_inspect.py [--json]   ← human + machine summary
```

Components:

| path | role | deps |
|---|---|---|
| `sos-plugin/xoa.py` | sos plugin (DebianPlugin) collecting XOA state into a standard sos report | sos ≥ 4.x on the XOA |
| `sos-plugin/install-plugin.sh` | installs the plugin + shared python package on the XOA | root, coreutils |
| `collector/xoa-support-collect.sh` | standalone collector when sos is unavailable/not wanted | bash, coreutils, tar |
| `src/xoa_support/secret_patterns.py` | **single source of truth** for secret redaction patterns | none |
| `src/xoa_support/anonymise.py` | deterministic, salted anonymiser | none |
| `bin/xoa_scrub.py` | redaction + anonymisation + strict verification + JSON report | python3 ≥ 3.9 |
| `bin/xoa_bundle_inspect.py` | bundle summary, text or `--json` (PoC of the consumer side) | python3 ≥ 3.9 |
| `tools/dependency_report.py` | generates `docs/DEPENDENCIES.md` | python3 |

## What is collected (XOA side)

- `/etc/xo-server/` (config.toml et al.) — scrubbed
- `/var/lib/xo-server/data/leveldb/` — the xo-server database files
- `/var/lib/xo-server/data/rpu-traces/` — RPU rolling-plugin-update traces (ndjson + heartbeat.json)
- `/var/lib/xo-server/data/sdn-controller/` — **public certs only**; `client-key.pem` is a forbidden path
- `/var/lib/xoa-updater/` — registration.json, update.json, license caches
- redis logs (`/var/log/redis`, journald `redis-server`) and xo-server logs/journal
- `systemctl` status, `redis-cli info`, disk usage, process list
- optionally the full default sos report, in the same archive (`--no-sos` to skip)

### Suggested additions (reviewed, not yet wired)

- `xoa` appliance-level state: `/etc/xoa/` (if present), `/var/log/xoa-updater/`
- `nginx` config under `/etc/nginx/` (XOA reverse proxy)
- `xo-cli` / API audit trail if exported by xo-server
- pool/host inventory as seen by XO (`redis-cli` keyspace summary, or an
  `xo-cli` export) — useful context for support, scrubbed/anonymised

## Rule precedence

When rules conflict: **xen-orchestra rules → XCP-ng-toolbox rules → sosreport
rules**. Concretely: xen-orchestra is AGPL/ISC licensed (we integrate via its
plugin/config interfaces, we do not copy its code; our own code is
GPL-2.0-only like sos); this repo uses semver `x.y.z` with patch-bump per
commit and minor bump only on explicit confirmation; sos plugins follow the
sos plugin API and GPL-2.0.

## Consuming the data (exploitation) — plan

Two complementary approaches, both to be built on top of this bundle:

1. **Restore into a disposable XOA** (human-friendly, known interface):
   import the metadata backup into a blank XOA with restore tuning to accept
   foreign UUIDs. Pros: uses the stock UI/CLI. Cons: needs changes on the
   restore path to accept the bundle; awkward for agents. Status: future work.
2. **Direct readers** (machine/agent-friendly, recommended PoC path):
   - `xoa_bundle_inspect.py` today: summary of config, leveldb files,
     rpu traces, redis logs.
   - Planned: a small leveldb reader (leveldb has few CLI tools — we will
     shell out to `python3 -c` with a pure-python reader or use the XOA's own
     node tooling offline), a redis-dump parser, and an ndjson trace
     timeline renderer. All stdlib-only so they run anywhere.

## Threat model (summary — see SECURITY.md)

- Bundle may contain customer secrets → redaction patterns + strict gate +
  never-collect paths.
- Bundle may contain PII → optional anonymisation with salted map.
- Tools themselves must be trustworthy → stdlib-only, dependency inventory,
  pip-audit in CI, Dependabot for repo metadata.
