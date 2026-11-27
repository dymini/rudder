#!/usr/bin/env python3
"""Verify a minisign signature with only the Python standard library.

Usage: minisign-verify.py PUBLIC_KEY_BASE64 MESSAGE_FILE SIGNATURE_FILE
Only modern prehashed minisign signatures (algorithm "ED") are accepted.
"""
import base64
import hashlib
import sys

Q = 2**255 - 19
L = 2**252 + 27742317777372353535851937790883648493
D = (-121665 * pow(121666, Q - 2, Q)) % Q
I = pow(2, (Q - 1) // 4, Q)


def inv(x):
    return pow(x, Q - 2, Q)


def xrecover(y):
    xx = (y * y - 1) * inv(D * y * y + 1) % Q
    x = pow(xx, (Q + 3) // 8, Q)
    if (x * x - xx) % Q:
        x = x * I % Q
    if x % 2:
        x = Q - x
    return x


BY = 4 * inv(5) % Q
B = (xrecover(BY), BY)


def edwards_add(p, q):
    x1, y1 = p
    x2, y2 = q
    common = D * x1 * x2 * y1 * y2 % Q
    x3 = (x1 * y2 + x2 * y1) * inv(1 + common) % Q
    y3 = (y1 * y2 + x1 * x2) * inv(1 - common) % Q
    return x3, y3


def scalarmult(point, scalar):
    result = (0, 1)
    addend = point
    while scalar:
        if scalar & 1:
            result = edwards_add(result, addend)
        addend = edwards_add(addend, addend)
        scalar >>= 1
    return result


def decode_point(encoded):
    if len(encoded) != 32:
        raise ValueError("bad point length")
    y = int.from_bytes(encoded, "little") & ((1 << 255) - 1)
    sign = encoded[31] >> 7
    x = xrecover(y)
    if x % 2 != sign:
        x = Q - x
    if (-x * x + y * y - 1 - D * x * x * y * y) % Q:
        raise ValueError("point is not on the curve")
    return x, y


def ed25519_verify(signature, message, public_key):
    if len(signature) != 64 or len(public_key) != 32:
        return False
    r_encoded = signature[:32]
    s = int.from_bytes(signature[32:], "little")
    if s >= L:
        return False
    try:
        r = decode_point(r_encoded)
        a = decode_point(public_key)
    except ValueError:
        return False
    digest = hashlib.sha512(r_encoded + public_key + message).digest()
    h = int.from_bytes(digest, "little")
    return scalarmult(B, s) == edwards_add(r, scalarmult(a, h))


def parse_public_key(text):
    rows = [line.strip() for line in text.splitlines() if line.strip()]
    encoded = rows[1] if len(rows) > 1 else rows[0]
    raw = base64.b64decode(encoded, validate=True)
    if len(raw) != 42 or raw[:2] not in (b"Ed", b"ED"):
        raise ValueError("unsupported minisign public key")
    return raw[2:10], raw[10:]


def parse_signature(text):
    rows = text.splitlines()
    if len(rows) < 4 or not rows[2].startswith("trusted comment: "):
        raise ValueError("malformed minisign signature")
    first = base64.b64decode(rows[1].strip(), validate=True)
    if len(first) != 74 or first[:2] != b"ED":
        raise ValueError("only prehashed minisign signatures are supported")
    global_sig = base64.b64decode(rows[3].strip(), validate=True)
    if len(global_sig) != 64:
        raise ValueError("malformed minisign global signature")
    return first[2:10], first[10:], rows[2][17:].encode(), global_sig


def main():
    if len(sys.argv) != 4:
        return 2
    try:
        key_id, key = parse_public_key(open(sys.argv[1], encoding="utf-8").read())
        message = open(sys.argv[2], "rb").read()
        sig_key_id, sig, trusted_comment, global_sig = parse_signature(
            open(sys.argv[3], encoding="utf-8").read()
        )
        if key_id != sig_key_id:
            return 1
        digest = hashlib.blake2b(message).digest()
        if not ed25519_verify(sig, digest, key):
            return 1
        if not ed25519_verify(global_sig, sig + trusted_comment, key):
            return 1
    except (OSError, ValueError, base64.binascii.Error):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
