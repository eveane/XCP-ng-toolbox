# Collecting an XOA support bundle

## Installing on the XOA

### Recommended: the self-contained .deb (works online AND airgapped)

xoa-support ships as **one self-contained `Architecture: all` deb** — the XOA
never needs to clone this repository. Build it once on any machine, then
transfer the single `.deb` file to the XOA by any channel (scp from a jump
host, USB, …). The build script works everywhere:

- **Debian/XOA/Ubuntu**: uses `dpkg-deb` (preinstalled)
- **Manjaro/Arch or any Unix**: falls back to plain `ar` + `tar` (both in the
  base system) — a `.deb` is just an `ar` archive of `debian-binary` +
  `control.tar.gz` + `data.tar.gz`. Alternatively `yay -S dpkg` from the AUR.

```sh
# on any online machine (repo checkout):
./xoa-support/packaging/build-deb.sh            # -> dist/xoa-support_<ver>_all.deb
scp dist/xoa-support_*_all.deb root@xoa:/root/

# on the XOA (airgap-safe: dpkg -i is fully local):
sudo dpkg -i /root/xoa-support_*_all.deb
```

This installs:
- `/usr/bin/xoa-support-collect`, `/usr/bin/xoa-scrub`, `/usr/bin/xoa_bundle_inspect`
- payload + docs + LICENSE under `/usr/share/xoa-support/`
- the sos plugin, auto-registered by `postinst` when `sos` is present
  (`sos report --batch -o xoa` then works)

Depends declared: `python3 (>= 3.9)`, `tar`, `coreutils`, `bash` — all already
on a stock XOA; `sosreport`/`redis-tools` are only `Recommends`.

### Alternative: run from a source checkout (online XOA)

```sh
git clone https://github.com/eveane/XCP-ng-toolbox.git
cd XCP-ng-toolbox/xoa-support
# optional: pip3 install .  — provides xoa-scrub / xoa-bundle-inspect commands
# without pip the tools work as plain scripts: python3 bin/xoa_scrub.py …
```

### Fallback: copy raw files (airgap, no .deb tooling)

xoa-support has **zero runtime dependencies** (bash + coreutils + tar +
python3 ≥ 3.9). From an online machine, copy the files over with any
out-of-band channel:

```sh
scp xoa-support/collector/xoa-support-collect.sh \
    xoa-support/bin/xoa_scrub.py xoa-support/bin/xoa_bundle_inspect.py \
    xoa-support/src/xoa_support/*.py \
    root@xoa:/root/xoa-support/
# for the sos plugin path, also: xoa-support/sos-plugin/{xoa.py,install-plugin.sh}
```

```sh
# on the XOA:
cd /root/xoa-support && chmod +x xoa-support-collect.sh install-plugin.sh
sudo ./xoa-support-collect.sh --no-sos -o /tmp/xoa.tar.gz
sudo python3 xoa_scrub.py /tmp/xoa.tar.gz --strict --anonymise -o /tmp/xoa.out.tar.gz
```

## Two equivalent collection paths

Both produce the same bundle layout and both feed `xoa_scrub.py` before
anything leaves the machine.

## Path A — sos report with the xoa plugin (preferred on a stock XOA)

```sh
# on the XOA, as root:
sudo ./sos-plugin/install-plugin.sh
sudo sos report --batch -o xoa          # xoa plugin runs with the default set
# or xoa only:
sudo sos report --batch -o xoa --only-plugins=xoa
```

The plugin (`sos-plugin/xoa.py`) collects the paths listed in
[DESIGN.md](DESIGN.md) and redacts secrets in `postproc()` using the shared
pattern list.

## Path B — standalone collector (airgap / sos missing / minimal)

Copy `collector/xoa-support-collect.sh` and `bin/xoa_scrub.py` onto the XOA
(e.g. via scp), then:

```sh
sudo ./xoa-support-collect.sh                 # includes sos report if sos exists
sudo ./xoa-support-collect.sh --no-sos -o /tmp/xoa.tar.gz
```

The script refuses to finish if private-key material is detected in the
staging area, and never copies `sdn-controller/client-key.pem`.

## Always, before the bundle leaves the site

```sh
python3 xoa_scrub.py /tmp/xoa.tar.gz --strict --anonymise -o /tmp/xoa.scrubbed.tar.gz
```

See [SCRUBBING.md](SCRUBBING.md) and [ANONYMISATION.md](ANONYMISATION.md).
Exit code 2 means the strict verification found residual secret material —
do not send that bundle.
