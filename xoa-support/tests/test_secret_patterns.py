"""Tests for the shared redaction patterns — the security core."""
import re

from xoa_support import secret_patterns as sp


def test_password_kv_redacted():
    out = sp.scrub_text('password = "hunter2"')
    assert "hunter2" not in out
    assert "password" in out and sp.REDACTED in out


def test_toml_s3_secret_redacted():
    out = sp.scrub_text('secretAccessKey = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"')
    assert "wJalrXUtnFEMI" not in out
    assert sp.REDACTED in out


def test_redis_uri_password_redacted():
    out = sp.scrub_text("uri = 'redis://:Sup3rSecret@10.20.30.40:6379/0'")
    assert "Sup3rSecret" not in out
    assert "10.20.30.40" in out  # host kept, password gone


def test_url_credentials_redacted():
    out = sp.scrub_text("endpoint = https://user:p4ssw0rd@s3.example.invalid/bucket")
    assert "p4ssw0rd" not in out


def test_pem_private_key_block_removed():
    pem = (
        "-----BEGIN RSA PRIVATE KEY-----\nAAAAfake\n-----END RSA PRIVATE KEY-----\n"
    )
    out = sp.scrub_text(pem)
    assert "BEGIN RSA PRIVATE KEY" not in out
    assert "AAAAfake" not in out


def test_authorization_header_redacted():
    out = sp.scrub_text("Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.abcdef.ghi123")
    assert "eyJhbGciOiJIUzI1NiJ9" not in out


def test_public_certificate_kept():
    cert = "-----BEGIN CERTIFICATE-----\nFAKEPUBLIC\n-----END CERTIFICATE-----\n"
    assert sp.scrub_text(cert) == cert


def test_normal_text_untouched():
    text = "xo-server started on 10.0.0.1, all good"
    assert sp.scrub_text(text) == text


def test_sensitive_filename_detection():
    assert any(rx.search("client-key.pem") for rx in sp.SENSITIVE_FILENAME_RES)
    assert any(rx.search("id_rsa") for rx in sp.SENSITIVE_FILENAME_RES)
    assert not any(rx.search("client-cert.pem") for rx in sp.SENSITIVE_FILENAME_RES)


def test_all_patterns_compile():
    for name, pattern, replacer in sp.SECRET_PATTERNS:
        assert isinstance(pattern, re.Pattern), name
        assert callable(replacer), name
        assert sp.sos_subst(name)


def test_json_quoted_key_redacted():
    out = sp.scrub_text('{"password": "should-be-redacted", "user": "bob"}')
    assert "should-be-redacted" not in out
    assert '"user": "bob"' in out  # non-secret pair untouched
