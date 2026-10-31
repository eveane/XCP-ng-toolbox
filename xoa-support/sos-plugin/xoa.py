# Copyright (C) 2026 the xoa-support authors
# This file is part of xoa-support and is licensed under GPL-2.0-only
# (see the LICENSE file at the root of the xoa-support project), the same
# license as sosreport, so it can be shipped alongside sos plugins.

"""sos plugin for Xen Orchestra Appliance (XOA).

Collects XOA-specific state on top of the default sos report:
  - xo-server configuration (/etc/xo-server)
  - xo-server data dir (leveldb, rpu-traces), with private key material
    excluded at collection time
  - sdn-controller certificates (public certs only; client-key.pem is
    a forbidden path and is never collected)
  - xoa-updater state (/var/lib/xoa-updater)
  - redis and xo-server logs (journald + log files)
  - service status for xo-server / redis / xoa-updater

All collected files are post-processed with the shared secret patterns
from xoa_support.secret_patterns so passwords, keys, tokens and S3
credentials are redacted before the archive is sealed.
"""

import os
import sys

from sos.report.plugins import Plugin, DebianPlugin

# Allow running from a source checkout (tests, airgapped installs) by
# adding the bundled src/ dir to sys.path when the package is not installed.
_HERE = os.path.dirname(os.path.abspath(__file__))
_SRC = os.path.normpath(os.path.join(_HERE, "..", "src"))
if os.path.isdir(_SRC) and _SRC not in sys.path:
    sys.path.insert(0, _SRC)

from xoa_support import secret_patterns  # noqa: E402

LEVELDB_SIZELIMIT = 50 * 1024 * 1024  # bytes; tail beyond this
RPU_TRACES_SIZELIMIT = 20 * 1024 * 1024


class xoa(Plugin, DebianPlugin):
    """Xen Orchestra Appliance support data."""

    short_desc = "Xen Orchestra Appliance (XOA)"
    description = (
        "Collects xo-server configuration and state, xoa-updater state, "
        "redis logs and sdn-controller public certificates. Secrets are "
        "redacted during post-processing."
    )
    plugin_name = "xoa"
    packages = ("xo-server", "xoa-updater", "redis-server")
    # run even if the package list does not match (XOA installs may not
    # register dpkg packages the same way)
    _force_run = True

    def setup(self):
        # --- never collect private key material, even by accident ---
        self.add_forbidden_path(
            [
                "/var/lib/xo-server/data/sdn-controller/client-key.pem",
                "/var/lib/xo-server/**/*.key",
                "/var/lib/xo-server/**/id_rsa*",
                "/etc/xo-server/**/*.key",
                "/etc/xo-server/**/id_rsa*",
            ]
        )

        # --- xo-server configuration (scrubbed in postproc) ---
        self.add_copy_spec(
            [
                "/etc/xo-server",
                "/etc/xen-orchestra",
            ]
        )

        # --- xo-server data: leveldb + rpu traces (sized) ---
        self.add_copy_spec(
            "/var/lib/xo-server/data/leveldb",
            sizelimit=LEVELDB_SIZELIMIT,
        )
        self.add_copy_spec(
            "/var/lib/xo-server/data/rpu-traces",
            sizelimit=RPU_TRACES_SIZELIMIT,
        )
        # sdn-controller: public certs only (key forbidden above)
        self.add_copy_spec("/var/lib/xo-server/data/sdn-controller")

        # --- xoa-updater state (registration/update caches) ---
        self.add_copy_spec("/var/lib/xoa-updater")

        # --- redis logs ---
        self.add_copy_spec("/var/log/redis")
        self.add_journal(units=["redis-server", "redis"])

        # --- xo-server / xoa-updater logs and service state ---
        self.add_journal(units=["xo-server", "xoa-updater", "nginx"])
        self.add_copy_spec("/var/log/xo-server")
        self.add_cmd_output(
            [
                "systemctl status xo-server redis-server xoa-updater nginx --no-pager",
                "systemctl list-units 'xo*' --no-pager",
                "ps -eo pid,user,args | grep -E 'xo-server|redis' | grep -v grep",
                "redis-cli ping",
                "redis-cli info server",
                "redis-cli info persistence",
                "df -h /var/lib/xo-server",
                "ls -laR /var/lib/xo-server/data",
                "ls -la /var/lib/xoa-updater",
            ]
        )

    def postproc(self):
        # Apply every shared secret pattern to every file this plugin
        # collected. sos applies substitutions per file matching pathregex.
        for name, pattern, _replacer in secret_patterns.SECRET_PATTERNS:
            self.do_file_sub(
                r"sos_commands/xoa/.*",
                pattern.pattern,
                secret_patterns.sos_subst(name),
            )
            self.do_file_sub(
                r"etc/xo-server/.*",
                pattern.pattern,
                secret_patterns.sos_subst(name),
            )
            self.do_file_sub(
                r"var/lib/xo-server/.*",
                pattern.pattern,
                secret_patterns.sos_subst(name),
            )
            self.do_file_sub(
                r"var/lib/xoa-updater/.*",
                pattern.pattern,
                secret_patterns.sos_subst(name),
            )
            self.do_file_sub(
                r"var/log/.*",
                pattern.pattern,
                secret_patterns.sos_subst(name),
            )
        # belt and braces: scrub any certificate/private-key-looking content
        # from anything that slipped through (public certs are kept, keys are
        # not)
        self.do_file_private_sub(
            r"var/lib/xo-server/data/sdn-controller/.*", desc="certificate/key"
        )
