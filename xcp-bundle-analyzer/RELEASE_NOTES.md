# Release notes — xcp-bundle-analyzer

Versioning scheme: `x.y.z` (major.minor.patch) — every patch/commit bumps
`z`; every feature or addition bumps `y` only after explicit confirmation;
the major `x` stays static at 0 until the first release-ready version.

## v0.3.3 (patch)

- fix: template/OS mismatch now also reviews VMs cloned from a generic or
  unrecognized template ("Other install media", custom template names) —
  previously only templates with a known OS family were compared. When the
  pool has a better-matching template (same OS family and version, e.g.
  Windows Server 2019 → "Windows Server 2019 (64-bit)", Oracle Linux 8.10 →
  "Oracle Linux 8"), it is recommended in the finding; warning severity
  when guest metrics report the OS, info when the guess comes from the VM
  name.

## v0.3.2 (patch)

- feat: VM section shows the template each VM was cloned from (resolved via
  the template's children set, with other-config:base_template_name as
  fallback) — text, JSON and HTML outputs.
- feat: template/OS mismatch anomaly — a VM running an OS from a different
  family than its template (e.g. Windows on a Linux template) is flagged
  as a warning when guest metrics report the OS, and as a lower-confidence
  info when the OS is not detected or the VM is off and the guess comes
  from the VM name.

## v0.3.1 (patch)

- docs: versioning policy updated across the repository — every patch/commit
  bumps `z`, minor (`y`) bumps for features/additions require explicit
  confirmation, and the major (`x`) stays static at 0 until the first
  release-ready version.

## v0.3.0 (minor)

- feat: `completions` subcommand — shell auto-completion scripts for bash,
  zsh, fish, elvish, and PowerShell.
- feat: restructured long help (`xcp-bundle-analyzer --help` and
  `analyze --help`) with a clearly readable list of the sections available
  for the analyze command and a dedicated OUTPUT section documenting the
  html/json/text and file (`-o`) output options.
- docs: README — Windows installation instructions, shell completion setup,
  versioning policy.
- chore: version bumped to 0.3.0.

## v0.2.0 (minor)

- feat: `object` subcommand showing all details of one object, like
  `xe <object>-param-list` (vdi, vbd, vif, pbd, pif, sr, network, vm, host).
- feat: per-SR VDI statistics in the VDIs section (base / snapshots /
  orphaned counts and total size per SR group).
- feat: improve PBD section output — grouped per SR, disk-format
  preference order, multipath path counts, pool inconsistency highlights.
- feat: flag orphaned VHD base-copy chains in anomalies.
- fix: `--section vdis`/`srs`/`pbds` renders only its own sub-section.
- feat: report pool-level HA status (ha_enabled) in the pool section.
- test: replaced the /tmp/real-xapi-db.xml dependency with the committed
  synthetic fixture `tests/fixtures/xapi-db-real-like.xml`.
- refactor: split `report.rs` and `render.rs` into module directories.

## v0.1.x (patch)

- Initial releases: analyze/list/xapi-db/vm-display/vdi-tree subcommands,
  text/JSON/HTML output, `--section` filtering, consolidated pool view
  from several bundles, anomaly heuristics, VDI snapshot trees, VBD/VIF
  sections grouped per VM, storage categories and multipath details,
  per-SR VDI statistics groundwork, README and CI.
