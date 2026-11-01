"""Import/structure tests for the sos plugin, with a stubbed sos package.

The real sos is not installed in CI; we stub sos.report.plugins so the plugin
module imports and its setup()/postproc() wiring can be verified.
"""
import os
import sys
import types

import pytest

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, ".."))
sys.path.insert(0, os.path.join(ROOT, "src"))
sys.path.insert(0, os.path.join(ROOT, "sos-plugin"))


class _StubPlugin:
    def __init__(self):
        self.copied = []
        self.forbidden = []
        self.cmds = []
        self.journals = []
        self.subs = []

    def add_copy_spec(self, spec, sizelimit=None):
        self.copied.append((spec, sizelimit))

    def add_forbidden_path(self, paths):
        self.forbidden.extend(paths)

    def add_cmd_output(self, cmds):
        self.cmds.extend(cmds)

    def add_journal(self, units=None):
        self.journals.extend(units or [])

    def do_file_sub(self, pathregex, regexp, subst):
        self.subs.append((pathregex, regexp, subst))

    def do_file_private_sub(self, pathregex, desc=""):
        self.subs.append((pathregex, "PRIVATE", desc))


@pytest.fixture()
def sos_stub(monkeypatch):
    sos = types.ModuleType("sos")
    report = types.ModuleType("sos.report")
    plugins = types.ModuleType("sos.report.plugins")

    class Plugin(_StubPlugin):
        pass

    class DebianPlugin:
        pass

    plugins.Plugin = Plugin
    plugins.DebianPlugin = DebianPlugin
    report.plugins = plugins
    sos.report = report
    monkeypatch.setitem(sys.modules, "sos", sos)
    monkeypatch.setitem(sys.modules, "sos.report", report)
    monkeypatch.setitem(sys.modules, "sos.report.plugins", plugins)
    return plugins


def test_plugin_setup_and_postproc(sos_stub):
    import xoa

    p = xoa.xoa()
    p.setup()
    flat_forbidden = " ".join(p.forbidden)
    assert "sdn-controller/client-key.pem" in flat_forbidden
    specs = " ".join(str(s) for s, _ in p.copied)
    assert "/etc/xo-server" in specs
    assert "/var/lib/xoa-updater" in specs
    assert "/var/lib/xo-server/data/leveldb" in specs
    assert "/var/lib/xo-server/data/rpu-traces" in specs

    p.postproc()
    assert p.subs, "postproc must register redaction substitutions"
    regexes = " ".join(r for _, r, _ in p.subs)
    assert "password" in regexes.lower()
    assert "PRIVATE KEY" in regexes
