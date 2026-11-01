# Consuming a bundle (humans and machines)

The bundle is a plain `tar.gz` mirroring the XOA filesystem plus
`meta/MANIFEST.txt` — anything that reads tar + text + JSON works.

## Bundle map — where the key elements live

| bundle path | content | notes |
|---|---|---|
| `etc/xo-server/` | xo-server config (config.toml, …) | scrubbed; secrets `***REDACTED***` |
| `var/lib/xo-server/data/leveldb/` | xo-server database files (`.ldb`, `.log`, `CURRENT`, …) | `*.sock` excluded; binary — read with a leveldb reader |
| `var/lib/xo-server/data/rpu-traces/` | RPU traces: `*.ndjson` events + `*.heartbeat.json` | heartbeat gaps = RPU stalls |
| `var/lib/xo-server/data/sdn-controller/` | `ca-cert.pem`, `client-cert.pem` only | `client-key.pem` is NEVER collected (see `EXCLUDED.txt`) |
| `var/lib/xoa-updater/` | `registration.json`, `update.json`, license caches | registration email = PII → anonymise |
| `var/lib/xoa-backups/` | latest XOA metadata backups (`.xo.json.gz`, `*metadata*.gz`) | scrubbed; use for restore-into-blank-XOA |
| `var/log/redis/`, `var/log/journal-*.log` | redis logs + journald exports (xo-server, redis, nginx, …) | |
| `commands/*.out` | systemctl status, redis-cli info, df, listings, ps | machine-readable text |
| `sos/sosreport-*.tar.xz` (+ `.sha256`) | the full default sos report (when sos ran) | nested archive — extract it separately |
| `meta/MANIFEST.txt` | bundle inventory, host, date, collector version | |
| `anonymisation-map.json` | pseudonym mapping (only with `--anonymise`) | **secret — never publish** |
| `*.scrub-report.json` | per-file redaction report (next to the bundle, not inside) | |

## Quick summary (PoC)

```sh
python3 bin/xoa_bundle_inspect.py xoa.scrubbed.tar.gz           # human text
python3 bin/xoa_bundle_inspect.py xoa.scrubbed.tar.gz --json    # for agents/scripts
```

The inspector reports: file count, presence of `config.toml`, leveldb file
list, rpu-trace runs/heartbeats, redis log volume, xoa-updater files,
sdn-controller files — and warns (exit 1) if a private key is still present.

## Manual exploration

```sh
tar -tzf xoa.scrubbed.tar.gz | less
tar -xzf xoa.scrubbed.tar.gz -C /tmp/xoa && less /tmp/xoa/etc/xo-server/config.toml
```

## Planned machine readers (roadmap)

- **leveldb reader**: pure-python/stdlib-friendly reader for
  `/var/lib/xo-server/data/leveldb` (few CLI tools exist for leveldb);
  export keys/values to JSON. This is the highest-value piece for agents.
- **rpu-trace timeline**: parse `*.ndjson` + `*.heartbeat.json` into a
  chronological event table (text/JSON) — heartbeat intervals reveal RPU
  stalls.
- **redis dump parser**: if a `dump.rdb` or `redis-cli --scan` export is
  added, parse keyspace by prefix (`xo:` etc.) into JSON.
- **restore into blank XOA**: import the metadata backup into a disposable
  XOA (known UI/CLI, human-friendly) — requires restore-side changes to
  accept foreign UUIDs; tracked as future work, not part of the PoC.
