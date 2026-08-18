import argparse
import base64
import io
import json
import os
import stat
import struct
import subprocess
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

from deploys.helpers import bootstrap_homelab, ssh_homelab
from deploys.lib.ssh_credentials import validate_ed25519_public_key, validate_user_certificate
from deploys.lib.token_cache import MAX_CACHE_BYTES, TokenCache


def public_key() -> bytes:
    key_type = b"ssh-ed25519"
    blob = struct.pack(">I", len(key_type)) + key_type + struct.pack(">I", 32) + bytes(32)
    return b"ssh-ed25519 " + base64.b64encode(blob) + b" test-ca\n"


class _Response:
    def __init__(self, value: bytes):
        self.value = value

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return None

    def read(self, size: int) -> bytes:
        return self.value[:size]


class CredentialValidationTests(unittest.TestCase):
    def test_ca_requires_one_bounded_ed25519_key(self) -> None:
        self.assertEqual(validate_ed25519_public_key(public_key()), public_key().decode())
        for value in (b"", b"ssh-rsa AAAA\n", public_key() + public_key(), b"ssh-ed25519 !!!\n", b"x" * 4097):
            with self.subTest(value=value[:20]), self.assertRaises(ValueError):
                validate_ed25519_public_key(value)

    def test_certificate_requires_homelab_and_short_current_validity(self) -> None:
        now = datetime(2026, 8, 17, 12, 0, tzinfo=timezone.utc)
        valid = self._certificate(now - timedelta(seconds=30), now + timedelta(minutes=15))
        validate_user_certificate(valid, now)
        with self.assertRaises(ValueError):
            validate_user_certificate(valid.replace("                homelab", "                root"), now)
        with self.assertRaises(ValueError):
            validate_user_certificate(valid.replace("user certificate", "host certificate"), now)
        with self.assertRaises(ValueError):
            validate_user_certificate(self._certificate(now - timedelta(seconds=30), now + timedelta(minutes=15, seconds=1)), now)

    def test_certificate_validity_boundaries_are_exact(self) -> None:
        now = datetime(2026, 8, 17, 12, 0, tzinfo=timezone.utc)
        validate_user_certificate(self._certificate(now - timedelta(seconds=30), now + timedelta(minutes=15)), now)
        with self.assertRaises(ValueError):
            validate_user_certificate(self._certificate(now + timedelta(seconds=1), now + timedelta(minutes=15)), now)
        with self.assertRaises(ValueError):
            validate_user_certificate(self._certificate(now, now + timedelta(minutes=15, seconds=1)), now)
        with self.assertRaises(ValueError):
            validate_user_certificate(self._certificate(now - timedelta(seconds=31), now + timedelta(minutes=15)), now)
        with self.assertRaises(ValueError):
            validate_user_certificate(self._certificate(now - timedelta(minutes=15), now), now)

    @staticmethod
    def _certificate(start: datetime, end: datetime) -> str:
        return (
            "/tmp/id_ed25519-cert.pub:\n"
            "        Type: ssh-ed25519-cert-v01@openssh.com user certificate\n"
            "        Public key: ED25519-CERT SHA256:example\n"
            "        Signing CA: ED25519 SHA256:ca (using ssh-ed25519)\n"
            "        Key ID: \"example\"\n"
            "        Serial: 123\n"
            f"        Valid: from {start:%Y-%m-%dT%H:%M:%S} to {end:%Y-%m-%dT%H:%M:%S}\n"
            "        Principals:\n"
            "                homelab\n"
            "        Critical Options: (none)\n"
            "        Extensions: (none)\n"
        )


