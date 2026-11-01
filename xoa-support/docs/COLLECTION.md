# Collecting an XOA support bundle

## Installing on the XOA

### With internet access (online XOA)

```sh
# from any machine with network, then copy — or directly on the XOA:
git clone https://github.com/eveane/XCP-ng-toolbox.git
cd XCP-ng-toolbox/xoa-support

# optional: install the python package properly (still stdlib-only at runtime)
pip3 install .            # provides the xoa-scrub / xoa-bundle-inspect commands
```

Then pick Path A or B below. Without `pip install`, the tools work as plain
scripts (`python3 bin/xoa_scrub.py …`) — no installation needed at all.

### Airgapped XOA (no internet)

Nothing needs to be downloaded on the XOA itself: xoa-support has **zero
runtime dependencies** (bash + coreutils + tar + python3 ≥ 3.9, all present
on a stock XOA). From a machine with internet, fetch the files and copy them
over with any out-of-band channel (scp from a jump host, USB, …):

```sh
# on the online machine:
git clone https://github.com/eveane/XCP-ng-toolbox.git
cd XCP-ng-toolbox/xoa-support
# only these 4 files are needed on the XOA for the standalone path:
scp collector/xoa-support-collect.sh bin/xoa_scrub.py bin/xoa_bundle_inspect.py src/xoa_support/*.py root@xoa:/root/xoa-support/
# for the sos plugin path, also copy:
scp sos-plugin/xoa.py sos-plugin/install-plugin.sh root@xoa:/root/xoa-support/
```

```sh
# on the airgapped XOA:
cd /root/xoa-support
chmod +x xoa-support-collect.sh install-plugin.sh
# Path A (if sos is installed):  sudo ./install-plugin.sh && sudo sos report --batch -o xoa
# Path B (always works):         sudo ./xoa-support-collect.sh --no-sos -o /tmp/xoa.tar.gz
sudo python3 xoa_scrub.py /tmp/xoa.tar.gz --strict --anonymise -o /tmp/xoa.out.tar.gz
```

If the sos plugin is used on the XOA, `install-plugin.sh` copies `xoa.py` and
the `xoa_support/` package into the system sos plugin directory — that copy
is local (no network).

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
