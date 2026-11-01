"""Deterministic anonymisation helpers for xoa-support bundles.

Replaces identifying values (hostnames, IPs, UUIDs, emails, usernames,
pool/SR/VM names) with stable pseudonyms so the same entity always maps to
the same pseudonym within one bundle (or across bundles when the same salt
is used). The mapping is written to an anonymisation-map.json sidecar that
MUST stay with the bundle and never be published.

License: GPL-2.0-only.
"""

import hashlib
import ipaddress
import json
import re

IPV4_RE = re.compile(r"\b(?:\d{1,3}\.){3}\d{1,3}\b")
IPV6_RE = re.compile(r"\b(?:[0-9a-fA-F]{1,4}:){2,7}[0-9a-fA-F]{1,4}\b")
UUID_RE = re.compile(
    r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b"
)
EMAIL_RE = re.compile(r"\b[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}\b")
HOSTNAME_RE = re.compile(
    r"\b(?:[a-zA-Z0-9](?:[a-zA-Z0-9\-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z]{2,63}\b"
)


class Anonymiser:
    """Stateful, deterministic anonymiser.

    salt: optional string (e.g. a per-customer random secret) that makes the
          pseudonyms non-guessable by third parties. Stored nowhere; the
          operator keeps it if cross-bundle correlation is needed.
    """

    def __init__(self, salt=""):
        self.salt = salt.encode("utf-8")
        self.mapping = {}

    def _pseudo(self, kind, value):
        key = (kind, value.lower())
        if key not in self.mapping:
            digest = hashlib.sha256(self.salt + kind.encode() + value.lower().encode())
            n = int.from_bytes(digest.digest()[:8], "big")
            if kind == "ip":
                self.mapping[key] = f"10.{n & 0xFF}.{(n >> 8) & 0xFF}.{(n >> 16) & 0xFF}"
            elif kind == "uuid":
                h = digest.hexdigest()
                self.mapping[key] = "-".join(
                    [h[0:8], h[8:12], h[12:16], h[16:20], h[20:32]]
                )
            elif kind == "email":
                self.mapping[key] = f"user{n % 100000:05d}@example.invalid"
            elif kind == "host":
                self.mapping[key] = f"host-{n % 100000:05d}.example.invalid"
            else:
                self.mapping[key] = f"{kind}-{n % 100000:05d}"
        return self.mapping[key]

    def anonymise_text(self, text):
        """Replace IPs, UUIDs, emails and hostnames in *text*."""
        # order matters: IPs before hostnames, UUIDs before generic host
        text = IPV6_RE.sub(lambda m: self._pseudo("ip", m.group(0)), text)
        text = IPV4_RE.sub(lambda m: self._pseudo("ip", m.group(0)), text)
        text = UUID_RE.sub(lambda m: self._pseudo("uuid", m.group(0)), text)
        text = EMAIL_RE.sub(lambda m: self._pseudo("email", m.group(0)), text)
        text = HOSTNAME_RE.sub(
            lambda m: self._safe_host(m), text
        )
        return text

    def _safe_host(self, m):
        value = m.group(0)
        # keep obvious non-hostnames that look like domains in code/config
        # (e.g. example.com placeholders, package names) untouched
        lower = value.lower()
        if lower.endswith(
            (".invalid", ".example", ".local", ".test", ".localhost")
        ) or lower in ("localhost",):
            return value
        try:
            ipaddress.ip_address(value)
            return value
        except ValueError:
            pass
        return self._pseudo("host", value)

    def mapping_json(self):
        return json.dumps(
            {f"{k[0]}:{k[1]}": v for k, v in sorted(self.mapping.items())},
            indent=2,
            sort_keys=True,
        )
