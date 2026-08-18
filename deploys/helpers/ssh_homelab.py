import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from enum import Enum, auto
from pathlib import Path
from urllib.parse import urlsplit

from deploys.lib.ssh_credentials import validate_user_certificate
from deploys.lib.token_cache import TokenCache


DEFAULT_OPENBAO_URL = "https://openbao.holdenitdown.net"
EXPECTED_TOKEN_POLICY = "homelab-ssh-client-sign"
MAX_TOKEN_TTL_SECONDS = 8 * 60 * 60
MAX_BAO_RESPONSE_BYTES = 65_536
_BAO_ENV_ALLOWLIST = {
    "HOME",
    "LANG",
    "LC_ALL",
    "PATH",
}
_OIDC_ENV_ALLOWLIST = _BAO_ENV_ALLOWLIST | {
    "BROWSER",
    "DBUS_SESSION_BUS_ADDRESS",
    "DISPLAY",
    "TERM",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
}


def _executable(name: str) -> str:
    value = shutil.which(name)
    if value is None:
        raise ValueError(f"required executable is unavailable: {name}")
    return value


def validate_openbao_url(value: str) -> str:
    try:
        parsed = urlsplit(value)
        port = parsed.port
    except ValueError as error:
        raise ValueError("OpenBao URL must be a canonical HTTPS origin") from error
    if (
        parsed.scheme != "https"
        or not parsed.hostname
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
        or parsed.path not in {"", "/"}
    ):
        raise ValueError("OpenBao URL must be an HTTPS root origin without credentials, query, or fragment")
    if parsed.hostname != parsed.hostname.lower() or parsed.hostname.endswith("."):
        raise ValueError("OpenBao URL host must use canonical lowercase form")
    host = f"[{parsed.hostname}]" if ":" in parsed.hostname else parsed.hostname
    if port is not None:
        if port == 0:
            raise ValueError("OpenBao URL port must be from 1 through 65535")
        if port == 443:
            raise ValueError("OpenBao URL must omit the default HTTPS port")
        host = f"{host}:{port}"
    origin = f"https://{host}"
    if value not in {origin, origin + "/"}:
        raise ValueError("OpenBao URL must be a canonical HTTPS origin")
    return origin


def _bao_environment(openbao_url: str, oidc: bool = False) -> dict[str, str]:
    allowlist = _OIDC_ENV_ALLOWLIST if oidc else _BAO_ENV_ALLOWLIST
    env = {key: value for key, value in os.environ.items() if key in allowlist}
    env["BAO_ADDR"] = openbao_url
    env["BAO_DISABLE_REDIRECTS"] = "true"
    return env


def _login(bao: str, openbao_url: str) -> str:
    env = _bao_environment(openbao_url, oidc=True)
    result = subprocess.run(
        [bao, "login", "-method=oidc", "-token-only", "role=ssh"],
        env=env,
        stdout=subprocess.PIPE,
        check=False,
        timeout=300,
    )
    if result.returncode != 0:
        raise RuntimeError("OpenBao OIDC login failed")
    if len(result.stdout) > MAX_BAO_RESPONSE_BYTES:
        raise RuntimeError("OpenBao OIDC login response exceeded its bound")
    try:
        token = result.stdout.decode("utf-8").strip()
    except UnicodeDecodeError as error:
        raise RuntimeError("OpenBao OIDC login returned an invalid response") from error
    if not token or any(character.isspace() for character in token):
        raise RuntimeError("OpenBao OIDC login returned no token")
    return token


def _token_environment(openbao_url: str, token: str) -> dict[str, str]:
    env = _bao_environment(openbao_url)
    env["BAO_TOKEN"] = token
    return env


class _TokenLookup(Enum):
    VALID = auto()
    REJECTED = auto()
    CONTRACT_INVALID = auto()
    INDETERMINATE = auto()


_AUTH_REJECTION_MARKERS = (
    b"permission denied",
    b"invalid token",
    b"token is expired",
    b"expired token",
    b"bad token",
    b"missing client token",
    b"code\":403",
    b"code: 403",
)


def _is_authentication_rejection(diagnostic: bytes) -> bool:
    lowered = diagnostic[:MAX_BAO_RESPONSE_BYTES].lower()
    return any(marker in lowered for marker in _AUTH_REJECTION_MARKERS)


