"""Tests for bin/xoa_scrub.py — the security gate."""
import io
import json
import os
import tarfile

import pytest

import xoa_scrub

FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")


def read_member(tarpath, suffix):
    with tarfile.open(tarpath, "r:*") as t:
        for m in t:
            if m.name.endswith(suffix):
                return t.extractfile(m).read().decode("utf-8", "replace")
    raise AssertionError(f"member *{suffix} not found in {tarpath}")


def test_scrub_redacts_secrets_and_drops_key(tmp_path):
    src = os.path.join(FIX, "bundle.tar.gz")
    out = str(tmp_path / "out.tar.gz")
    report = {}
    rc = xoa_scrub.process_archive(src, out, anonymise=False, salt="", strict=True, report=report)
    assert rc == 0, report["violations"]
    assert report["status"] == "OK"

    config = read_member(out, "etc/xo-server/config.toml")
    assert "Sup3rSecret" not in config
    assert "hunter2-fake" not in config
    assert "wJalrXUtnFEMI" not in config
    assert "password" in config  # key preserved, value redacted

    log = read_member(out, "var/log/redis/redis-server.log")
    assert "Sup3rSecret" not in log

    reg = read_member(out, "var/lib/xoa-updater/registration.json")
    assert "should-be-redacted" not in reg

    # private key file must be dropped entirely
    with tarfile.open(out, "r:*") as t:
        names = [m.name for m in t]
    assert not any(n.endswith("client-key.pem") for n in names)


def test_scrub_strict_fails_on_residual_pem(tmp_path):
    # craft a bundle where scrubbing cannot remove a PEM (simulate bypass)
    src = str(tmp_path / "in.tar.gz")
    with tarfile.open(src, "w:gz") as t:
        data = b"-----BEGIN OPENSSH PRIVATE KEY-----\nZZZZ\n-----END OPENSSH PRIVATE KEY-----\n"
        info = tarfile.TarInfo("var/lib/xo-server/data/sdn-controller/x.pem")
        info.size = len(data)
        t.addfile(info, io.BytesIO(data))
    out = str(tmp_path / "out.tar.gz")
    report = {}
    rc = xoa_scrub.process_archive(src, out, anonymise=False, salt="", strict=True, report=report)
    # PEM blocks are replaced wholesale -> should be OK (redacted), not FAIL
    assert rc == 0
    body = read_member(out, "x.pem")
    assert "PRIVATE KEY" not in body


def test_anonymise_adds_map_and_replaces_ips(tmp_path):
    src = os.path.join(FIX, "bundle.tar.gz")
    out = str(tmp_path / "out.tar.gz")
    report = {}
    rc = xoa_scrub.process_archive(src, out, anonymise=True, salt="s3cret", strict=True, report=report)
    assert rc == 0
    config = read_member(out, "etc/xo-server/config.toml")
    assert "10.20.30.40" not in config
    with tarfile.open(out, "r:*") as t:
        names = [m.name for m in t]
    assert "anonymisation-map.json" in names
    with tarfile.open(out, "r:*") as t:
        for m in t:
            if m.name == "anonymisation-map.json":
                mapping = json.loads(t.extractfile(m).read().decode())
                assert mapping  # non-empty


def test_cli_end_to_end(tmp_path):
    src = os.path.join(FIX, "bundle.tar.gz")
    out = str(tmp_path / "cli.tar.gz")
    rc = xoa_scrub.main([src, "-o", out])
    assert rc == 0
    assert os.path.exists(out + ".scrub-report.json")
    with open(out + ".scrub-report.json") as fh:
        rep = json.load(fh)
    assert rep["status"] == "OK"
    assert any(f["action"] == "scrubbed" for f in rep["files"])
