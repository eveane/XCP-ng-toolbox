# bugtool-collector

Collects `xen-bugtool` archives from every host of an XCP-ng pool over SSH and
packs them into one local archive named `<pool name or master IP>-<UTC date-time>.tar.gz`.

The collector script itself is [`../bugtool_collector.sh`](../bugtool_collector.sh);
this directory only adds the Debian packaging used to deploy it on a Xen
Orchestra Appliance (XOA, Debian-based).

## Install on the XOA

Download the latest release asset and install it (works airgapped — one file,
local `dpkg -i`):

```sh
wget https://github.com/eveane/XCP-ng-toolbox/releases/latest/download/bugtool-collector_$(...).deb
sha256sum -c SHA256SUMS
sudo dpkg -i bugtool-collector_*_all.deb
```

The package installs `/usr/bin/bugtool-collector` and depends on
`bash, sshpass, openssh-client, tar, coreutils, grep, gawk, findutils`.
The script also checks these dependencies itself at runtime (`dpkg -s`) and
installs missing ones with `apt-get`.

## Usage

```sh
sudo bugtool-collector [-j N] <pool_master_ip>
```

- `-j N` — max number of hosts processed in parallel (default `3`); `0`
  collects all bundles at once.
- The final archive is written to the current directory as
  `<pool name or master IP>-<UTC date-time>.tar.gz`; per-host logs are kept in
  `./bugtool_archives/host-logs/`.

## Release

Publishing follows the same pattern as `xoa-support`: on every merge to
`main` that changes `bugtool-collector/` or `bugtool_collector.sh`, the
`bugtool-collector-release` workflow builds the `.deb` and publishes it (with
`SHA256SUMS`) as GitHub Release `bugtool-collector-v<VERSION>` (tag
`bugtool-collector-v*`). `bugtool-collector-version-guard` fails any PR that
changes these paths without bumping `bugtool-collector/VERSION`.

Build the package manually with:

```sh
./packaging/build-deb.sh dist   # -> dist/bugtool-collector_<VERSION>_all.deb
```
