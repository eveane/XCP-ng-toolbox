"""Canonical redaction patterns for xoa-support bundles.

Single source of truth for every secret-looking construct that must never
leave an XOA in a support bundle. Both the sos plugin (sos-plugin/xoa.py)
and the standalone scrubber (bin/xoa_scrub.py) import from here so the
two collection paths can never drift apart.

License: GPL-2.0-only (see ../../LICENSE), same as sosreport.
"""

import re

REDACTED = "***REDACTED***"


def _kv(m):
    """Keep 'key =' and redact only the value (keeps files parseable)."""
    return m.group("key") + m.group("sep") + REDACTED


def _scheme_at(m):
    """Keep 'scheme://' and redact the userinfo, keeping the host."""
    return m.group(1) + REDACTED + "@"


def _prefix(m):
    """Keep the leading prefix (group 1, e.g. 'Authorization: Bearer ')."""
    return m.group(1) + REDACTED


def _full(m):
    return REDACTED


# Each entry is (name, compiled regex, replacer). The replacer decides what
# survives: _kv keeps the key, _scheme_at keeps scheme+host, _prefix keeps
# the header prefix, _full redacts the whole match.
SECRET_PATTERNS = [
    # --- generic key = "value" pairs in config/JSON/TOML/YAML/ENV files ---
    (
        "kv-password",
        re.compile(
            r'(?i)(?P<key>[\x22\x27]?[\w.\-]*(?:password|passwd|pwd|passphrase)[\w.\-]*[\x22\x27]?)'
            r"(?P<sep>\s*[:=]\s*)"
            r"(?P<q>[\"']?)[^\s\"',;}\]]+(?P=q)"
        ),
        _kv,
    ),
    (
        "kv-secret",
        re.compile(
            r'(?i)(?P<key>[\x22\x27]?[\w.\-]*(?:secret|token|api[_-]?key|access[_-]?key)[\w.\-]*[\x22\x27]?)'
            r"(?P<sep>\s*[:=]\s*)"
            r"(?P<q>[\"']?)[^\s\"',;}\]]+(?P=q)"
        ),
        _kv,
    ),
    (
        "kv-private-key",
        re.compile(
            r'(?i)(?P<key>[\x22\x27]?[\w.\-]*(?:private[_-]?key|client[_-]?secret|shared[_-]?secret|encryption[_-]?key)[\w.\-]*[\x22\x27]?)'
            r"(?P<sep>\s*[:=]\s*)"
            r"(?P<q>[\"']?)[^\s\"',;}\]]+(?P=q)"
        ),
        _kv,
    ),
    # --- redis URI with credentials: redis://[db[:password]@]host:port/db ---
    (
        "redis-uri",
        re.compile(r"(?i)(redis(?:s)?://)[^\s/@]*@"),
        _scheme_at,
    ),
    # --- XAPI / XO server style pool secrets in xml/json ---
    (
        "xapi-secret",
        re.compile(
            r"(?i)(<value>(?:[^<]*(?:password|secret|passwd)[^<]*)</value>|"
            r"\b[\w.\-]*(?:password|secret)\b\s*[:=]\s*[\"'][^\"']*[\"'])"
        ),
        _full,
    ),
    # --- private key blocks (PEM) ---
    (
        "pem-private-key",
        re.compile(
            r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY(?: BLOCK)?-----"
            r".*?-----END (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY(?: BLOCK)?-----",
            re.DOTALL,
        ),
        _full,
    ),
    # --- bearer / basic auth headers and tokens in logs ---
    (
        "authorization-header",
        re.compile(r"(?i)(authorization\s*:\s*(?:bearer|basic|token)\s+)[^\s,;]+"),
        _prefix,
    ),
    (
        "jwt",
        re.compile(r"\beyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\b"),
        _full,
    ),
    # --- xo-server session / CSRF tokens ---
    (
        "xo-token",
        re.compile(r"(?i)\btoken\s*[:=]\s*[\"']?[A-Za-z0-9_\-]{16,}[\"']?"),
        _full,
    ),
    # --- S3 / backup remote credentials ---
    (
        "s3-secret",
        re.compile(
            r'(?i)(?P<key>[\x22\x27]?[\w.\-]*(?:aws[_-]?secret|secret[_-]?access[_-]?key|s3[_-]?secret)[\w.\-]*[\x22\x27]?)'
            r"(?P<sep>\s*[:=]\s*)"
            r"(?P<q>[\"']?)[^\s\"',;}\]]+(?P=q)"
        ),
        _kv,
    ),
    # --- URLs with embedded credentials: scheme://user:pass@host ---
    (
        "url-credentials",
        re.compile(r"(?i)(\b[a-z][a-z0-9+.\-]*://)[^\s/@]*@"),
        _scheme_at,
    ),
    # --- generic long hex/base64 blobs that look like keys (>=32 chars) ---
    (
        "long-hex-key",
        re.compile(r"\b[0-9a-fA-F]{32,}\b"),
        _full,
    ),
    (
        "long-b64-key",
        re.compile(r"\b[A-Za-z0-9+/]{40,}={0,2}\b"),
        _full,
    ),
]

# Filenames (basename match, case-insensitive) whose entire content is
# treated as secret and replaced wholesale. Never collect these in the
# first place (the collector skips them); the scrubber is the last line of
# defence if one slips through.
SENSITIVE_FILENAMES = [
    r"^client-key\.pem$",
    r"^.*-key\.pem$",
    r"^id_rsa.*$",
    r"^.*\.key$",
    r"^.*\.p12$",
    r"^.*\.pfx$",
    r"^credentials(\..+)?$",
    r"^\.env$",
    r"^secrets?\.(ya?ml|json|toml|ini)$",
]

SENSITIVE_FILENAME_RES = [re.compile(p, re.IGNORECASE) for p in SENSITIVE_FILENAMES]


def scrub_text(text):
    """Apply every SECRET_PATTERN to *text* and return the redacted string."""
    for _name, pattern, replacer in SECRET_PATTERNS:
        text = pattern.sub(replacer, text)
    return text


def count_redactions(text, scrubbed):
    """Cheap metric used in the scrub report: number of pattern hits applied."""
    return max(0, len(text) - len(scrubbed)) // max(1, len(REDACTED)) or (
        1 if text != scrubbed else 0
    )


# re.sub()-compatible replacement strings (usable as sos do_file_sub() subst)
# for each named pattern. Key-preserving where the pattern has named groups.
SOS_SUBSTS = {
    "kv-password": r"\g<key>\g<sep>" + REDACTED,
    "kv-secret": r"\g<key>\g<sep>" + REDACTED,
    "kv-private-key": r"\g<key>\g<sep>" + REDACTED,
    "s3-secret": r"\g<key>\g<sep>" + REDACTED,
    "redis-uri": r"\g<1>" + REDACTED + "@",
    "url-credentials": r"\g<1>" + REDACTED + "@",
    "authorization-header": r"\g<1>" + REDACTED,
}


def sos_subst(name):
    """Return the re.sub replacement string for pattern *name*."""
    return SOS_SUBSTS.get(name, REDACTED)
