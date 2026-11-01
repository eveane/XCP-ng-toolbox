# Scrubbing (mandatory redaction)

Requirement: **no password, passkey, key or critical customer information may
leave the site in a bundle.** xoa-support enforces this in three layers:

1. **Never collect** — the collector and the sos plugin declare private-key
   paths as forbidden (`client-key.pem`, `*.key`, `id_rsa*`, …) and the
   collector self-checks the staging area for `BEGIN ... PRIVATE KEY`.
2. **Redact during/after collection** — one shared pattern list
   (`src/xoa_support/secret_patterns.py`) is applied by the sos plugin's
   `postproc()` and by `bin/xoa_scrub.py` to every text file in the bundle:
   - `password`/`passwd`/`pwd`/`passphrase` key-value pairs (TOML/JSON/YAML/ENV)
   - `secret`, `token`, `api-key`, `access-key`, `private-key`,
     `client-secret`, `shared-secret`, `encryption-key` pairs
   - S3 / AWS secret keys (backup remotes)
   - `redis://user:password@host` URIs (the `[redis] uri` in config.toml)
   - URLs with embedded credentials `scheme://user:pass@host`
   - `Authorization: Bearer/Basic/Token …` headers, JWTs, long-lived tokens
   - PEM `PRIVATE KEY` blocks (whole-block replacement)
   - long hex/base64 blobs that look like raw keys (≥32/≥40 chars)
3. **Verify (strict gate)** — `xoa_scrub.py --strict` (default) re-scans the
   scrubbed output; if any pattern still matches or a PEM block survives, the
   bundle is marked `FAIL` and the command exits 2. **Do not send a FAIL
   bundle.** A machine-readable `*.scrub-report.json` lists every file and
   how many redactions were applied.

Redaction keeps the *key* and replaces the *value* with `***REDACTED***`, so
scrubbed TOML/JSON stays parseable by machines.

## Running it

```sh
python3 bin/xoa_scrub.py bundle.tar.gz                 # strict, in-place-ish (-o)
python3 bin/xoa_scrub.py bundle.tar.gz -o out.tar.gz --strict
python3 bin/xoa_scrub.py bundle.tar.gz --no-strict      # report only (debug)
```

Output: `out.tar.gz` + `out.tar.gz.scrub-report.json`.

## Extending the patterns

Add a pattern to `SECRET_PATTERNS` in `secret_patterns.py` (with a test in
`tests/test_secret_patterns.py`). Both the sos plugin and the scrubber pick
it up automatically — that is why there is exactly one list.
