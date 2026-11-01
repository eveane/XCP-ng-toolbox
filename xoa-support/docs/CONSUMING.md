# Consuming a bundle (humans and machines)

The bundle is a plain `tar.gz` mirroring the XOA filesystem plus
`meta/MANIFEST.txt` — anything that reads tar + text + JSON works.

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
