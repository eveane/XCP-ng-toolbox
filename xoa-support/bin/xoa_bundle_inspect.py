#!/usr/bin/env python3
"""xoa_bundle_inspect.py — summarize an xoa-support bundle for humans and machines.

Proof of concept for the "consume" side: reads a bundle (tar.gz or plain
directory), extracts the xo-server config, counts leveldb files, parses
rpu-traces ndjson heartbeats, samples redis logs and emits either a
human-readable text summary (default) or JSON (--json) suitable for agents.

Usage:
  xoa_bundle_inspect.py BUNDLE.tar.gz [--json] [--keys SECTION...]

License: GPL-2.0-only.
"""

import argparse
import io
import json
import os
import re
import sys
import tarfile

sys.path.insert(
    0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src")
)

NDJSON_RE = re.compile(r"\.ndjson$")
REDIS_LOG_RE = re.compile(r"redis.*\.log$|journal-redis", re.IGNORECASE)


def open_bundle(path):
    if os.path.isdir(path):
        return None, path
    return tarfile.open(path, "r:*"), None


def iter_files(tf, root):
    if tf is None:
        for dirpath, _dirs, files in os.walk(root):
            for f in files:
                full = os.path.join(dirpath, f)
                yield os.path.relpath(full, root), open(full, "rb")
    else:
        for m in tf:
            if m.isfile():
                yield m.name, tf.extractfile(m)


def summarise(path):
    tf, root = open_bundle(path)
    summary = {
        "bundle": os.path.abspath(path),
        "files_total": 0,
        "config_toml_present": False,
        "leveldb_files": [],
        "rpu_traces": {"ndjson": 0, "heartbeat": 0, "runs": []},
        "redis_log_lines": 0,
        "updater_files": [],
        "sdn_controller_files": [],
        "warnings": [],
    }
    for name, fh in iter_files(tf, root):
        summary["files_total"] += 1
        base = os.path.basename(name)
        data = fh.read() if fh is not None else b""
        if name.endswith("config.toml") or "xo-server" in name and base.endswith(".toml"):
            summary["config_toml_present"] = True
        if "/leveldb/" in name or name.startswith("var/lib/xo-server/data/leveldb"):
            summary["leveldb_files"].append(name)
        if "/rpu-traces/" in name:
            if NDJSON_RE.search(name):
                summary["rpu_traces"]["ndjson"] += 1
                m = re.search(r"(rpu(?:-finalize)?-[0-9a-f-]+)-(\d{8}T\d+ )?", name)
                rid = m.group(1) if m else base
                if rid not in summary["rpu_traces"]["runs"]:
                    summary["rpu_traces"]["runs"].append(rid)
            elif name.endswith(".heartbeat.json"):
                summary["rpu_traces"]["heartbeat"] += 1
                try:
                    hb = json.loads(data.decode("utf-8", "replace"))
                    summary["rpu_traces"].setdefault("last_heartbeat", hb)
                except Exception:
                    summary["warnings"].append(f"unparsable heartbeat: {name}")
        if REDIS_LOG_RE.search(name):
            summary["redis_log_lines"] += data.count(b"\n")
        if "/xoa-updater/" in name:
            summary["updater_files"].append(base)
        if "/sdn-controller/" in name:
            summary["sdn_controller_files"].append(base)
            if base.endswith("-key.pem") or base == "client-key.pem":
                summary["warnings"].append(
                    f"PRIVATE KEY PRESENT IN BUNDLE: {name} — run xoa_scrub.py --strict"
                )
    if tf is not None:
        tf.close()
    return summary


def render_text(s):
    lines = [
        f"bundle: {s['bundle']}",
        f"files: {s['files_total']}",
        f"xo-server config.toml present: {s['config_toml_present']}",
        f"leveldb files: {len(s['leveldb_files'])}",
        f"rpu-traces: {s['rpu_traces']['ndjson']} ndjson, "
        f"{s['rpu_traces']['heartbeat']} heartbeats, runs: {len(s['rpu_traces']['runs'])}",
        f"redis log lines: {s['redis_log_lines']}",
        f"xoa-updater files: {', '.join(s['updater_files']) or '-'}",
        f"sdn-controller files: {', '.join(s['sdn_controller_files']) or '-'}",
    ]
    if s["warnings"]:
        lines.append("WARNINGS:")
        lines += [f"  - {w}" for w in s["warnings"]]
    return "\n".join(lines)


def main(argv=None):
    p = argparse.ArgumentParser(description="Inspect an xoa-support bundle")
    p.add_argument("bundle")
    p.add_argument("--json", action="store_true", help="emit JSON for machines/agents")
    args = p.parse_args(argv)
    s = summarise(args.bundle)
    if args.json:
        print(json.dumps(s, indent=2, default=str))
    else:
        print(render_text(s))
    return 1 if any("PRIVATE KEY" in w for w in s["warnings"]) else 0


if __name__ == "__main__":
    sys.exit(main())
