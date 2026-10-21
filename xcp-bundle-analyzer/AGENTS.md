# AGENTS.md — Guidance for AI coding agents

## Project

Rust CLI that analyzes XCP-ng / XenServer bugtool status-report bundles.
Binary: `xcp-bundle-analyzer`.

## Architecture (respect these boundaries)

- `main.rs` — CLI parsing (clap), command dispatch. No business logic.
- `bundle.rs` — Input abstraction (tar / tar.gz / tar.bz2 / zip / directory /
  bare `xapi-db.xml`). No parsing of bundle content.
- `xapidb.rs` — `xapi-db.xml` → typed tables + xapi value decoding
  (`OpaqueRef:` refs, OCaml sets/maps, Xml_spaces whitespace protection).
- `inventory.rs` — `xensource-inventory` parsing.
- `report/` — Domain model. `types.rs` (serde structs = JSON contract, see
  `docs/SPEC.md`), `build.rs` (XapiDb → BundleReport), `anomalies.rs`,
  `consolidate.rs` (pool view), `util.rs`.
- `render/` — Output only: `text.rs`, `json.rs`, `html.rs`, `anomalies.rs`,
  `vm_display.rs`, `vdi_tree.rs`, `util.rs`. Never parses input.

Data flows one way: `bundle` → `xapidb` → `report` → `render`.
Rendering modules must not read files; parsing modules must not format output.

## Conventions

- Errors: `anyhow`, `bail!` / `with_context`, include the failing path in the message.
- Collections: `BTreeMap` everywhere (sorted, deterministic output is part of the UX).
- Doc comments (`///`) on every `pub` item; keep the xapi encoding notes referencing
  `db_xml.ml` / `xml_spaces.ml` — they document non-obvious behavior.
- No new dependencies without strong justification. Current deps are minimal.
- Do NOT hand-edit `tests/fixtures/`. Regenerate with:
  `python3 tests/build_fixtures.py`
- Sizes are stored in bytes (`u64`); display helpers (`gib`/`mib`) live in `render/util.rs` and `report/util.rs`.

## Commands (run in this order, stop at first failure)

```
cargo check
cargo clippy -- -D warnings
cargo test            # all tests run; no #[ignore]d tests
cargo fmt --check     # do not reformat unrelated code
```

## Commits

Conventional style: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`.

## Known issues

- none currently (the former /tmp/real-xapi-db.xml dependency was replaced by
  the committed synthetic fixture tests/fixtures/xapi-db-real-like.xml)
- HTML output supports only a single bundle per invocation.