def _lookup_token(bao: str, token: str, openbao_url: str) -> _TokenLookup:
    env = _token_environment(openbao_url, token)
    try:
        try:
            result = subprocess.run(
                [bao, "token", "lookup", "-format=json"],
                env=env,
                capture_output=True,
                check=False,
                timeout=15,
            )
        except (OSError, subprocess.SubprocessError):
            return _TokenLookup.INDETERMINATE
    finally:
        env.pop("BAO_TOKEN", None)
    if len(result.stdout) > MAX_BAO_RESPONSE_BYTES or len(result.stderr) > MAX_BAO_RESPONSE_BYTES:
        return _TokenLookup.INDETERMINATE
    if result.returncode != 0:
        if _is_authentication_rejection(result.stderr):
            return _TokenLookup.REJECTED
        return _TokenLookup.INDETERMINATE
    try:
        response = json.loads(result.stdout)
    except (UnicodeDecodeError, json.JSONDecodeError):
        return _TokenLookup.INDETERMINATE
    if not isinstance(response, dict) or not isinstance(response.get("data"), dict):
        return _TokenLookup.INDETERMINATE
    data = response["data"]
    ttl = data.get("ttl")
    policies = data.get("policies")
    metadata = data.get("meta")
    if (
        isinstance(ttl, bool)
        or not isinstance(ttl, int)
        or not 0 < ttl <= MAX_TOKEN_TTL_SECONDS
        or policies != [EXPECTED_TOKEN_POLICY]
        or not isinstance(metadata, dict)
        or metadata.get("role") != "ssh"
        or data.get("type") != "service"
    ):
        return _TokenLookup.CONTRACT_INVALID
    if "token_policies" in data and data["token_policies"] != [EXPECTED_TOKEN_POLICY]:
        return _TokenLookup.CONTRACT_INVALID
    if "identity_policies" in data and data["identity_policies"] not in (None, []):
        return _TokenLookup.CONTRACT_INVALID
    if "path" in data and (
        not isinstance(data["path"], str) or not data["path"].startswith("auth/oidc/")
    ):
        return _TokenLookup.CONTRACT_INVALID
    return _TokenLookup.VALID


def _revoke_token(bao: str, token: str, openbao_url: str) -> bool:
    env = _token_environment(openbao_url, token)
    try:
        try:
            result = subprocess.run(
                [bao, "token", "revoke", "-self"],
                env=env,
                capture_output=True,
                check=False,
                timeout=15,
            )
        except (OSError, subprocess.SubprocessError):
            return False
    finally:
        env.pop("BAO_TOKEN", None)
    return (
        result.returncode == 0
        and len(result.stdout) <= MAX_BAO_RESPONSE_BYTES
        and len(result.stderr) <= MAX_BAO_RESPONSE_BYTES
    )


def _login_validate_and_save(bao: str, openbao_url: str, cache: TokenCache) -> str:
    token = _login(bao, openbao_url)
    lookup = _lookup_token(bao, token, openbao_url)
    if lookup is not _TokenLookup.VALID:
        if not _revoke_token(bao, token, openbao_url):
            print(
                "warning: the new OpenBao token could not be confirmed revoked; it was not cached "
                "but may remain valid until its server-side expiry",
                file=sys.stderr,
            )
        if lookup in {_TokenLookup.REJECTED, _TokenLookup.CONTRACT_INVALID}:
            raise RuntimeError("OpenBao OIDC token did not satisfy the SSH signing token contract")
        raise RuntimeError(
            "OpenBao OIDC token validation was indeterminate; the token was not cached; retry login"
        )
    cache.save(openbao_url, token)
    return token


class _AuthenticationRejected(RuntimeError):
    pass


def _sign(bao: str, public_key: Path, certificate: Path, token: str, openbao_url: str) -> None:
    env = _token_environment(openbao_url, token)
    try:
        result = subprocess.run(
            [
                bao,
                "write",
                "-field=signed_key",
                "homelab-ssh-client/sign/homelab",
                f"public_key=@{public_key}",
                "cert_type=user",
                "valid_principals=homelab",
                "ttl=15m",
            ],
            env=env,
            capture_output=True,
            check=False,
            timeout=30,
        )
    finally:
        env.pop("BAO_TOKEN", None)
    if result.returncode != 0:
        if len(result.stdout) > MAX_BAO_RESPONSE_BYTES or len(result.stderr) > MAX_BAO_RESPONSE_BYTES:
            raise RuntimeError("OpenBao SSH certificate request failed")
        if _is_authentication_rejection(result.stderr):
            raise _AuthenticationRejected("OpenBao token was rejected while signing")
        raise RuntimeError("OpenBao SSH certificate request failed")
    if not result.stdout or len(result.stdout) > MAX_BAO_RESPONSE_BYTES:
        raise RuntimeError("OpenBao SSH certificate response is empty or too large")
    certificate.write_bytes(result.stdout)
    certificate.chmod(0o600)


def build_ssh_command(args: argparse.Namespace, ssh: str, key: Path, certificate: Path) -> list[str]:
    command = [
        ssh,
        "-F",
        "none",
        "-i",
        str(key),
        "-o",
        f"CertificateFile={certificate}",
        "-o",
        "IdentitiesOnly=yes",
        "-o",
        "BatchMode=yes",
        "-o",
        "PreferredAuthentications=publickey",
        "-o",
        f"UserKnownHostsFile={args.known_hosts.resolve()}",
        "-o",
        "StrictHostKeyChecking=yes",
        "-p",
        str(args.port),
        f"homelab@{args.target}",
    ]
    return command


