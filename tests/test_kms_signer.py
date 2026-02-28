"""Deterministic tests for greenfloor.adapters.kms_signer (mocked boto3)."""

from __future__ import annotations

import hashlib

import pytest

from greenfloor.adapters.kms_signer import (
    _extract_p256_xy_from_spki,
    _parse_der_ecdsa_signature,
    _read_der_integer,
    get_public_key_compressed_hex,
    sign_digest,
)

# ---------------------------------------------------------------------------
# Test vectors
# ---------------------------------------------------------------------------

# A well-known NIST P-256 test point (uncompressed, 65 bytes).
_TEST_X = bytes.fromhex("6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296")
_TEST_Y = bytes.fromhex("4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5")
_TEST_UNCOMPRESSED_POINT = b"\x04" + _TEST_X + _TEST_Y

# Build a minimal SubjectPublicKeyInfo DER blob for the test point.
_ALGO_ID_DER = bytes.fromhex("301306072a8648ce3d020106082a8648ce3d030107")
_BIT_STRING_CONTENT = b"\x00" + _TEST_UNCOMPRESSED_POINT  # unused-bits byte + point
_BIT_STRING_DER = b"\x03" + bytes([len(_BIT_STRING_CONTENT)]) + _BIT_STRING_CONTENT
_SPKI_INNER = _ALGO_ID_DER + _BIT_STRING_DER
_TEST_SPKI_DER = b"\x30" + bytes([len(_SPKI_INNER)]) + _SPKI_INNER

# The expected compressed form: Y is odd (last byte 0xf5 -> odd), so prefix 0x03.
_TEST_COMPRESSED_HEX = "03" + _TEST_X.hex()

# A DER-encoded ECDSA signature with known r and s.
_TEST_R = (12345678901234567890).to_bytes(32, "big")
_TEST_S = (98765432109876543210).to_bytes(32, "big")


def _build_der_integer(value_bytes: bytes) -> bytes:
    """Encode an unsigned integer as a DER INTEGER (tag 0x02)."""
    stripped = value_bytes.lstrip(b"\x00") or b"\x00"
    if stripped[0] & 0x80:
        stripped = b"\x00" + stripped
    return b"\x02" + bytes([len(stripped)]) + stripped


def _build_der_ecdsa_signature(r_bytes: bytes, s_bytes: bytes) -> bytes:
    inner = _build_der_integer(r_bytes) + _build_der_integer(s_bytes)
    return b"\x30" + bytes([len(inner)]) + inner


_TEST_SIG_DER = _build_der_ecdsa_signature(_TEST_R, _TEST_S)


# ---------------------------------------------------------------------------
# Unit tests for DER helpers
# ---------------------------------------------------------------------------


class TestExtractP256XY:
    def test_valid_spki_returns_coordinates(self) -> None:
        x, y = _extract_p256_xy_from_spki(_TEST_SPKI_DER)
        assert x == _TEST_X
        assert y == _TEST_Y

    def test_invalid_bitstring_tag_raises(self) -> None:
        bad = bytearray(_TEST_SPKI_DER)
        # BIT STRING tag sits right after the outer SEQUENCE header (2 bytes)
        # plus the full AlgorithmIdentifier DER (len(_ALGO_ID_DER) = 21 bytes).
        bitstring_offset = 2 + len(_ALGO_ID_DER)
        assert bad[bitstring_offset] == 0x03  # sanity: is the BIT STRING tag
        bad[bitstring_offset] = 0x04
        with pytest.raises(ValueError, match="BIT STRING"):
            _extract_p256_xy_from_spki(bytes(bad))

    def test_non_uncompressed_point_raises(self) -> None:
        short_point = b"\x02" + _TEST_X  # compressed, not uncompressed
        bs_content = b"\x00" + short_point
        bs_der = b"\x03" + bytes([len(bs_content)]) + bs_content
        inner = _ALGO_ID_DER + bs_der
        spki = b"\x30" + bytes([len(inner)]) + inner
        with pytest.raises(ValueError, match="uncompressed"):
            _extract_p256_xy_from_spki(spki)


