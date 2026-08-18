import base64
import binascii
import re
from datetime import datetime, timedelta, timezone


MAX_CA_BYTES = 4096
_VALIDITY = re.compile(r"^\s*Valid: from (\S+) to (\S+)\s*$", re.MULTILINE)
_USER_CERTIFICATE = re.compile(r"^\s*Type: \S+ user certificate\s*$", re.MULTILINE)


def validate_ed25519_public_key(value: bytes) -> str:
    if not value or len(value) > MAX_CA_BYTES:
        raise ValueError("CA response is empty or too large")
    try:
        text = value.decode("ascii")
    except UnicodeDecodeError as error:
        raise ValueError("CA response is not ASCII") from error
    if len(text.splitlines()) != 1:
        raise ValueError("CA response must contain exactly one public key")
    fields = text.strip().split()
    if len(fields) not in {2, 3} or fields[0] != "ssh-ed25519":
        raise ValueError("CA response must contain exactly one Ed25519 public key")
    try:
        blob = base64.b64decode(fields[1], validate=True)
    except (binascii.Error, ValueError) as error:
        raise ValueError("CA public key has invalid base64") from error
    key_type = b"ssh-ed25519"
    expected_prefix = len(key_type).to_bytes(4, "big") + key_type + (32).to_bytes(4, "big")
    if len(blob) != len(expected_prefix) + 32 or not blob.startswith(expected_prefix):
        raise ValueError("CA public key has an invalid Ed25519 key blob")
    return text.strip() + "\n"


def validate_user_certificate(output: str, now: datetime | None = None) -> None:
    if not _USER_CERTIFICATE.search(output):
        raise ValueError("SSH certificate must be a user certificate")
    lines = output.splitlines()
    try:
        start = next(index for index, line in enumerate(lines) if line.strip() == "Principals:")
        heading_indent = len(lines[start]) - len(lines[start].lstrip())
        end = next(
            index
            for index in range(start + 1, len(lines))
            if lines[index].strip()
            and len(lines[index]) - len(lines[index].lstrip()) <= heading_indent
        )
    except (StopIteration, ValueError) as error:
        raise ValueError("SSH certificate has no bounded principals section") from error
    principals = [line.strip() for line in lines[start + 1 : end] if line.strip()]
    if principals != ["homelab"]:
        raise ValueError("SSH certificate principal must be homelab")
    match = _VALIDITY.search(output)
    if not match:
        raise ValueError("SSH certificate has no bounded validity")
    try:
        valid_after = datetime.strptime(match.group(1), "%Y-%m-%dT%H:%M:%S").replace(tzinfo=timezone.utc)
        valid_before = datetime.strptime(match.group(2), "%Y-%m-%dT%H:%M:%S").replace(tzinfo=timezone.utc)
    except ValueError as error:
        raise ValueError("SSH certificate validity is invalid") from error
    current = now or datetime.now(timezone.utc)
    if valid_after > current or valid_before <= current:
        raise ValueError("SSH certificate is not currently valid")
    if valid_before - current > timedelta(minutes=15):
        raise ValueError("SSH certificate has more than 15 minutes remaining")
    if valid_before - valid_after > timedelta(minutes=15, seconds=30):
        raise ValueError("SSH certificate validity exceeds the managed 15m30s interval")