def run(args: argparse.Namespace) -> int:
    openbao_url = validate_openbao_url(args.openbao_url)
    cache = TokenCache.from_environment()
    cached = cache.load(openbao_url)
    if getattr(args, "logout", False):
        if cached is None:
            return 0
        bao = _executable("bao")
        try:
            revoked = _revoke_token(bao, cached.token, openbao_url)
        finally:
            cache.clear()
        if not revoked:
            print(
                "warning: cached OpenBao token could not be revoked; the local cache was cleared "
                "but the token may remain valid until its server-side expiry",
                file=sys.stderr,
            )
        return 0
    if args.target is None:
        raise ValueError("target is required unless --logout is used")
    if not args.known_hosts.is_file():
        raise ValueError("known-hosts must name an existing file")
    bao = _executable("bao")
    if getattr(args, "reauth", False):
        if cached is not None:
            try:
                revoked = _revoke_token(bao, cached.token, openbao_url)
            finally:
                cache.clear()
            if not revoked:
                print(
                    "warning: cached OpenBao token could not be revoked; continuing with a new login "
                    "after clearing the local cache",
                    file=sys.stderr,
                )
        cached = None
    lookup = _lookup_token(bao, cached.token, openbao_url) if cached is not None else None
    if lookup is _TokenLookup.INDETERMINATE:
        raise RuntimeError(
            "cached OpenBao token validation was indeterminate; the cache was preserved; "
            "retry or use --reauth"
        )
    token_is_cached = lookup is _TokenLookup.VALID
    if lookup is _TokenLookup.CONTRACT_INVALID:
        revoked = _revoke_token(bao, cached.token, openbao_url)
        if not revoked:
            print(
                "warning: the contract-invalid cached OpenBao token could not be confirmed revoked; "
                "clearing it before login",
                file=sys.stderr,
            )
        cache.clear()
        cached = None
    elif lookup is _TokenLookup.REJECTED:
        cache.clear()
        cached = None
    token = cached.token if token_is_cached else _login_validate_and_save(bao, openbao_url, cache)
    ssh_keygen = _executable("ssh-keygen")
    ssh = _executable("ssh")
    with tempfile.TemporaryDirectory(prefix="ssh-homelab-") as directory:
        temp = Path(directory)
        key = temp / "id_ed25519"
        certificate = temp / "id_ed25519-cert.pub"
        generated = subprocess.run(
            [ssh_keygen, "-q", "-t", "ed25519", "-N", "", "-f", str(key)],
            capture_output=True,
            check=False,
            timeout=10,
        )
        if generated.returncode != 0:
            raise RuntimeError("temporary SSH key generation failed")
        try:
            try:
                _sign(bao, key.with_suffix(".pub"), certificate, token, openbao_url)
            except _AuthenticationRejected:
                if not token_is_cached:
                    revoked = _revoke_token(bao, token, openbao_url)
                    cache.clear()
                    if not revoked:
                        print(
                            "warning: the rejected OpenBao token could not be confirmed revoked; "
                            "the local cache was cleared",
                            file=sys.stderr,
                        )
                    raise RuntimeError("OpenBao SSH certificate request rejected the new OIDC token") from None
                revoked = _revoke_token(bao, token, openbao_url)
                if not revoked:
                    print(
                        "warning: the rejected cached OpenBao token could not be confirmed revoked; "
                        "clearing it before login",
                        file=sys.stderr,
                    )
                cache.clear()
                token = _login_validate_and_save(bao, openbao_url, cache)
                token_is_cached = False
                try:
                    _sign(bao, key.with_suffix(".pub"), certificate, token, openbao_url)
                except _AuthenticationRejected:
                    revoked = _revoke_token(bao, token, openbao_url)
                    cache.clear()
                    if not revoked:
                        print(
                            "warning: the rejected OpenBao token could not be confirmed revoked; "
                            "the local cache was cleared",
                            file=sys.stderr,
                        )
                    raise RuntimeError("OpenBao SSH certificate request rejected the new OIDC token") from None
        finally:
            token = ""
        inspected = subprocess.run(
            [ssh_keygen, "-L", "-f", str(certificate)],
            env={"LC_ALL": "C", "TZ": "UTC"},
            capture_output=True,
            text=True,
            check=False,
            timeout=10,
        )
        if inspected.returncode != 0:
            raise RuntimeError("OpenBao returned an invalid SSH certificate")
        if len(inspected.stdout.encode()) > 65_536:
            raise RuntimeError("SSH certificate inspection exceeded its bound")
        validate_user_certificate(inspected.stdout)
        return subprocess.run(build_ssh_command(args, ssh, key, certificate), check=False).returncode


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description="Open a short-lived OpenBao-certified SSH session")
    result.add_argument("target", nargs="?")
    result.add_argument("--port", type=int, default=22)
    result.add_argument("--known-hosts", type=Path, default=Path.home() / ".ssh/known_hosts")
    result.add_argument("--openbao-url", default=DEFAULT_OPENBAO_URL)
    action = result.add_mutually_exclusive_group()
    action.add_argument("--reauth", action="store_true", help="revoke the cached token and authenticate again")
    action.add_argument("--logout", action="store_true", help="revoke and remove the cached token without SSH")
    return result


def main() -> int:
    return run(parser().parse_args())


if __name__ == "__main__":
    raise SystemExit(main())
