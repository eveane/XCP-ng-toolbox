# Security policy — xoa-support

## Reporting a vulnerability

Open a **private** security advisory on this repository
(GitHub → Security → Report a vulnerability) or contact the maintainers.
Do **not** open a public issue for undisclosed vulnerabilities.

## What this project protects

Support bundles contain customer secrets by nature. The project guarantees:

1. Private key material is **never collected** (forbidden paths in both the
   sos plugin and the collector; collector self-check aborts on PEM blocks).
2. Passwords, passkeys, tokens, API/S3 keys, redis URIs with credentials are
   **redacted** by the shared pattern list before the archive is sealed.
3. A **strict verification** (`xoa_scrub.py --strict`, default) fails the
   bundle (exit 2, `status: FAIL` in the scrub report) if any pattern still
   matches after scrubbing.
4. Optional **anonymisation** removes identifiers; the sidecar map is secret.

## Scope notes

- This project ships no network-facing code. The collector runs locally on
  the XOA; the scrubber/inspector run on bundles.
- Runtime has **zero third-party Python dependencies** (stdlib only), tracked
  in `docs/DEPENDENCIES.md`; dev deps are audited with `pip-audit` in CI.
- If you find a way to make a bundle leak a secret (pattern bypass, binary
  file smuggling, …), that is a security bug: report it privately and add a
  regression test in `tests/`.
