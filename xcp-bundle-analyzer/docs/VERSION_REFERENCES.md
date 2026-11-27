# Version reference tables used by xcp-bundle-analyzer

The analyzer runs **offline**, so "how fresh is this host?" is answered from
tables embedded at build time in `src/report/versions.rs`. This file documents
those tables, where each entry comes from, and how to refresh them.

Refresh policy (repo rule): **on every commit** that touches version knowledge,
check <https://xcp-ng.org/blog/tag/update/> (and the forum thread
"XCP-ng 8.3 updates announcements and testing") for new XCP-ng 8.3 LTS update
announcements, add/adjust entries, and bump `RELEASE_DB_LAST_UPDATED`.

## Tables in `src/report/versions.rs`

### `XCPNG_RELEASES` — XCP-ng GA dates
| product version | GA date |
|---|---|
| 7.4 | 2018-03-31 |
| 7.5 | 2018-08-10 |
| 7.6 | 2018-10-31 |
| 8.0 | 2019-12-19 |
| 8.1 | 2020-03-31 |
| 8.2 | 2020-11-18 |
| 8.2.1 | 2022-02-28 |
| 8.3 | 2024-10-07 (LTS since 2025-06-16, supported until 2028-11-30) |

Sources: docs.xcp-ng.org/releases, github.com/xcp-ng/xcp releases.

### `XENSERVER_RELEASES` — XenServer (Citrix Hypervisor) GA dates
7.0–8.3, for dumps reporting a XenServer product version instead of XCP-ng.
Sources: XenServer release notes / docs.xenserver.com.

### `XCPNG_83_UPDATES` — XCP-ng 8.3 LTS monthly update batches
The product version stays `8.3.0` across updates, so the update level is
derived from the host's `software_version:xapi` / `software_version:xen`
(which move with nearly every batch). Each entry is
`(min xapi version, min xen version, announcement date, source URL)`:

| min xapi | min xen | announced | source |
|---|---|---|---|
| 24.39 | 4.17.5 | 2025-05-26 | blog: may-2025-maintenance-update-for-xcp-ng-8-3 |
| 26.1.3 | 4.17.6 | 2026-03-10 | blog: march-2026-maintenance-updates-for-xcp-ng-8-3-lts |
| 26.1.11 | 4.17.7 | 2026-07-28 | blog: july-2026-updates-1-for-xcp-ng-8-3-lts |
| 26.2 | 4.17.7 | 2026-10-05 | blog: october-2026-updates-1-for-xcp-ng-8-3-lts |
| 26.2 | 4.17.7 | 2026-11-03 | blog: november-2026-security-and-maintenance-updates-1 |
| 26.2 | 4.17.7 | 2026-11-04 | blog: november-2026-security-updates-2 |
| 26.2 | 4.17.7 | 2026-11-10 | blog: november-2026-security-updates-3 |
| 26.1.23 | 4.17.7 | 2026-11-25 | blog: november-2026-updates-4-for-xcp-ng-8-3-lts |

Notes on the last entry (Nov 2026 Updates #4, 2026-11-25): xapi
`26.1.23-2.1.xcpng8.3` (forum announcement thread), kernel synced with
XenServer `4.19.19-8.0.51` (`4.19.19-8.0.51.1.xcpng8.3`), OpenSSH
`9.9p1-33.3.xcpng8.3`, OpenSSL 3.5.7, GnuTLS 3.8.12, Samba 4.23.3, swtpm
0.7.5, Windows guest tools 9.2.445, XO Lite 0.27.0, XSA-524/XSA-525 (AMD)
and NoisyTCP (CVE-2026-99288) fixes.

## Matching rules (see `estimate_83_update`)

- xapi lines are **not monotonic** across batches (Oct 2026 = 26.2.x,
  Nov #4 = 26.1.23), so matching is **same-line first**: the newest entry on
  the host's own xapi major.minor line with min <= host version wins; if the
  host's line is absent from the table, fall back to window matching (newest
  entry with min xapi <= host xapi).
- Hosts often report an abbreviated xapi (`26.1` for the `26.1.x` line): an
  equal prefix counts as that line (newest entry on the line wins).
- The xen version cross-checks the entry (`host xen >= entry min xen`).
- `newer_than_table` is set when the host's xapi is strictly above every
  entry AND its line is not in the table — i.e. the embedded DB is stale.

## XOSTOR / LINSTOR detection

XOSTOR is a LinstorSR SMAPI driver (github.com/xcp-ng/sm,
`drivers/linstorvolumemanager.py`): the SR is created with `type=linstor`
(`device-config:hosts=…, group-name=linstor_group/thin_device,
redundancy=N, shared=true`). In a bundle it is detected via:

- `SR.type` containing `linstor`/`xostor` → `category = "xostor"`
  (`classify_sr` in `src/report/util.rs`), shown in the STORAGE section;
- `host software_version` keys `xostor` / `linstor` → `HostReport.xostor_version`;
- fallback: a host attached (PBD) to a linstor SR is reported as
  "xostor present via SR …" even when no version key exists in the dump
  (older installs only ship the `sm-*.linstor.*` RPMs).
