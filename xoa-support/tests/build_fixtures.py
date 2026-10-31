#!/usr/bin/env python3
"""Regenerate tests/fixtures/. Do not hand-edit fixtures — run this script."""
import json
import os
import tarfile
import io

HERE = os.path.dirname(os.path.abspath(__file__))
FIX = os.path.join(HERE, "fixtures")
os.makedirs(FIX, exist_ok=True)

CONFIG_TOML = """\
# fake XOA config for tests — values are obviously fake
[redis]
uri = 'redis://:Sup3rSecret@10.20.30.40:6379/0'

[backups.defaultSettings.remotes]
"s3://bucket" = { type = "s3", host = "s3.example.invalid", accessKeyId = "AKIAIOSFODNN7EXAMPLE", secretAccessKey = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY", bucket = "backups" }

[authentication]
password = "hunter2-fake"
"""

REDIS_LOG = """\
12345:M 01 Jan 2026 10:00:00.000 * Ready to accept connections
12345:M 01 Jan 2026 10:01:00.000 * AUTH failed for user default: redis://:Sup3rSecret@10.20.30.40:6379/0
12345:M 01 Jan 2026 10:02:00.000 * Client closed connection token=abc123def456ghi789
"""

HEARTBEAT = {
    "runId": "rpu-6e81f99c-f05c-54c1-1458-3275ba6803dc-20261028T153014054Z",
    "status": "running",
    "startedAt": "2026-10-28T15:30:14Z",
}

NDJSON = "\n".join(
    json.dumps({"ts": f"2026-10-28T15:30:{s:02d}Z", "event": e})
    for s, e in enumerate(["start", "step-1", "step-2", "done"])
)

UPDATER = {
    "email": "admin@customer-example.invalid",
    "plan": "premium",
    "password": "should-be-redacted",
}

PEM_KEY = """-----BEGIN RSA PRIVATE KEY-----
MIIEowIBAAKCAQEAfakefakefakefakefakefakefakefakefake
-----END RSA PRIVATE KEY-----
"""


def write(rel, content, binary=False):
    path = os.path.join(FIX, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    mode = "wb" if binary else "w"
    with open(path, mode) as fh:
        fh.write(content)


def main():
    write("bundle/etc/xo-server/config.toml", CONFIG_TOML)
    write("bundle/var/log/redis/redis-server.log", REDIS_LOG)
    write(
        "bundle/var/lib/xo-server/data/rpu-traces/"
        "rpu-6e81f99c-f05c-54c1-1458-3275ba6803dc-20261028T153014054Z.heartbeat.json",
        json.dumps(HEARTBEAT, indent=2),
    )
    write(
        "bundle/var/lib/xo-server/data/rpu-traces/"
        "rpu-6e81f99c-f05c-54c1-1458-3275ba6803dc-20261028T153014054Z.ndjson",
        NDJSON + "\n",
    )
    write("bundle/var/lib/xo-server/data/leveldb/CURRENT", "MANIFEST-000593\n")
    write("bundle/var/lib/xoa-updater/registration.json", json.dumps(UPDATER, indent=2))
    write(
        "bundle/var/lib/xo-server/data/sdn-controller/client-cert.pem",
        "-----BEGIN CERTIFICATE-----\nfakepubliccert\n-----END CERTIFICATE-----\n",
    )
    write(
        "bundle/var/lib/xo-server/data/sdn-controller/client-key.pem",
        PEM_KEY,
    )
    write("bundle/meta/MANIFEST.txt", "fake bundle manifest\n")
    # tar.gz version of the same tree
    tarpath = os.path.join(FIX, "bundle.tar.gz")
    with tarfile.open(tarpath, "w:gz") as t:
        t.add(os.path.join(FIX, "bundle"), arcname=".")
    print(f"fixtures written under {FIX}")


if __name__ == "__main__":
    main()
