"""Tests for packaging/build-deb.sh — the self-contained .deb."""
import os
import shutil
import subprocess

import pytest

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
BUILD = os.path.join(ROOT, "packaging", "build-deb.sh")

pytestmark = pytest.mark.skipif(
    shutil.which("dpkg-deb") is None, reason="dpkg-deb not available"
)


@pytest.fixture(scope="module")
def deb(tmp_path_factory):
    out = tmp_path_factory.mktemp("deb")
    for attempt in range(3):
        r = subprocess.run(["bash", BUILD, str(out)], capture_output=True)
        if r.returncode == 0:
            break
        # CI runners occasionally deliver SIGPIPE (141) to the build's
        # children under capture; one retry makes the fixture robust.
        if attempt == 2:
            raise AssertionError(
                f"build-deb.sh failed rc={r.returncode}: {r.stderr.decode()[-500:]}"
            )
    debs = list(out.glob("xoa-support_*_all.deb"))
    assert len(debs) == 1
    return debs[0]


def test_deb_metadata(deb):
    info = subprocess.run(
        ["dpkg-deb", "--info", str(deb)], check=True, capture_output=True, text=True
    ).stdout
    assert "Package: xoa-support" in info
    assert "Architecture: all" in info
    assert "Depends: python3 (>= 3.9)" in info
    assert "postinst" in info and "prerm" in info


def test_deb_contains_commands_and_payload(deb):
    listing = subprocess.run(
        ["dpkg-deb", "-c", str(deb)], check=True, capture_output=True, text=True
    ).stdout
    for path in [
        "./usr/bin/xoa-support-collect",
        "./usr/bin/xoa_scrub",
        "./usr/bin/xoa_bundle_inspect",
        "./usr/share/xoa-support/bin/xoa_scrub.py",
        "./usr/share/xoa-support/sos-plugin/xoa.py",
        "./usr/share/xoa-support/src/xoa_support/secret_patterns.py",
        "./usr/share/xoa-support/LICENSE",
    ]:
        assert path in listing, path
    assert "__pycache__" not in listing
    assert ".pyc" not in listing


def test_deb_version_matches_VERSION_file(deb):
    version = open(os.path.join(ROOT, "VERSION")).read().strip()
    assert f"xoa-support_{version}_all.deb" == os.path.basename(deb)