class TokenCacheTests(unittest.TestCase):
    def test_secure_cache_round_trip_has_owner_only_modes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cache = TokenCache(root)
            cache.save("https://openbao.example", "dummy-token")
            self.assertEqual(stat.S_IMODE(cache.directory.stat().st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(cache.path.stat().st_mode), 0o600)
            self.assertEqual(cache.load("https://openbao.example").token, "dummy-token")

    def test_runtime_root_must_be_absolute_secure_owned_and_not_a_symlink(self) -> None:
        with patch.dict(os.environ, {}, clear=True), self.assertRaises(ValueError):
            TokenCache.from_environment()
        with patch.dict(os.environ, {"XDG_RUNTIME_DIR": "relative"}, clear=True), self.assertRaises(ValueError):
            TokenCache.from_environment()
        with self.assertRaises(ValueError):
            TokenCache(Path("relative"))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            root.chmod(0o755)
            with self.assertRaises(ValueError):
                TokenCache(root)
            root.chmod(0o700)
            with self.assertRaises(ValueError):
                TokenCache(root, uid=os.getuid() + 1)
            link = root.parent / f"{root.name}-link"
            link.symlink_to(root, target_is_directory=True)
            try:
                with self.assertRaises(ValueError):
                    TokenCache(link)
            finally:
                link.unlink()

    def test_cache_rejects_symlinks_wrong_modes_and_wrong_owners(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cache = TokenCache(root)
            cache.directory.symlink_to(root, target_is_directory=True)
            with self.assertRaises(ValueError):
                cache.load("https://openbao.example")
            cache.directory.unlink()
            cache.directory.mkdir(mode=0o700)
            target = root / "target"
            target.write_text("{}")
            cache.path.symlink_to(target)
            with self.assertRaises(ValueError):
                cache.load("https://openbao.example")
            cache.path.unlink()
            cache.path.write_text("{}")
            cache.path.chmod(0o644)
            with self.assertRaises(ValueError):
                cache.load("https://openbao.example")
            cache.path.chmod(0o600)
            with patch("os.fstat", side_effect=lambda fd: SimpleNamespace(st_mode=stat.S_IFREG | 0o600, st_uid=os.getuid() + 1)):
                with self.assertRaises(ValueError):
                    cache.load("https://openbao.example")

    def test_malformed_oversized_and_wrong_origin_entries_are_cleared(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            cache = TokenCache(Path(directory))
            cache.directory.mkdir(mode=0o700)
            cache.path.write_bytes(b"x" * (MAX_CACHE_BYTES + 1))
            cache.path.chmod(0o600)
            self.assertIsNone(cache.load("https://openbao.example"))
            self.assertFalse(cache.path.exists())
            cache.save("https://first.example", "dummy-token")
            self.assertIsNone(cache.load("https://second.example"))
            self.assertFalse(cache.path.exists())


class BootstrapHelperTests(unittest.TestCase):
    def test_system_tls_context_ignores_ambient_ca_overrides(self) -> None:
        paths = SimpleNamespace(
            openssl_cafile="/system/ca-certificates.crt",
            openssl_capath="/system/certs",
            cafile="/hostile/ca.pem",
            capath="/hostile/certs",
        )
        context = Mock()
        with patch.dict(
            os.environ,
            {"SSL_CERT_FILE": "/hostile/ca.pem", "SSL_CERT_DIR": "/hostile/certs"},
        ), patch("ssl.get_default_verify_paths", return_value=paths), patch(
            "ssl.SSLContext", return_value=context
        ):
            self.assertIs(bootstrap_homelab._system_tls_context(), context)
        context.load_verify_locations.assert_called_once_with(
            cafile="/system/ca-certificates.crt",
            capath="/system/certs",
        )

    def test_download_rejects_non_https_redirects_size_and_bad_keys(self) -> None:
        with self.assertRaises(ValueError):
            bootstrap_homelab.download_ca("http://openbao.example/key")
        handler = bootstrap_homelab._RejectRedirects()
        with self.assertRaises(ValueError):
            handler.redirect_request(None, None, 302, "redirect", {}, "https://other.example/key")
        opener = Mock()
        opener.open.return_value = _Response(b"x" * 4097)
        tls_context = Mock()
        with patch.object(bootstrap_homelab, "_system_tls_context", return_value=tls_context), patch(
            "urllib.request.build_opener", return_value=opener
        ) as build_opener, self.assertRaises(ValueError):
            bootstrap_homelab.download_ca("https://openbao.example/key")
        proxy_handler, https_handler, _ = build_opener.call_args.args
        self.assertEqual(proxy_handler.proxies, {})
        self.assertIs(https_handler._context, tls_context)
        opener.open.return_value = _Response(public_key())
        with patch.object(bootstrap_homelab, "_system_tls_context", return_value=tls_context), patch(
            "urllib.request.build_opener", return_value=opener
        ):
            self.assertEqual(bootstrap_homelab.download_ca("https://openbao.example/key"), public_key().decode())

    def test_inventory_and_command_have_no_secret_and_temp_is_cleaned(self) -> None:
        with tempfile.NamedTemporaryFile() as known_hosts:
            args = argparse.Namespace(
                target="node.example",
                admin_user="admin",
                port=22,
                known_hosts=Path(known_hosts.name),
                ca_url="https://openbao.example/key",
                separate_sudo_password=False,
            )
            seen = {}

            def invoke(command, **kwargs):
                seen["command"] = command
                seen["inventory"] = kwargs["env"]["HOMELAB_INVENTORY_JSON"]
                seen["ca_path"] = json.loads(seen["inventory"])["ca_public_key_file"]
                return Mock(returncode=0)

            with patch.object(bootstrap_homelab, "download_ca", return_value=public_key().decode()), patch(
                "subprocess.run", side_effect=invoke
            ):
                self.assertEqual(bootstrap_homelab.run(args), 0)
        inventory = json.loads(seen["inventory"])
        self.assertNotIn("dummy-password", seen["inventory"])
        self.assertNotIn("dummy-token", seen["inventory"])
        self.assertFalse(inventory["separate_sudo_password"])
        self.assertNotIn("ssh_key", inventory["host"])
        self.assertEqual(seen["command"][1:5], ["run", "--locked", "pyinfra", "--yes"])
        self.assertFalse(Path(seen["ca_path"]).exists())


class SshHelperTests(unittest.TestCase):
    @staticmethod
    def _lookup(token_ttl: int = 3600, **overrides):
        data = {
            "ttl": token_ttl,
            "policies": ["homelab-ssh-client-sign"],
            "token_policies": ["homelab-ssh-client-sign"],
            "identity_policies": [],
            "meta": {"role": "ssh"},
            "type": "service",
            "path": "auth/oidc/login",
        }
        data.update(overrides)
        return Mock(returncode=0, stdout=json.dumps({"data": data}).encode(), stderr=b"")

    @staticmethod
    def _args(known_hosts: Path, **overrides):
        values = {
            "target": "node.example",
            "port": 2222,
            "known_hosts": known_hosts,
            "openbao_url": "https://openbao.example",
            "reauth": False,
            "logout": False,
        }
        values.update(overrides)
        return argparse.Namespace(**values)

    def test_openbao_url_requires_a_canonical_https_origin(self) -> None:
        self.assertEqual(
            ssh_homelab.validate_openbao_url("https://openbao.holdenitdown.net/"),
            "https://openbao.holdenitdown.net",
        )
        self.assertEqual(
            ssh_homelab.validate_openbao_url("https://openbao.example:8200"),
            "https://openbao.example:8200",
        )
        for value in (
            "http://openbao.example",
            "https://user@openbao.example",
            "https://openbao.example/v1",
            "https://openbao.example?x=1",
            "https://openbao.example#fragment",
            "https://OPENBAO.example",
            "https://openbao.example:0",
            "https://openbao.example:443",
        ):
            with self.subTest(value=value), self.assertRaises(ValueError):
                ssh_homelab.validate_openbao_url(value)

    def test_login_is_no_store_and_sign_token_is_environment_only(self) -> None:
        login_result = Mock(returncode=0, stdout=b"dummy-token")
        hostile = {
            "ALL_PROXY": "http://hostile",
            "BAO_ADDR": "https://stale",
            "BAO_AGENT_ADDR": "http://hostile-agent",
            "BAO_CACERT": "/hostile/ca.pem",
            "BAO_CLIENT_CERT": "/hostile/client.pem",
            "BAO_CLIENT_KEY": "/hostile/client-key.pem",
            "BAO_NAMESPACE": "hostile",
            "BAO_PROXY_ADDR": "http://hostile-proxy",
            "BAO_SKIP_VERIFY": "true",
            "BAO_DISABLE_REDIRECTS": "false",
            "BAO_TOKEN": "old-token",
            "HOME": "/home/operator",
            "HTTPS_PROXY": "http://hostile",
            "HTTP_PROXY": "http://hostile",
            "NO_PROXY": "*",
            "PATH": "/usr/bin",
            "SSL_CERT_DIR": "/hostile/certs",
            "SSL_CERT_FILE": "/hostile/ca.pem",
            "VAULT_ADDR": "https://hostile",
            "VAULT_AGENT_ADDR": "http://hostile-agent",
            "VAULT_CACERT": "/hostile/ca.pem",
            "VAULT_CLIENT_CERT": "/hostile/client.pem",
            "VAULT_CLIENT_KEY": "/hostile/client-key.pem",
            "VAULT_NAMESPACE": "hostile",
            "VAULT_PROXY_ADDR": "http://hostile-proxy",
            "VAULT_SKIP_VERIFY": "true",
            "VAULT_TOKEN": "old-vault",
        }
        with patch.dict(os.environ, hostile, clear=True), patch(
            "subprocess.run", return_value=login_result
        ) as invoke:
            token = ssh_homelab._login("/usr/bin/bao", "https://openbao.example")
        command = invoke.call_args.args[0]
        env = invoke.call_args.kwargs["env"]
        self.assertEqual(token, "dummy-token")
        self.assertIn("-token-only", command)
        self.assertNotIn("-format=json", command)
        self.assertEqual(invoke.call_args.kwargs["stdout"], subprocess.PIPE)
        self.assertNotIn("stderr", invoke.call_args.kwargs)
        self.assertNotIn("BAO_TOKEN", env)
        self.assertNotIn("VAULT_TOKEN", env)
        self.assertEqual(env["BAO_ADDR"], "https://openbao.example")
        self.assertEqual(env["BAO_DISABLE_REDIRECTS"], "true")
        self.assertEqual(set(env), {"BAO_ADDR", "BAO_DISABLE_REDIRECTS", "HOME", "PATH"})
        self.assertNotIn("dummy-token", command)

    def test_lookup_requires_exact_policy_role_type_and_bounded_ttl(self) -> None:
        valid = self._lookup()
        seen = {}

        def invoke(command, **kwargs):
            seen["command"] = command
            seen["env"] = kwargs["env"].copy()
            return valid

        with patch("subprocess.run", side_effect=invoke):
            self.assertIs(
                ssh_homelab._lookup_token("/usr/bin/bao", "dummy-token", "https://openbao.example"),
                ssh_homelab._TokenLookup.VALID,
            )
        self.assertEqual(seen["command"], ["/usr/bin/bao", "token", "lookup", "-format=json"])
        self.assertEqual(seen["env"]["BAO_TOKEN"], "dummy-token")
        invalid = (
            self._lookup(token_ttl=0),
            self._lookup(token_ttl=28_801),
            self._lookup(policies=["default", "homelab-ssh-client-sign"]),
            self._lookup(meta={"role": "admin"}),
            self._lookup(type="batch"),
            self._lookup(identity_policies=["admin"]),
            self._lookup(path="auth/token/create"),
            Mock(returncode=0, stdout=b"{", stderr=b""),
        )
        for result in invalid:
            with self.subTest(result=result), patch("subprocess.run", return_value=result):
                expected = (
                    ssh_homelab._TokenLookup.INDETERMINATE
                    if result.stdout == b"{"
                    else ssh_homelab._TokenLookup.CONTRACT_INVALID
                )
                self.assertIs(ssh_homelab._lookup_token("bao", "secret", "https://openbao.example"), expected)

        with patch(
            "subprocess.run",
            return_value=Mock(returncode=2, stdout=b"", stderr=b"permission denied"),
        ):
            self.assertIs(
                ssh_homelab._lookup_token("bao", "secret", "https://openbao.example"),
                ssh_homelab._TokenLookup.REJECTED,
            )

        with patch("subprocess.run", side_effect=subprocess.TimeoutExpired("bao", 15)):
            self.assertIs(
                ssh_homelab._lookup_token("bao", "secret", "https://openbao.example"),
                ssh_homelab._TokenLookup.INDETERMINATE,
            )

    def test_session_reuses_cache_without_login_and_cleans_ephemeral_identity(self) -> None:
        with tempfile.NamedTemporaryFile() as known_hosts, tempfile.TemporaryDirectory() as runtime:
            cache = TokenCache(Path(runtime))
            cache.save("https://openbao.example", "dummy-token")
            args = self._args(Path(known_hosts.name))
            calls = []
            temp_path = None

            def invoke(command, **kwargs):
                nonlocal temp_path
                calls.append((command, kwargs))
                if command[1:3] == ["token", "lookup"]:
                    self.assertEqual(kwargs["env"]["BAO_DISABLE_REDIRECTS"], "true")
                    return self._lookup()
                if command[1] == "write":
                    self.assertEqual(kwargs["env"].get("BAO_TOKEN"), "dummy-token")
                    self.assertEqual(kwargs["env"].get("BAO_ADDR"), "https://openbao.example")
                    self.assertEqual(kwargs["env"]["BAO_DISABLE_REDIRECTS"], "true")
                    self.assertNotIn("dummy-token", command)
                    return Mock(returncode=0, stdout=b"dummy-certificate", stderr=b"")
                if command[1] == "-L":
                    self.assertEqual(kwargs["env"], {"LC_ALL": "C", "TZ": "UTC"})
                    return Mock(returncode=0, stdout="validated", stderr="")
                if command[0] == "/usr/bin/ssh":
                    temp_path = Path(command[command.index("-i") + 1]).parent
                    return Mock(returncode=7)
                return Mock(returncode=0, stdout=b"")

            with patch.dict(os.environ, {"XDG_RUNTIME_DIR": runtime, "PATH": "/usr/bin"}, clear=True), patch.object(ssh_homelab, "_executable", side_effect=lambda name: f"/usr/bin/{name}"), patch(
                "subprocess.run", side_effect=invoke
            ), patch.object(ssh_homelab, "validate_user_certificate"):
                self.assertEqual(ssh_homelab.run(args), 7)
        ssh_command = calls[-1][0]
        self.assertIn("IdentitiesOnly=yes", ssh_command)
        self.assertIn("BatchMode=yes", ssh_command)
        self.assertIn("PreferredAuthentications=publickey", ssh_command)
        self.assertEqual(ssh_command[1:3], ["-F", "none"])
        self.assertIn("StrictHostKeyChecking=yes", ssh_command)
        self.assertIn(f"UserKnownHostsFile={Path(known_hosts.name).resolve()}", ssh_command)
        self.assertIn("CertificateFile=", " ".join(ssh_command))
        self.assertFalse(temp_path.exists())
        self.assertFalse(any(command[1] == "login" for command, _ in calls))

    def test_invalid_cached_token_logs_in_and_saves_only_after_validation(self) -> None:
        with tempfile.NamedTemporaryFile() as known_hosts, tempfile.TemporaryDirectory() as runtime:
            cache = TokenCache(Path(runtime))
            cache.save("https://openbao.example", "expired-token")
            calls = []

            def invoke(command, **kwargs):
                recorded = kwargs.copy()
                if "env" in recorded:
                    recorded["env"] = recorded["env"].copy()
                calls.append((command, recorded))
                if command[1:3] == ["token", "lookup"]:
                    token = kwargs["env"]["BAO_TOKEN"]
                    return Mock(returncode=1, stdout=b"", stderr=b"permission denied") if token == "expired-token" else self._lookup()
                if command[1] == "login":
                    self.assertFalse(cache.path.exists())
                    return Mock(returncode=0, stdout=b"new-token")
                if command[1] == "write":
                    return Mock(returncode=0, stdout=b"certificate", stderr=b"")
                if command[1] == "-L":
                    return Mock(returncode=0, stdout="validated", stderr="")
                return Mock(returncode=0, stdout=b"", stderr=b"")

            with patch.dict(os.environ, {"XDG_RUNTIME_DIR": runtime, "PATH": "/usr/bin"}, clear=True), patch.object(
                ssh_homelab, "_executable", side_effect=lambda name: f"/usr/bin/{name}"
            ), patch("subprocess.run", side_effect=invoke), patch.object(ssh_homelab, "validate_user_certificate"):
                self.assertEqual(ssh_homelab.run(self._args(Path(known_hosts.name))), 0)
            self.assertEqual(cache.load("https://openbao.example").token, "new-token")
            self.assertEqual(sum(command[1] == "login" for command, _ in calls), 1)
            self.assertFalse(any(command[1:3] == ["token", "revoke"] for command, _ in calls))

    def test_contract_invalid_cached_token_is_revoked_before_login_with_safe_warning(self) -> None:
        with tempfile.NamedTemporaryFile() as known_hosts, tempfile.TemporaryDirectory() as runtime:
            cache = TokenCache(Path(runtime))
            cache.save("https://openbao.example", "contract-invalid-token")
            calls = []

            def invoke(command, **kwargs):
                recorded = kwargs.copy()
                if "env" in recorded:
                    recorded["env"] = recorded["env"].copy()
                calls.append((command, recorded))
                if command[1:3] == ["token", "lookup"]:
                    if kwargs["env"]["BAO_TOKEN"] == "contract-invalid-token":
                        return self._lookup(policies=["default"])
                    return self._lookup()
                if command[1:3] == ["token", "revoke"]:
                    return Mock(
                        returncode=1,
                        stdout=b"contract-invalid-token",
                        stderr=b"contract-invalid-token",
                    )
                if command[1] == "login":
                    self.assertFalse(cache.path.exists())
                    return Mock(returncode=0, stdout=b"new-token")
                if command[1] == "write":
                    return Mock(returncode=0, stdout=b"certificate", stderr=b"")
                if command[1] == "-L":
                    return Mock(returncode=0, stdout="validated", stderr="")
                return Mock(returncode=0, stdout=b"", stderr=b"")

            with patch.dict(os.environ, {"XDG_RUNTIME_DIR": runtime, "PATH": "/usr/bin"}, clear=True), patch.object(
                ssh_homelab, "_executable", side_effect=lambda name: f"/usr/bin/{name}"
            ), patch("subprocess.run", side_effect=invoke), patch.object(
                ssh_homelab, "validate_user_certificate"
            ), patch("sys.stderr", new_callable=io.StringIO) as stderr:
                self.assertEqual(ssh_homelab.run(self._args(Path(known_hosts.name))), 0)

            revoke_index = next(index for index, call in enumerate(calls) if call[0][1:3] == ["token", "revoke"])
            login_index = next(index for index, call in enumerate(calls) if call[0][1] == "login")
            self.assertLess(revoke_index, login_index)
            self.assertIn("contract-invalid cached OpenBao token", stderr.getvalue())
            self.assertNotIn("contract-invalid-token", stderr.getvalue())
            self.assertTrue(all("contract-invalid-token" not in command for command, _ in calls))

    def test_indeterminate_cached_lookup_preserves_cache_and_fails_closed(self) -> None:
        with tempfile.NamedTemporaryFile() as known_hosts, tempfile.TemporaryDirectory() as runtime:
            cache = TokenCache(Path(runtime))
            cache.save("https://openbao.example", "cached-token")
            calls = []

            def invoke(command, **kwargs):
                calls.append(command)
                if command[1:3] == ["token", "lookup"]:
                    raise subprocess.TimeoutExpired(command, 15)
                return Mock(returncode=0, stdout=b"", stderr=b"")

            with patch.dict(os.environ, {"XDG_RUNTIME_DIR": runtime, "PATH": "/usr/bin"}, clear=True), patch.object(
                ssh_homelab, "_executable", side_effect=lambda name: f"/usr/bin/{name}"
            ), patch("subprocess.run", side_effect=invoke), self.assertRaisesRegex(
                RuntimeError, "validation was indeterminate; the cache was preserved"
            ) as raised:
                ssh_homelab.run(self._args(Path(known_hosts.name)))
            self.assertEqual(cache.load("https://openbao.example").token, "cached-token")
            self.assertFalse(any(command[1] == "login" for command in calls))
            self.assertNotIn("cached-token", str(raised.exception))

    def test_indeterminate_new_token_is_revoked_and_not_cached(self) -> None:
        with tempfile.TemporaryDirectory() as runtime:
            cache = TokenCache(Path(runtime))
            calls = []

            def invoke(command, **kwargs):
                calls.append((command, kwargs["env"].copy()))
                if command[1] == "login":
                    return Mock(returncode=0, stdout=b"new-token")
                if command[1:3] == ["token", "lookup"]:
                    return Mock(returncode=0, stdout=b"{", stderr=b"")
                if command[1:3] == ["token", "revoke"]:
                    return Mock(returncode=0, stdout=b"", stderr=b"")
                raise AssertionError(command)

            with patch.dict(os.environ, {"XDG_RUNTIME_DIR": runtime, "PATH": "/usr/bin"}, clear=True), patch(
                "subprocess.run", side_effect=invoke
            ), self.assertRaisesRegex(RuntimeError, "validation was indeterminate") as raised:
                ssh_homelab._login_validate_and_save("/usr/bin/bao", "https://openbao.example", cache)
            self.assertFalse(cache.path.exists())
            self.assertEqual([call[0][1:3] for call in calls], [["login", "-method=oidc"], ["token", "lookup"], ["token", "revoke"]])
            self.assertTrue(all(env["BAO_DISABLE_REDIRECTS"] == "true" for _, env in calls))
            self.assertTrue(all("new-token" not in command for command, _ in calls))
            self.assertNotIn("new-token", str(raised.exception))

    def test_contract_invalid_new_token_is_revoked_and_not_cached(self) -> None:
        with tempfile.TemporaryDirectory() as runtime:
            cache = TokenCache(Path(runtime))
            calls = []

            def invoke(command, **kwargs):
                calls.append((command, kwargs["env"].copy()))
                if command[1] == "login":
                    return Mock(returncode=0, stdout=b"new-token")
                if command[1:3] == ["token", "lookup"]:
                    return self._lookup(policies=["default"])
                if command[1:3] == ["token", "revoke"]:
                    return Mock(returncode=0, stdout=b"", stderr=b"")
                raise AssertionError(command)

            with patch.dict(os.environ, {"XDG_RUNTIME_DIR": runtime, "PATH": "/usr/bin"}, clear=True), patch(
                "subprocess.run", side_effect=invoke
            ), self.assertRaisesRegex(RuntimeError, "did not satisfy") as raised:
                ssh_homelab._login_validate_and_save("/usr/bin/bao", "https://openbao.example", cache)
            self.assertFalse(cache.path.exists())
            self.assertEqual(calls[-1][0][1:3], ["token", "revoke"])
            self.assertNotIn("new-token", str(raised.exception))

    def test_cached_token_rejection_reauthenticates_and_retries_sign_once(self) -> None:
        with tempfile.NamedTemporaryFile() as known_hosts, tempfile.TemporaryDirectory() as runtime:
            TokenCache(Path(runtime)).save("https://openbao.example", "cached-token")
            signs = []
            calls = []

            def invoke(command, **kwargs):
                recorded = kwargs.copy()
                if "env" in recorded:
                    recorded["env"] = recorded["env"].copy()
                calls.append((command, recorded))
                if command[1:3] == ["token", "lookup"]:
                    return self._lookup()
                if command[1:3] == ["token", "revoke"]:
                    return Mock(returncode=1, stdout=b"cached-token", stderr=b"cached-token")
                if command[1] == "login":
                    return Mock(returncode=0, stdout=b"new-token")
                if command[1] == "write":
                    signs.append(kwargs["env"]["BAO_TOKEN"])
                    if len(signs) == 1:
                        return Mock(returncode=2, stdout=b"", stderr=b"permission denied")
                    return Mock(returncode=0, stdout=b"certificate", stderr=b"")
                if command[1] == "-L":
                    return Mock(returncode=0, stdout="validated", stderr="")
                return Mock(returncode=0, stdout=b"", stderr=b"")

            with patch.dict(os.environ, {"XDG_RUNTIME_DIR": runtime, "PATH": "/usr/bin"}, clear=True), patch.object(
                ssh_homelab, "_executable", side_effect=lambda name: f"/usr/bin/{name}"
            ), patch("subprocess.run", side_effect=invoke), patch.object(
                ssh_homelab, "validate_user_certificate"
            ), patch("sys.stderr", new_callable=io.StringIO) as stderr:
                self.assertEqual(ssh_homelab.run(self._args(Path(known_hosts.name))), 0)
            self.assertEqual(signs, ["cached-token", "new-token"])
            self.assertEqual(sum(command[1] == "login" for command, _ in calls), 1)
            revoke_index = next(index for index, call in enumerate(calls) if call[0][1:3] == ["token", "revoke"])
            login_index = next(index for index, call in enumerate(calls) if call[0][1] == "login")
            self.assertLess(revoke_index, login_index)
            self.assertIn("could not be confirmed revoked", stderr.getvalue())
            self.assertNotIn("cached-token", stderr.getvalue())
            revoke = next(call for call in calls if call[0][1:3] == ["token", "revoke"])
            self.assertEqual(revoke[1]["env"]["BAO_DISABLE_REDIRECTS"], "true")

    def test_arbitrary_signing_failure_is_safe_and_not_classified_for_retry(self) -> None:
        result = Mock(returncode=2, stdout=b"secret", stderr=b"backend unavailable: secret")
        with tempfile.TemporaryDirectory() as directory, patch("subprocess.run", return_value=result) as invoke:
            with self.assertRaisesRegex(RuntimeError, "OpenBao SSH certificate request failed") as raised:
                ssh_homelab._sign(
                    "/usr/bin/bao",
                    Path(directory) / "id.pub",
                    Path(directory) / "id-cert.pub",
                    "secret",
                    "https://openbao.example",
                )
        self.assertEqual(invoke.call_count, 1)
        self.assertNotIn("secret", str(raised.exception))
        self.assertNotIn("secret", invoke.call_args.args[0])

    def test_reauth_revokes_clears_and_logs_in(self) -> None:
        with tempfile.NamedTemporaryFile() as known_hosts, tempfile.TemporaryDirectory() as runtime:
            TokenCache(Path(runtime)).save("https://openbao.example", "cached-token")
            calls = []

            def invoke(command, **kwargs):
                recorded = kwargs.copy()
                if "env" in recorded:
                    recorded["env"] = recorded["env"].copy()
                calls.append((command, recorded))
                if command[1:3] == ["token", "revoke"]:
                    return Mock(returncode=0, stdout=b"", stderr=b"")
                if command[1] == "login":
                    return Mock(returncode=0, stdout=b"new-token")
                if command[1:3] == ["token", "lookup"]:
                    return self._lookup()
                if command[1] == "write":
                    return Mock(returncode=0, stdout=b"certificate", stderr=b"")
                if command[1] == "-L":
                    return Mock(returncode=0, stdout="validated", stderr="")
                return Mock(returncode=0, stdout=b"", stderr=b"")

            with patch.dict(os.environ, {"XDG_RUNTIME_DIR": runtime, "PATH": "/usr/bin"}, clear=True), patch.object(
                ssh_homelab, "_executable", side_effect=lambda name: f"/usr/bin/{name}"
            ), patch("subprocess.run", side_effect=invoke), patch.object(ssh_homelab, "validate_user_certificate"):
                self.assertEqual(ssh_homelab.run(self._args(Path(known_hosts.name), reauth=True)), 0)
            revoke = next(call for call in calls if call[0][1:3] == ["token", "revoke"])
            self.assertEqual(revoke[0], ["/usr/bin/bao", "token", "revoke", "-self"])
            self.assertEqual(revoke[1]["env"]["BAO_TOKEN"], "cached-token")
            self.assertEqual(revoke[1]["env"]["BAO_DISABLE_REDIRECTS"], "true")

    def test_logout_needs_no_target_and_missing_cache_succeeds(self) -> None:
        with tempfile.TemporaryDirectory() as runtime:
            args = self._args(Path("unused"), target=None, logout=True)
            with patch.dict(os.environ, {"XDG_RUNTIME_DIR": runtime}, clear=True), patch.object(
                ssh_homelab, "_executable"
            ) as executable:
                self.assertEqual(ssh_homelab.run(args), 0)
            executable.assert_not_called()

            cache = TokenCache(Path(runtime))
            cache.save("https://openbao.example", "cached-token")
            with patch.dict(os.environ, {"XDG_RUNTIME_DIR": runtime}, clear=True), patch.object(
                ssh_homelab, "_executable", return_value="/usr/bin/bao"
            ), patch("subprocess.run", return_value=Mock(returncode=1, stdout=b"cached-token", stderr=b"cached-token")), patch(
                "sys.stderr", new_callable=io.StringIO
            ) as stderr:
                self.assertEqual(ssh_homelab.run(args), 0)
            self.assertFalse(cache.path.exists())
            self.assertNotIn("cached-token", stderr.getvalue())

    def test_target_is_required_except_for_logout(self) -> None:
        parsed = ssh_homelab.parser().parse_args(["--logout"])
        self.assertIsNone(parsed.target)
        with tempfile.TemporaryDirectory() as runtime, patch.dict(
            os.environ, {"XDG_RUNTIME_DIR": runtime}, clear=True
        ), self.assertRaises(ValueError):
            ssh_homelab.run(self._args(Path("unused"), target=None))


if __name__ == "__main__":
    unittest.main()
