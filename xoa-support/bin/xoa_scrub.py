#!/usr/bin/env python3
"""xoa_scrub.py — redact and optionally anonymise an xoa-support bundle.

Runs AFTER collection, BEFORE the bundle leaves the customer site. This is
the security gate: it rewrites the archive applying the shared secret
patterns (xoa_support.secret_patterns), optionally anonymises identifiers
(--anonymise), verifies no secret remains (--strict, default on) and
writes a machine-readable scrub-report.json next to the output.

Usage:
  xoa_scrub.py BUNDLE.tar.gz [-o OUT.tar.gz] [--anonymise [--salt S]]
                [--no-strict] [--report REPORT.json]
  xoa_scrub.py --input-dir DIR -o OUT.tar.gz ...   # scrub a plain directory

Exit codes: 0 = clean, 1 = usage/IO error, 2 = strict-mode violation.

License: GPL-2.0-only.
"""

import argparse
import io
import json
import os
import re
import sys
import tarfile
import tempfile

sys.path.insert(
    0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src")
)

from xoa_support import secret_patterns as sp  # noqa: E402
from xoa_support.anonymise import Anonymiser  # noqa: E402

PEM_RE = re.compile(rb"-----BEGIN [A-Z ]*PRIVATE KEY")
REDACTED_BYTES = sp.REDACTED.encode()
BINARY_EXTS = {".ldb", ".log", ".sock", ".db", ".dat", ".bin"}
MAX_TEXT = 64 * 1024 * 1024


def is_probably_text(data: bytes) -> bool:
    if not data:
        return True
    sample = data[:4096]
    return b"\x00" not in sample


def scrub_bytes(data: bytes, anonymiser=None):
    """Return (scrubbed_bytes, n_redactions)."""
    if PEM_RE.search(data):
        return REDACTED_BYTES, 1
    if not is_probably_text(data) or len(data) > MAX_TEXT:
        return data, 0
    text = data.decode("utf-8", errors="surrogateescape")
    original = text
    hits = 0
    for _name, pattern, replacer in sp.SECRET_PATTERNS:
        text, n = pattern.subn(replacer, text)
        hits += n
    if anonymiser is not None:
        text = anonymiser.anonymise_text(text)
    if text == original:
        return data, 0
    return text.encode("utf-8", errors="surrogateescape"), max(hits, 1)


def process_archive(src, dst, anonymise, salt, strict, report):
    anon = Anonymiser(salt) if anonymise else None
    files = []
    violations = []
    with tarfile.open(src, "r:*") as tin, tarfile.open(dst, "w:gz") as tout:
        for member in tin:
            if not member.isfile():
                tout.addfile(member)
                continue
            data = tin.extractfile(member).read()
            if any(rx.search(os.path.basename(member.name)) for rx in sp.SENSITIVE_FILENAME_RES):
                files.append({"path": member.name, "action": "dropped-sensitive-file"})
                continue
            scrubbed, hits = scrub_bytes(data, anon)
            if strict and hits:
                residue = sp.scrub_text(scrubbed.decode("utf-8", "replace"))
                if residue != scrubbed.decode("utf-8", "replace") or PEM_RE.search(scrubbed):
                    violations.append(member.name)
            if scrubbed != data or hits:
                member.size = len(scrubbed)
                tout.addfile(member, io.BytesIO(scrubbed))
                files.append({"path": member.name, "action": "scrubbed", "redactions": hits})
            else:
                tout.addfile(member, io.BytesIO(data))
                files.append({"path": member.name, "action": "unchanged"})
        if anon is not None:
            payload = anon.mapping_json().encode()
            info = tarfile.TarInfo("anonymisation-map.json")
            info.size = len(payload)
            tout.addfile(info, io.BytesIO(payload))
    report.update(
        {
            "input": os.path.abspath(src),
            "output": os.path.abspath(dst),
            "anonymised": anonymise,
            "strict": strict,
            "files": files,
            "violations": violations,
            "status": "FAIL" if violations else "OK",
        }
    )
    return 2 if violations else 0


def main(argv=None):
    p = argparse.ArgumentParser(description="Scrub/anonymise an xoa-support bundle")
    p.add_argument("bundle", help="input bundle (.tar.gz) or directory")
    p.add_argument("-o", "--output", help="output bundle (default: <input>.scrubbed.tar.gz)")
    p.add_argument("--input-dir", action="store_true", help="treat BUNDLE as a plain directory")
    p.add_argument("--anonymise", action="store_true", help="replace identifiers with stable pseudonyms")
    p.add_argument("--salt", default="", help="salt for anonymisation (keep secret, enables cross-bundle correlation)")
    p.add_argument("--no-strict", action="store_true", help="do not fail if redaction verification fails")
    p.add_argument("--report", default=None, help="scrub report JSON path (default: alongside output)")
    args = p.parse_args(argv)

    strict = not args.no_strict
    if args.input_dir:
        tmp = tempfile.NamedTemporaryFile(suffix=".tar.gz", delete=False)
        with tarfile.open(tmp.name, "w:gz") as t:
            t.add(args.bundle, arcname=".")
        src = tmp.name
    else:
        src = args.bundle
    out = args.output or re.sub(r"(\.tar(\.gz|\.bz2|\.xz)?|)$", ".scrubbed.tar.gz", src, count=1)
    if out == src:
        out = src + ".scrubbed.tar.gz"
    report = {}
    rc = process_archive(src, out, args.anonymise, args.salt, strict, report)
    report_path = args.report or out + ".scrub-report.json"
    with open(report_path, "w") as fh:
        json.dump(report, fh, indent=2)
    print(f"status: {report['status']}  report: {report_path}")
    print(f"scrubbed bundle: {out}")
    if args.input_dir:
        os.unlink(src)
    return rc


if __name__ == "__main__":
    sys.exit(main())
