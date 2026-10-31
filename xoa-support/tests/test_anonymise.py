"""Tests for deterministic, salted anonymisation."""
from xoa_support.anonymise import Anonymiser


def test_deterministic_same_input_same_output():
    a = Anonymiser(salt="s")
    out1 = a.anonymise_text("host xoa-prod-1.corp.lan at 10.20.30.40")
    b = Anonymiser(salt="s")
    out2 = b.anonymise_text("host xoa-prod-1.corp.lan at 10.20.30.40")
    assert out1 == out2
    assert "xoa-prod-1.corp.lan" not in out1
    assert "10.20.30.40" not in out1


def test_different_salt_different_output():
    a = Anonymiser(salt="one").anonymise_text("10.20.30.40")
    b = Anonymiser(salt="two").anonymise_text("10.20.30.40")
    assert a != b


def test_uuid_and_email():
    a = Anonymiser()
    out = a.anonymise_text(
        "run 6e81f99c-f05c-54c1-1458-3275ba6803dc mail admin@customer-example.invalid"
    )
    assert "6e81f99c-f05c-54c1-1458-3275ba6803dc" not in out
    assert "admin@customer-example.invalid" not in out
    assert "@example.invalid" in out


def test_placeholder_domains_untouched():
    a = Anonymiser()
    text = "see docs at docs.example.invalid and localhost"
    assert a.anonymise_text(text) == text


def test_mapping_json_stable():
    a = Anonymiser(salt="x")
    a.anonymise_text("10.1.2.3")
    j1 = a.mapping_json()
    b = Anonymiser(salt="x")
    b.anonymise_text("10.1.2.3")
    assert j1 == b.mapping_json()
