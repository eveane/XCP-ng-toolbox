"""Tests for bin/xoa_bundle_inspect.py (consumer PoC)."""
import json
import os

import xoa_bundle_inspect as insp

FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")


def test_summarize_tar_bundle():
    s = insp.summarise(os.path.join(FIX, "bundle.tar.gz"))
    assert s["files_total"] > 0
    assert s["config_toml_present"]
    assert any("leveldb" in f for f in s["leveldb_files"])
    assert s["rpu_traces"]["ndjson"] >= 1
    assert s["rpu_traces"]["heartbeat"] >= 1
    assert s["redis_log_lines"] >= 3
    assert "registration.json" in s["updater_files"]
    assert "client-key.pem" in s["sdn_controller_files"]
    assert any("PRIVATE KEY" in w for w in s["warnings"])


def test_summarize_directory():
    s = insp.summarise(os.path.join(FIX, "bundle"))
    assert s["config_toml_present"]


def test_json_output_is_valid(capsys):
    rc = insp.main([os.path.join(FIX, "bundle.tar.gz"), "--json"])
    out = capsys.readouterr().out
    data = json.loads(out)
    assert data["files_total"] > 0
    assert rc == 1  # because fixture contains a private key -> warning