class TestParseDerEcdsaSignature:
    def test_roundtrip(self) -> None:
        r, s = _parse_der_ecdsa_signature(_TEST_SIG_DER)
        assert int.from_bytes(r, "big") == int.from_bytes(_TEST_R.lstrip(b"\x00") or b"\x00", "big")
        assert int.from_bytes(s, "big") == int.from_bytes(_TEST_S.lstrip(b"\x00") or b"\x00", "big")

    def test_leading_zero_stripped(self) -> None:
        # 0x80 high bit forces DER to add a leading 0x00
        r_with_high_bit = b"\x80" + b"\x01" * 31
        s_normal = b"\x01" * 32
        der = _build_der_ecdsa_signature(r_with_high_bit, s_normal)
        r, s = _parse_der_ecdsa_signature(der)
        assert r == r_with_high_bit
        assert s == s_normal

    def test_invalid_tag_raises(self) -> None:
        bad = b"\x30\x04\x05\x00\x05\x00"  # NULL tags instead of INTEGER
        with pytest.raises(ValueError, match="INTEGER"):
            _parse_der_ecdsa_signature(bad)


class TestReadDerInteger:
    def test_single_byte(self) -> None:
        der = b"\x02\x01\x42"
        val, next_offset = _read_der_integer(der, 0)
        assert val == b"\x42"
        assert next_offset == 3


# ---------------------------------------------------------------------------
# Integration tests with mocked boto3
# ---------------------------------------------------------------------------


class TestGetPublicKeyCompressedHex:
    def test_returns_compressed_hex(self, monkeypatch) -> None:
        get_public_key_compressed_hex.cache_clear()

        class FakeKmsClient:
            def get_public_key(self, KeyId):
                return {"PublicKey": _TEST_SPKI_DER}

        monkeypatch.setattr(
            "greenfloor.adapters.kms_signer._kms_client",
            lambda region: FakeKmsClient(),
        )
        result = get_public_key_compressed_hex("arn:fake:key/123", "us-west-2")
        assert result == _TEST_COMPRESSED_HEX
        get_public_key_compressed_hex.cache_clear()

    def test_even_y_gets_prefix_02(self, monkeypatch) -> None:
        get_public_key_compressed_hex.cache_clear()
        even_y = bytes(32)  # all zeros -> even
        point = b"\x04" + _TEST_X + even_y
        bs_content = b"\x00" + point
        bs_der = b"\x03" + bytes([len(bs_content)]) + bs_content
        inner = _ALGO_ID_DER + bs_der
        spki = b"\x30" + bytes([len(inner)]) + inner

        class FakeKmsClient:
            def get_public_key(self, KeyId):
                return {"PublicKey": spki}

        monkeypatch.setattr(
            "greenfloor.adapters.kms_signer._kms_client",
            lambda region: FakeKmsClient(),
        )
        result = get_public_key_compressed_hex("arn:fake:key/456", "us-west-2")
        assert result.startswith("02")
        get_public_key_compressed_hex.cache_clear()


class TestSignDigest:
    def test_signs_and_returns_compact(self, monkeypatch) -> None:
        get_public_key_compressed_hex.cache_clear()
        test_message_hex = "deadbeef" * 8
        expected_digest = hashlib.sha256(bytes.fromhex(test_message_hex)).digest()

        captured_digest: list[bytes] = []

        class FakeKmsClient:
            def sign(self, KeyId, Message, MessageType, SigningAlgorithm):
                captured_digest.append(Message)
                assert MessageType == "DIGEST"
                assert SigningAlgorithm == "ECDSA_SHA_256"
                return {"Signature": _TEST_SIG_DER}

        monkeypatch.setattr(
            "greenfloor.adapters.kms_signer._kms_client",
            lambda region: FakeKmsClient(),
        )
        result = sign_digest("arn:fake:key/789", "us-west-2", test_message_hex)

        # Verify the digest passed to KMS was SHA-256 of the message bytes
        assert captured_digest[0] == expected_digest

        # Verify compact format: r (32 bytes) || s (32 bytes) = 64 bytes = 128 hex chars
        assert len(result) == 128
        r_hex = result[:64]
        s_hex = result[64:]
        assert int(r_hex, 16) == int.from_bytes(_TEST_R.lstrip(b"\x00") or b"\x00", "big")
        assert int(s_hex, 16) == int.from_bytes(_TEST_S.lstrip(b"\x00") or b"\x00", "big")
