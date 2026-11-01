# Collecting an XOA support bundle

Two equivalent paths. Both produce the same layout and both feed
`xoa_scrub.py` before anything leaves the machine.

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
