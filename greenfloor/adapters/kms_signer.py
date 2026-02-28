"""AWS KMS P-256 (secp256r1) signing adapter for vault custody keys."""

from __future__ import annotations

import hashlib
import logging
from functools import lru_cache
from typing import Any

logger = logging.getLogger(__name__)


def _kms_client(region: str) -> Any:
    import boto3

    return boto3.client("kms", region_name=region)


@lru_cache(maxsize=4)
def get_public_key_compressed_hex(key_id: str, region: str) -> str:
    """Fetch a P-256 public key from KMS and return it as compressed hex (33 bytes).

    Port of ent-wallet's ``derToCompressedSecp256r1PublicKeyHex``.
    """
    client = _kms_client(region)
    resp = client.get_public_key(KeyId=key_id)
    der_bytes: bytes = resp["PublicKey"]

    x, y = _extract_p256_xy_from_spki(der_bytes)
    prefix = b"\x02" if y[-1] % 2 == 0 else b"\x03"
    compressed = prefix + x
    if len(compressed) != 33:
        raise ValueError(f"unexpected compressed key length: {len(compressed)}")
    return compressed.hex()


def sign_digest(key_id: str, region: str, message_hex: str) -> str:
    """Sign a vault message with KMS and return compact (r||s) hex.

    Matches the ent-wallet hotwallet flow:
    ``sha256(message_bytes)`` -> KMS ``Sign(ECDSA_SHA_256, DIGEST)`` -> DER -> compact.
    """
    message_bytes = bytes.fromhex(message_hex)
    digest = hashlib.sha256(message_bytes).digest()

    client = _kms_client(region)
    resp = client.sign(
        KeyId=key_id,
        Message=digest,
        MessageType="DIGEST",
        SigningAlgorithm="ECDSA_SHA_256",
    )
    der_sig: bytes = resp["Signature"]
    r_bytes, s_bytes = _parse_der_ecdsa_signature(der_sig)

    r_padded = r_bytes.rjust(32, b"\x00")
    s_padded = s_bytes.rjust(32, b"\x00")
    compact = r_padded + s_padded
    if len(compact) != 64:
        raise ValueError(f"unexpected compact signature length: {len(compact)}")
    return compact.hex()


# ---------------------------------------------------------------------------
# DER / ASN.1 helpers
# ---------------------------------------------------------------------------


def _extract_p256_xy_from_spki(der: bytes) -> tuple[bytes, bytes]:
    """Extract (x, y) 32-byte coordinates from a SubjectPublicKeyInfo DER blob.

    The uncompressed point encoding is 0x04 || x (32 bytes) || y (32 bytes),
    embedded as a BIT STRING inside the SPKI SEQUENCE.
    """
    # Walk the outer SEQUENCE
    idx, _ = _read_der_tag_length(der, 0)
    # Skip the AlgorithmIdentifier SEQUENCE
    idx, algo_len = _read_der_tag_length(der, idx)
    idx += algo_len
    # Read the BIT STRING containing the public key
    if der[idx] != 0x03:
        raise ValueError("expected BIT STRING tag (0x03)")
    idx, bs_len = _read_der_tag_length(der, idx)
    # First byte of BIT STRING is the unused-bits count (should be 0)
    if der[idx] != 0x00:
        raise ValueError(f"unexpected unused-bits byte: {der[idx]:#x}")
    point = der[idx + 1 : idx + bs_len]
    if len(point) != 65 or point[0] != 0x04:
        raise ValueError(
            f"expected 65-byte uncompressed point (0x04||x||y), got {len(point)} bytes"
        )
    x = point[1:33]
    y = point[33:65]
    return x, y


def _parse_der_ecdsa_signature(der: bytes) -> tuple[bytes, bytes]:
    """Parse a DER-encoded ECDSA signature into (r, s) byte strings.

    ASN.1: SEQUENCE { INTEGER r, INTEGER s }
    """
    idx, _ = _read_der_tag_length(der, 0)
    r, idx = _read_der_integer(der, idx)
    s, _ = _read_der_integer(der, idx)
    return r, s


def _read_der_tag_length(data: bytes, offset: int) -> tuple[int, int]:
    """Read a DER tag + length and return (offset_after_length, content_length)."""
    offset += 1  # skip tag byte
    if data[offset] & 0x80 == 0:
        length = data[offset]
        return offset + 1, length
    num_len_bytes = data[offset] & 0x7F
    offset += 1
    length = int.from_bytes(data[offset : offset + num_len_bytes], "big")
    return offset + num_len_bytes, length


def _read_der_integer(data: bytes, offset: int) -> tuple[bytes, int]:
    """Read a DER INTEGER and return (unsigned big-endian bytes, next_offset)."""
    if data[offset] != 0x02:
        raise ValueError(f"expected INTEGER tag (0x02), got {data[offset]:#x}")
    offset, length = _read_der_tag_length(data, offset)
    raw = data[offset : offset + length]
    # Strip leading zero byte that DER uses for sign padding
    if len(raw) > 1 and raw[0] == 0x00:
        raw = raw[1:]
    return raw, offset + length
