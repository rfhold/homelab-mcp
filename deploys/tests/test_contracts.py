import json
import os
import base64
import struct
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

from paramiko import AuthenticationException, BadAuthenticationType
from pyinfra.api.exceptions import ConnectError
from pyinfra.connectors.ssh import SSHConnector

from deploys.lib.inventory import bootstrap_inventory, system_info_inventory
from deploys.lib.bootstrap import _SSHD_DROP_IN, _SUDOERS, _bootstrap_operation
from deploys.lib.password_auth import AgentFirstAuthStrategy, MAX_AGENT_KEY_ATTEMPTS
from deploys.lib.system_info import _SYSTEM_INFO_COMMAND


ROOT = Path(__file__).parents[2]


class _Agent:
    def __init__(self, keys=()):
        self.keys = keys
        self.closed = False

    def get_keys(self):
        return iter(self.keys)

    def close(self):
        self.closed = True


class _Transport:
    def __init__(self, allowed, accepted_key=None, accepted_password=None):
        self.allowed = allowed
        self.accepted_key = accepted_key
        self.accepted_password = accepted_password
        self.key_attempts = []
        self.password_attempts = []

    def auth_none(self, username):
        raise BadAuthenticationType("authentication required", self.allowed)

    def auth_publickey(self, username, key):
        self.key_attempts.append(key)
        if key is not self.accepted_key:
            raise AuthenticationException("key rejected")
        return []

    def auth_password(self, username, password, fallback=True):
        self.password_attempts.append((password, fallback))
        if password != self.accepted_password:
            raise AuthenticationException("password rejected")
        return []


class CatalogTests(unittest.TestCase):
    def test_catalog_is_strict_and_complete(self) -> None:
        catalog = json.loads((ROOT / "deploys/catalog.json").read_text())
        self.assertEqual(catalog["version"], 1)
        self.assertEqual([item["id"] for item in catalog["deploys"]], ["bootstrap-homelab", "system-info"])
        expected = {
            "id", "description", "entrypoint", "inventory", "risk",
            "local_available", "mcp_available", "supported_distributions",
            "timeout_seconds", "requires_sudo", "mutating",
        }
        for item in catalog["deploys"]:
            self.assertEqual(set(item), expected)
            self.assertTrue((ROOT / item["entrypoint"]).is_file())
            self.assertTrue((ROOT / item["inventory"]).is_file())
            self.assertEqual(item["supported_distributions"], ["debian", "ubuntu", "arch"])

    def test_bootstrap_policy_is_fixed(self) -> None:
        self.assertEqual(_SUDOERS, "homelab ALL=(ALL:ALL) NOPASSWD: ALL\n")
        self.assertEqual(_SSHD_DROP_IN, "TrustedUserCAKeys /etc/ssh/trusted-user-ca-keys.pem\n")


