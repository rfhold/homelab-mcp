import argparse
import json
import os
import ssl
import subprocess
import tempfile
import urllib.request
from pathlib import Path

from deploys.lib.ssh_credentials import MAX_CA_BYTES, validate_ed25519_public_key


DEFAULT_CA_URL = "https://openbao.holdenitdown.net/v1/homelab-ssh-client/public_key"
ROOT = Path(__file__).parents[2]


class _RejectRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError("CA endpoint redirects are not allowed")


def _system_tls_context() -> ssl.SSLContext:
    paths = ssl.get_default_verify_paths()
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    if not paths.openssl_cafile and not paths.openssl_capath:
        raise RuntimeError("system CA trust paths are unavailable")
    context.load_verify_locations(cafile=paths.openssl_cafile, capath=paths.openssl_capath)
    return context


def download_ca(url: str, timeout: int = 15) -> str:
    if not url.startswith("https://"):
        raise ValueError("CA URL must use HTTPS")
    opener = urllib.request.build_opener(
        urllib.request.ProxyHandler({}),
        urllib.request.HTTPSHandler(context=_system_tls_context()),
        _RejectRedirects,
    )
    request = urllib.request.Request(url, headers={"Accept": "text/plain"})
    with opener.open(request, timeout=timeout) as response:
        value = response.read(MAX_CA_BYTES + 1)
    return validate_ed25519_public_key(value)


def build_inventory(args: argparse.Namespace, ca_path: Path) -> dict:
    return {
        "version": 1,
        "host": {
            "address": args.target,
            "user": args.admin_user,
            "port": args.port,
            "known_hosts": str(args.known_hosts.resolve()),
        },
        "ca_public_key_file": str(ca_path),
        "separate_sudo_password": args.separate_sudo_password,
    }


def run(args: argparse.Namespace) -> int:
    if not args.known_hosts.is_file():
        raise ValueError("known-hosts must name an existing file")
    ca_public_key = download_ca(args.ca_url)
    with tempfile.TemporaryDirectory(prefix="homelab-bootstrap-") as directory:
        ca_path = Path(directory) / "user-ca.pub"
        ca_path.write_text(ca_public_key, encoding="ascii")
        ca_path.chmod(0o600)
        inventory = json.dumps(build_inventory(args, ca_path), separators=(",", ":"))
        env = os.environ.copy()
        env["HOMELAB_INVENTORY_JSON"] = inventory
        command = [
            "uv",
            "run",
            "--locked",
            "pyinfra",
            "--yes",
            str(ROOT / "deploys/inventory_bootstrap.py"),
            str(ROOT / "deploys/entrypoints/bootstrap_homelab.py"),
        ]
        return subprocess.run(command, cwd=ROOT, env=env, check=False).returncode


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(
        description="Bootstrap one homelab host with agent-first SSH and advertised password fallback"
    )
    result.add_argument("target")
    result.add_argument("admin_user")
    result.add_argument("--port", type=int, default=22)
    result.add_argument("--known-hosts", type=Path, default=Path.home() / ".ssh/known_hosts")
    result.add_argument("--ca-url", default=DEFAULT_CA_URL)
    result.add_argument("--separate-sudo-password", action="store_true")
    return result


def main() -> int:
    return run(parser().parse_args())


if __name__ == "__main__":
    raise SystemExit(main())
