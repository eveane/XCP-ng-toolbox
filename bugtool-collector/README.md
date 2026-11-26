# bugtool-collector

Collects `xen-bugtool` archives from every host of an XCP-ng pool over SSH and
packs them into one local archive named `<pool name or master IP>-<UTC date-time>.tar.gz`.

The collector script itself is [`../bugtool_collector.sh`](../bugtool_collector.sh);
this directory only adds the Debian packaging used to deploy it on a Xen
Orchestra Appliance (XOA, Debian-based).

## Usage

```sh
sudo bugtool-collector [-j N] [pool_master_ip]
```

- `-j N` — max number of hosts processed in parallel (default `3`); `0`
  collects all bundles at once.
- `pool_master_ip` is **optional**: when omitted, the pools registered in the
  local XO server database are listed via `xo-server-db ls server`. With a
  single registered pool it is collected directly; with several you are asked
  to choose one. The SSH username/password are taken from the chosen XO server
  entry when available (otherwise you are prompted).
- The final archive is written to the current directory as
  `<pool name or master IP>-<UTC date-time>.tar.gz`; per-host logs are kept in
  `./bugtool_archives/host-logs/`.

## Dependencies

Hard requirement on the XOA: `openssh-client` (ssh/scp). `sshpass` is only a
`Recommends`: when absent, the script uses OpenSSH's `SSH_ASKPASS` mechanism,
so no extra package is needed for password auth. `bash`, `coreutils`, `grep`,
`tar`, `findutils` are Essential/required on Debian and `awk` comes from the
required-priority `mawk`, so they are not declared as dependencies. The script
checks `ssh`/`scp`/`tar` at runtime and `apt-get install`s the missing packages
(`dpkg -s` check first).

## Install on the XOA

Online — download the latest release asset and install it:

```sh
wget https://github.com/eveane/XCP-ng-toolbox/releases/latest/download/bugtool-collector_*_all.deb
wget https://github.com/eveane/XCP-ng-toolbox/releases/latest/download/SHA256SUMS
sha256sum -c SHA256SUMS
sudo dpkg -i bugtool-collector_*_all.deb
```

**Airgapped / no internet access** — use the self-contained airgap bundle,
which contains this package plus every dependency `.deb` (built for the XOA's
Debian release) and an `install.sh` that verifies checksums and runs
`dpkg -i` locally, no network needed:

```sh
wget https://github.com/eveane/XCP-ng-toolbox/releases/latest/download/bugtool-collector_*_airgap.tar.gz
tar -xzf bugtool-collector_*_airgap.tar.gz
cd airgap   # or bugtool-collector_<version>_airgap
sudo ./install.sh
```

The single `.deb` alone is also enough on a stock airgapped XOA, since the
only hard dependency (`openssh-client`) is part of the XOA base image.

## Release

Publishing follows the same pattern as `xoa-support`: on every merge to
`main` that changes `bugtool-collector/` or `bugtool_collector.sh`, the
`bugtool-collector-release` workflow builds the `.deb`, publishes it (with
`SHA256SUMS`) plus the airgap bundle as GitHub Release
`bugtool-collector-v<VERSION>` (tag `bugtool-collector-v*`,
`workflow_dispatch` fallbacks). `bugtool-collector-version-guard` fails any PR
that changes these paths without bumping `bugtool-collector/VERSION`.

Build the package manually with:

```sh
./packaging/build-deb.sh dist   # -> dist/bugtool-collector_<VERSION>_all.deb
```