class InventoryTests(unittest.TestCase):
    def _ca_key(self) -> str:
        key_type = b"ssh-ed25519"
        blob = struct.pack(">I", len(key_type)) + key_type + struct.pack(">I", 32) + bytes(32)
        return "ssh-ed25519 " + base64.b64encode(blob).decode() + " test-ca\n"

    def _base(self, key: str, known_hosts: str) -> dict:
        return {
            "version": 1,
            "host": {
                "address": "node.example",
                "user": "admin",
                "port": 22,
                "ssh_key": key,
                "known_hosts": known_hosts,
            },
        }

    def _bootstrap_data(self, known_hosts: str, ca_file: str, separate: bool = False):
        value = {
            "version": 1,
            "host": {
                "address": "node.example",
                "user": "admin",
                "port": 22,
                "known_hosts": known_hosts,
            },
            "ca_public_key_file": ca_file,
            "separate_sudo_password": separate,
        }
        with patch.dict(os.environ, {"HOMELAB_INVENTORY_JSON": json.dumps(value)}):
            return bootstrap_inventory()[0][1]

    def test_system_info_accepts_only_fixed_shape(self) -> None:
        with tempfile.NamedTemporaryFile() as key, tempfile.NamedTemporaryFile() as known_hosts:
            value = self._base(key.name, known_hosts.name)
            with patch.dict(os.environ, {"HOMELAB_INVENTORY_JSON": json.dumps(value)}):
                address, data = system_info_inventory()[0]
            self.assertEqual(address, "node.example")
            self.assertEqual(data["ssh_known_hosts_file"], known_hosts.name)
            self.assertEqual(data["ssh_strict_host_key_checking"], "yes")
            self.assertNotIn("ssh_config_file", data)
            value["command"] = "id"
            with patch.dict(os.environ, {"HOMELAB_INVENTORY_JSON": json.dumps(value)}):
                with self.assertRaises(ValueError):
                    system_info_inventory()

    def test_inventory_requires_existing_known_hosts_file(self) -> None:
        with tempfile.NamedTemporaryFile() as key:
            value = self._base(key.name, "/missing/known_hosts")
            with patch.dict(os.environ, {"HOMELAB_INVENTORY_JSON": json.dumps(value)}):
                with self.assertRaises(ValueError):
                    system_info_inventory()
            del value["host"]["known_hosts"]
            with patch.dict(os.environ, {"HOMELAB_INVENTORY_JSON": json.dumps(value)}):
                with self.assertRaises(ValueError):
                    system_info_inventory()

    def test_inventory_accepts_ipv6_with_supported_ports(self) -> None:
        with tempfile.NamedTemporaryFile() as key, tempfile.NamedTemporaryFile() as known_hosts:
            for address, port in (("2001:db8::10", 22), ("2001:db8::20", 2222)):
                value = self._base(key.name, known_hosts.name)
                value["host"]["address"] = address
                value["host"]["port"] = port
                with patch.dict(os.environ, {"HOMELAB_INVENTORY_JSON": json.dumps(value)}):
                    inventory_address, data = system_info_inventory()[0]
                self.assertEqual(inventory_address, address)
                self.assertEqual(data["ssh_port"], port)

    def test_inventory_rejects_malformed_names_and_addresses(self) -> None:
        with tempfile.NamedTemporaryFile() as key, tempfile.NamedTemporaryFile() as known_hosts:
            for address in ("2001:db8:::1", "999.1.1.1", "bad..example", "-bad.example"):
                value = self._base(key.name, known_hosts.name)
                value["host"]["address"] = address
                with patch.dict(os.environ, {"HOMELAB_INVENTORY_JSON": json.dumps(value)}):
                    with self.assertRaises(ValueError):
                        system_info_inventory()

    def test_bootstrap_requires_one_public_ca_key(self) -> None:
        with tempfile.NamedTemporaryFile() as key, tempfile.NamedTemporaryFile() as known_hosts, tempfile.NamedTemporaryFile(mode="w+") as ca:
            ca.write(self._ca_key())
            ca.flush()
            value = {
                "version": 1,
                "host": {
                    "address": "node.example",
                    "user": "admin",
                    "port": 22,
                    "known_hosts": known_hosts.name,
                },
                "ca_public_key_file": ca.name,
                "separate_sudo_password": False,
            }
            with patch.dict(os.environ, {"HOMELAB_INVENTORY_JSON": json.dumps(value)}), patch(
                "deploys.lib.inventory.getpass", return_value="dummy-password"
            ) as prompt:
                data = bootstrap_inventory()[0][1]
            prompt.assert_not_called()
            self.assertTrue(data["homelab_ca_public_key"].startswith("ssh-ed25519 "))
            self.assertEqual(data["ssh_known_hosts_file"], known_hosts.name)
            self.assertEqual(data["ssh_strict_host_key_checking"], "yes")
            self.assertFalse(data["ssh_allow_agent"])
            self.assertFalse(data["ssh_look_for_keys"])
            self.assertNotIn("ssh_key", data)
            self.assertNotIn("ssh_password", data)
            self.assertEqual(data["ssh_config_file"], "/dev/null")
            strategy = data["ssh_paramiko_connect_kwargs"]["auth_strategy"]
            self.assertNotIn("dummy-password", repr(strategy))
            self.assertNotIn("dummy-password", repr(data["ssh_paramiko_connect_kwargs"]))
            self.assertNotIn("dummy-password", repr(data))
            connector = SSHConnector(
                SimpleNamespace(config=SimpleNamespace(CONNECT_TIMEOUT=10)),
                SimpleNamespace(data=data, name="node.example"),
            )
            connect_kwargs = connector.make_paramiko_kwargs()
            self.assertNotIn("password", connect_kwargs)
            self.assertNotIn("dummy-password", repr(connect_kwargs))
            with patch("pyinfra.connectors.ssh.SSHClient") as client_type, patch(
                "pyinfra.connectors.ssh.logger.debug"
            ) as debug:
                client_type.return_value.connect.side_effect = AuthenticationException("denied")
                with self.assertRaises(ConnectError) as error:
                    connector._connect()
            self.assertNotIn("dummy-password", str(error.exception))
            self.assertNotIn("dummy-password", repr(debug.call_args_list))

    def test_bootstrap_rejects_private_material(self) -> None:
        with tempfile.NamedTemporaryFile() as key, tempfile.NamedTemporaryFile() as known_hosts, tempfile.NamedTemporaryFile(mode="w+") as ca:
            ca.write("-----BEGIN OPENSSH " + "PRIVATE KEY-----\n")
            ca.flush()
            value = {
                "version": 1,
                "host": {"address": "node.example", "user": "admin", "port": 22, "known_hosts": known_hosts.name},
                "ca_public_key_file": ca.name,
                "separate_sudo_password": False,
            }
            with patch.dict(os.environ, {"HOMELAB_INVENTORY_JSON": json.dumps(value)}), patch(
                "deploys.lib.inventory.getpass", return_value="dummy-password"
            ):
                with self.assertRaises(ValueError):
                    bootstrap_inventory()

    def test_bootstrap_can_prompt_for_separate_sudo_password(self) -> None:
        with tempfile.NamedTemporaryFile() as known_hosts, tempfile.NamedTemporaryFile(mode="w+") as ca:
            ca.write(self._ca_key())
            ca.flush()
            with patch("deploys.lib.inventory.getpass", side_effect=["dummy-ssh", "dummy-sudo"]) as prompt:
                data = self._bootstrap_data(known_hosts.name, ca.name, separate=True)
                self.assertEqual(prompt.call_count, 0)
                strategy = data["ssh_paramiko_connect_kwargs"]["auth_strategy"]
                agent = _Agent()
                strategy._agent_factory = lambda: agent
                strategy.authenticate(_Transport(["password"], accepted_password="dummy-ssh"))
                self.assertEqual(prompt.call_count, 1)
                self.assertEqual(data["homelab_sudo_password"].reveal(), "dummy-sudo")
            self.assertEqual(prompt.call_count, 2)
            self.assertNotIn("dummy-ssh", repr(strategy))
            self.assertNotIn("dummy-sudo", repr(data))

    def test_agent_keys_preserve_order_avoid_ssh_prompt_and_close_before_lazy_sudo(self) -> None:
        with tempfile.NamedTemporaryFile() as known_hosts, tempfile.NamedTemporaryFile(mode="w+") as ca:
            ca.write(self._ca_key())
            ca.flush()
            first = Mock()
            first.get_name.return_value = "ssh-ed25519"
            first.fingerprint = "SHA256:first"
            second = Mock()
            second.get_name.return_value = "ssh-ed25519"
            second.fingerprint = "SHA256:second"
            second.private_material = "SENTINEL-private-key"
            agent = _Agent([first, second])
            prompt = Mock(return_value="sudo-password")
            with patch("deploys.lib.inventory.getpass", prompt):
                data = self._bootstrap_data(known_hosts.name, ca.name)
                strategy = data["ssh_paramiko_connect_kwargs"]["auth_strategy"]
                strategy._agent_factory = lambda: agent
                transport = _Transport(["publickey"], accepted_key=second)
                result = strategy.authenticate(transport)
                self.assertEqual(prompt.call_count, 0)
                self.assertTrue(agent.closed)
                self.assertEqual(transport.key_attempts, [first, second])
                self.assertEqual(data["homelab_sudo_password"].reveal(), "sudo-password")
                self.assertEqual(prompt.call_count, 1)
                self.assertNotIn("SENTINEL", str(result))
                self.assertNotIn("SENTINEL", repr(result))

    def test_rejected_agent_falls_back_to_one_password_and_reuses_it_for_sudo(self) -> None:
        with tempfile.NamedTemporaryFile() as known_hosts, tempfile.NamedTemporaryFile(mode="w+") as ca:
            ca.write(self._ca_key())
            ca.flush()
            key = Mock()
            key.get_name.return_value = "ssh-ed25519"
            key.fingerprint = "SHA256:rejected"
            agent = _Agent([key])
            prompt = Mock(return_value="shared-password")
            with patch("deploys.lib.inventory.getpass", prompt):
                data = self._bootstrap_data(known_hosts.name, ca.name)
                strategy = data["ssh_paramiko_connect_kwargs"]["auth_strategy"]
                strategy._agent_factory = lambda: agent
                transport = _Transport(
                    ["publickey", "password"],
                    accepted_password="shared-password",
                )
                strategy.authenticate(transport)
                self.assertTrue(agent.closed)
                self.assertEqual(prompt.call_count, 1)
                self.assertEqual(transport.password_attempts, [("shared-password", False)])
                self.assertEqual(data["homelab_sudo_password"].reveal(), "shared-password")
                self.assertEqual(prompt.call_count, 1)

    def test_publickey_only_failure_never_prompts_password_and_is_safe(self) -> None:
        key = Mock()
        key.get_name.return_value = "ssh-ed25519"
        key.fingerprint = "SHA256:rejected"
        key.private_material = "SENTINEL-private-key"
        agent = _Agent([key])
        prompt = Mock(return_value="SENTINEL-password")
        strategy = AgentFirstAuthStrategy("admin", prompt, agent_factory=lambda: agent)
        with self.assertRaisesRegex(AuthenticationException, "no bounded agent identity succeeded") as error:
            strategy.authenticate(_Transport(["publickey"]))
        self.assertTrue(agent.closed)
        prompt.assert_not_called()
        self.assertNotIn("SENTINEL", str(error.exception))
        self.assertNotIn("SENTINEL", repr(strategy))

        empty_agent = _Agent()
        empty_prompt = Mock(return_value="SENTINEL-password")
        empty_strategy = AgentFirstAuthStrategy(
            "admin",
            empty_prompt,
            agent_factory=lambda: empty_agent,
        )
        with self.assertRaisesRegex(AuthenticationException, "no bounded agent identity succeeded"):
            empty_strategy.authenticate(_Transport(["publickey"]))
        self.assertTrue(empty_agent.closed)
        empty_prompt.assert_not_called()

    def test_password_only_skips_agent_and_disables_interactive_fallback(self) -> None:
        prompt = Mock(return_value="password")
        agent_factory = Mock(side_effect=AssertionError("agent must not be opened"))
        strategy = AgentFirstAuthStrategy("admin", prompt, agent_factory=agent_factory)
        transport = _Transport(["password"], accepted_password="password")
        strategy.authenticate(transport)
        agent_factory.assert_not_called()
        prompt.assert_called_once_with()
        self.assertEqual(transport.password_attempts, [("password", False)])

    def test_agent_attempt_budget_truncates_then_reaches_password_fallback(self) -> None:
        keys = []
        for index in range(MAX_AGENT_KEY_ATTEMPTS + 3):
            key = Mock()
            key.get_name.return_value = "ssh-ed25519"
            key.fingerprint = f"SHA256:key-{index}"
            keys.append(key)
        keys[MAX_AGENT_KEY_ATTEMPTS].private_material = "SENTINEL-excess-key-material"
        agent = _Agent(keys)
        prompt = Mock(return_value="fallback-password")
        strategy = AgentFirstAuthStrategy("admin", prompt, agent_factory=lambda: agent)
        transport = _Transport(
            ["publickey", "password"],
            accepted_password="fallback-password",
        )
        result = strategy.authenticate(transport)
        self.assertTrue(agent.closed)
        self.assertEqual(transport.key_attempts, keys[:MAX_AGENT_KEY_ATTEMPTS])
        self.assertNotIn(keys[MAX_AGENT_KEY_ATTEMPTS], transport.key_attempts)
        self.assertEqual(transport.password_attempts, [("fallback-password", False)])
        prompt.assert_called_once_with()
        self.assertNotIn("SENTINEL", str(result))
        self.assertNotIn("SENTINEL", repr(result))

    def test_agent_query_failure_closes_and_redacts_error(self) -> None:
        class BrokenAgent(_Agent):
            def get_keys(self):
                raise RuntimeError("SENTINEL-private-key")

        agent = BrokenAgent()
        strategy = AgentFirstAuthStrategy(
            "admin",
            Mock(return_value="SENTINEL-password"),
            agent_factory=lambda: agent,
        )
        with self.assertRaisesRegex(AuthenticationException, "invalid response") as error:
            strategy.authenticate(_Transport(["publickey"]))
        self.assertTrue(agent.closed)
        self.assertNotIn("SENTINEL", str(error.exception))
        self.assertNotIn("SENTINEL", repr(error.exception))

    def test_passwords_are_absent_from_remote_commands_and_debug_representations(self) -> None:
        sentinel = "SENTINEL-password-do-not-surface"
        command, stdin = _bootstrap_operation("debian", self._ca_key(), sentinel)
        self.assertNotIn(sentinel, command)
        self.assertNotIn("PYINFRA_SUDO_PASSWORD", command)
        self.assertNotIn("SUDO_ASKPASS", command)
        self.assertIn("sudo -S -k -p ''", command)
        self.assertIn("exec </dev/null", command)
        self.assertNotIn(sentinel, repr(stdin))
        self.assertEqual(stdin.readlines(), [sentinel + "\n"])

    def test_bootstrap_uses_each_user_creation_branch_and_preserves_existing_primary_group(self) -> None:
        command, _ = _bootstrap_operation("debian", self._ca_key(), "password")
        self.assertIn("if id -u homelab", command)
        self.assertIn("homelab_group=$(id -gn homelab); usermod -d /home/homelab", command)
        self.assertIn("elif getent group homelab", command)
        self.assertIn("homelab_group=homelab; useradd -m -d /home/homelab -s /bin/sh -g homelab homelab", command)
        self.assertIn("else useradd -m -d /home/homelab -s /bin/sh homelab; homelab_group=$(id -gn homelab); fi", command)
        self.assertIn('install -d -o homelab -g "$homelab_group" -m 0755 /home/homelab', command)
        self.assertNotIn("usermod -g", command)
        self.assertNotIn("install -d -o homelab -g homelab", command)


class SystemInfoTests(unittest.TestCase):
    def test_command_is_bounded_and_avoids_sensitive_sources(self) -> None:
        self.assertIn("head -c 32768", _SYSTEM_INFO_COMMAND)
        self.assertIn("HOMELAB_SYSTEM_INFO_V1_BEGIN", _SYSTEM_INFO_COMMAND)
        self.assertIn("emit uptime uptime -p", _SYSTEM_INFO_COMMAND)
        for forbidden in ("/proc/", "ps ", "env", "printenv", "journalctl"):
            self.assertNotIn(forbidden, _SYSTEM_INFO_COMMAND)


if __name__ == "__main__":
    unittest.main()
