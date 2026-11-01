# Anonymisation (optional)

On top of redaction, `--anonymise` replaces identifiers with **stable
pseudonyms** so support can still correlate events within the bundle without
learning the customer's real names:

| kind | example | becomes |
|---|---|---|
| IPv4/IPv6 | `10.0.0.5` | `10.x.y.z` (deterministic per original) |
| UUID | `6e81f99c-…` | random-looking UUID, same for same input |
| email | `admin@corp.example` | `user00042@example.invalid` |
| hostname/FQDN | `xoa-prod-1.corp.lan` | `host-00042.example.invalid` |

Properties:

- **Deterministic**: the same input always maps to the same pseudonym
  (SHA-256 of `salt + kind + value`), so timelines and joins still work.
- **Salted**: pass `--salt <secret>` (or env `XOA_ANON_SALT`) so pseudonyms
  cannot be brute-forced by a third party. Keep the salt if you need to
  correlate across bundles; discard it for maximum privacy.
- **Sidecar**: the mapping is written to `anonymisation-map.json` inside the
  bundle. **Treat this file as secret** — anyone with it (plus the salt) can
  reverse the anonymisation. Delete it before public sharing if not needed.

```sh
python3 bin/xoa_scrub.py bundle.tar.gz --anonymise --salt "$SECRET" -o out.tar.gz
```

Anonymisation runs **after** redaction, so secrets are already gone before
identifiers are replaced.
