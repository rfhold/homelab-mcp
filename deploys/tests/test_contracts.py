import json
import os
import base64
import io
import shlex
import subprocess
import struct
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import ANY, Mock, patch

from paramiko import AuthenticationException, BadAuthenticationType
from pyinfra.api.exceptions import ConnectError
from pyinfra.connectors.ssh import SSHConnector
from pyinfra.connectors.ssh_util import get_private_key

from deploys.lib.inventory import bootstrap_inventory, system_info_inventory
from deploys.lib.bootstrap import (
    _SSHD_DROP_IN,
    _SUDOERS,
    _SUDO_SHELL,
    _configure_bootstrap,
    _ssh_trust_command,
    _sudoers_operation,
)
from deploys.lib.password_auth import AgentFirstAuthStrategy, MAX_AGENT_KEY_ATTEMPTS
from deploys.lib.system_info import (
    _SYSTEM_INFO_COMMAND,
    _SYSTEM_INFO_METADATA_NAME,
    _SYSTEM_INFO_OPERATION,
    _SystemInfoOutputCallback,
)


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


class RuntimeSshCredentialTests(unittest.TestCase):
    def test_pyinfra_retains_certificate_when_plain_public_key_is_absent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            identity = root / "identity"
            ca = root / "ca"
            for key in (identity, ca):
                subprocess.run(
                    ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", "", "-f", key],
                    check=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                )
            subprocess.run(
                [
                    "ssh-keygen", "-q", "-s", ca, "-I", "runtime-test",
                    "-n", "homelab", "-V", "-30s:+15m", identity.with_suffix(".pub"),
                ],
                check=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            identity.with_suffix(".pub").unlink()

            self.assertTrue(identity.is_file())
            self.assertTrue((root / "identity-cert.pub").is_file())
            self.assertFalse(identity.with_suffix(".pub").exists())
            key = get_private_key(SimpleNamespace(private_keys={}, cwd=None), str(identity), "")
            self.assertIsNotNone(key.public_blob)
            self.assertEqual(key.public_blob.key_type, "ssh-ed25519-cert-v01@openssh.com")


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
        command, stdin = _sudoers_operation(sentinel)
        self.assertNotIn(sentinel, command)
        self.assertNotIn("PYINFRA_SUDO_PASSWORD", command)
        self.assertNotIn("SUDO_ASKPASS", command)
        self.assertIn("sudo -S -k -p ''", command)
        self.assertIn("exec </dev/null", command)
        self.assertNotIn(sentinel, repr(stdin))
        self.assertEqual(stdin.readlines(), [sentinel + "\n"])

    def test_bootstrap_uses_native_debian_operations_and_preserves_primary_group(self) -> None:
        operations = []
        with patch("deploys.lib.bootstrap.apt.packages") as apt_packages, patch(
            "deploys.lib.bootstrap.pacman.packages"
        ) as pacman_packages, patch("deploys.lib.bootstrap.server.user") as user, patch(
            "deploys.lib.bootstrap.files.directory"
        ) as directory, patch("deploys.lib.bootstrap.server.shell") as shell:
            apt_packages.side_effect = lambda **_: operations.append("packages")
            user.side_effect = lambda **_: operations.append("user")
            directory.side_effect = lambda **_: operations.append("home")
            shell.side_effect = lambda **kwargs: operations.append(kwargs["name"])
            _configure_bootstrap(
                "debian",
                {"homelab": {"group": "existing-primary"}},
                self._ca_key(),
                "SENTINEL-password",
            )

        apt_packages.assert_called_once_with(
            name="Install OpenSSH and sudo packages",
            packages=["openssh-server", "sudo"],
            update=True,
            _shell_executable=_SUDO_SHELL,
            _stdin=ANY,
        )
        pacman_packages.assert_not_called()
        user.assert_called_once_with(
            name="Reconcile homelab administrator account",
            user="homelab",
            home="/home/homelab",
            shell="/bin/sh",
            create_home=True,
            ensure_home=False,
            _shell_executable=_SUDO_SHELL,
            _stdin=ANY,
        )
        directory.assert_called_once_with(
            name="Reconcile homelab home directory",
            path="/home/homelab",
            user="homelab",
            group="existing-primary",
            mode="0755",
            _shell_executable=_SUDO_SHELL,
            _stdin=ANY,
        )
        self.assertEqual(shell.call_count, 2)
        sudoers_call, ssh_call = shell.call_args_list
        self.assertEqual(
            operations,
            [
                "packages",
                "user",
                "home",
                "Validate and install homelab sudoers policy",
                "Validate and activate SSH user CA trust",
            ],
        )
        for operation_call in (
            apt_packages.call_args,
            user.call_args,
            directory.call_args,
            sudoers_call,
            ssh_call,
        ):
            self.assertNotIn("SENTINEL-password", repr(operation_call))
            self.assertNotIn("_env", operation_call.kwargs)
            self.assertNotIn("_sudo", operation_call.kwargs)
        self.assertEqual(sudoers_call.kwargs["_stdin"].readlines(), ["SENTINEL-password\n"])
        privileged_calls = (
            apt_packages.call_args,
            user.call_args,
            directory.call_args,
            ssh_call,
        )
        self.assertEqual(
            len({id(operation_call.kwargs["_stdin"]) for operation_call in privileged_calls}),
            len(privileged_calls),
        )
        for operation_call in privileged_calls:
            self.assertEqual(operation_call.kwargs["_shell_executable"], _SUDO_SHELL)
            stdin = operation_call.kwargs["_stdin"]
            self.assertNotIn("SENTINEL-password", repr(stdin))
            self.assertEqual(stdin.readlines(), ["SENTINEL-password\n"])
            self.assertEqual(stdin.readlines(), ["SENTINEL-password\n"])
        self.assertIn("systemctl --quiet is-active ssh.service", ssh_call.kwargs["commands"])

    def test_bootstrap_uses_native_arch_packages_and_predicts_new_user_group(self) -> None:
        with patch("deploys.lib.bootstrap.apt.packages") as apt_packages, patch(
            "deploys.lib.bootstrap.pacman.packages"
        ) as pacman_packages, patch("deploys.lib.bootstrap.server.user") as user, patch(
            "deploys.lib.bootstrap.files.directory"
        ) as directory, patch("deploys.lib.bootstrap.server.shell"):
            _configure_bootstrap("arch linux", {}, self._ca_key(), "password")

        apt_packages.assert_not_called()
        pacman_packages.assert_called_once_with(
            name="Install OpenSSH and sudo packages",
            packages=["openssh", "sudo"],
            update=True,
            _shell_executable=_SUDO_SHELL,
            _stdin=ANY,
        )
        self.assertNotIn("group", user.call_args.kwargs)
        self.assertTrue(user.call_args.kwargs["create_home"])
        self.assertEqual(directory.call_args.kwargs["group"], "homelab")

    def test_bootstrap_rejects_unsupported_distribution_before_operations(self) -> None:
        with patch("deploys.lib.bootstrap.server.shell") as shell:
            with self.assertRaisesRegex(ValueError, "unsupported Linux distribution: fedora"):
                _configure_bootstrap("fedora", {}, self._ca_key(), "password")
        shell.assert_not_called()

    def test_sudoers_installation_is_validated_and_atomic(self) -> None:
        command, _ = _sudoers_operation("password")
        script = shlex.split(command)[-1]
        self.assertIn("visudo -cf", script)
        metadata = "LC_ALL=C stat -c '%F:%u:%g:%a'"
        repair_predicate = "[ \"$metadata\" != 'regular file:0:0:440' ]"
        sudoers_install = 'install -o root -g root -m 0440 "$staging" /etc/sudoers.d/.homelab.tmp.$$'
        self.assertIn(metadata, script)
        self.assertIn(repair_predicate, script)
        self.assertIn('[ -f "$target" ] && cmp -s "$staging" "$target"', script)
        directory_guard = '[ -d "$target" ] && [ ! -L "$target" ]'
        self.assertIn(directory_guard, script)
        self.assertLess(script.index(directory_guard), script.index(sudoers_install))
        self.assertLess(script.index("visudo -cf"), script.index(sudoers_install))
        self.assertLess(script.index(repair_predicate), script.index(sudoers_install))
        self.assertIn('mv -fT /etc/sudoers.d/.homelab.tmp.$$ "$target"', script)

    def test_ssh_transaction_validates_candidate_and_final_before_conditional_reload(self) -> None:
        for service in ("ssh", "sshd"):
            command = _ssh_trust_command(self._ca_key(), service)
            candidate_validation = 'sshd -t -f "$candidate"'
            final_validation = "sshd -t;"
            producer = 'exec sshd -T >"$effective_fifo"'
            bounded_capture = 'head -c 131073 >"$staging/sshd.effective"'
            producer_wait = 'if wait "$producer_pid"; then producer_status=0'
            producer_check = '[ "$producer_status" -eq 0 ]'
            size_check = '[ "$effective_size" -le 131072 ]'
            effective_check = (
                "grep -Fqx -- 'trustedusercakeys "
                "/etc/ssh/trusted-user-ca-keys.pem'"
            )
            reload_check = f"systemctl --quiet is-active {service}.service"
            ca_install = (
                'install -o root -g root -m 0644 "$staging/user-ca.pub" '
                "/etc/ssh/.trusted-user-ca-keys.pem.tmp.$$"
            )
            self.assertIn("ssh-keygen -l", command)
            self.assertLess(command.index(candidate_validation), command.index(ca_install))
            self.assertLess(command.index(ca_install), command.index(final_validation))
            self.assertLess(command.index(final_validation), command.index(reload_check))
            self.assertLess(command.index(producer), command.index(bounded_capture))
            self.assertLess(command.index(bounded_capture), command.index(producer_wait))
            self.assertLess(command.index(producer_wait), command.index(producer_check))
            self.assertLess(command.index(producer_check), command.index(size_check))
            self.assertLess(command.index(size_check), command.index(effective_check))
            self.assertLess(command.index(effective_check), command.index(reload_check))
            self.assertIn("cat >/dev/null", command)
            self.assertIn('kill "$producer_pid"', command)
            self.assertIn('wait "$producer_pid" 2>/dev/null', command)
            self.assertIn("mkfifo \"$effective_fifo\"", command)
            self.assertIn(f"systemctl reload {service}.service", command)
            self.assertIn("mv -fT /etc/ssh/.trusted-user-ca-keys.pem.tmp.$$", command)
            self.assertIn("ca_metadata=$(LC_ALL=C stat -c '%F:%u:%g:%a'", command)
            self.assertIn("[ \"$ca_metadata\" != 'regular file:0:0:644' ]", command)
            self.assertIn("drop_in_metadata=$(LC_ALL=C stat -c '%F:%u:%g:%a'", command)
            self.assertIn("[ \"$drop_in_metadata\" != 'regular file:0:0:644' ]", command)
            ca_directory_guard = '[ -d "$ca_target" ] && [ ! -L "$ca_target" ]'
            drop_in_directory_guard = (
                '[ -d "$drop_in_target" ] && [ ! -L "$drop_in_target" ]'
            )
            self.assertLess(command.index(ca_directory_guard), command.index(ca_install))
            self.assertLess(command.index(drop_in_directory_guard), command.index(ca_install))
            self.assertIn(
                'if [ "$ca_content_matches" -ne 1 ]; then content_changed=1',
                command,
            )
            self.assertIn(
                'if [ "$drop_in_content_matches" -ne 1 ]; then content_changed=1',
                command,
            )
            self.assertNotIn('if [ "$content_changed" -eq 1 ]; then sshd -t', command)
            self.assertNotIn("sudo -S", command)
            self.assertNotIn("visudo", command)
            self.assertNotIn("useradd", command)
            self.assertNotIn("apt-get", command)
            self.assertNotIn("pacman", command)


class SystemInfoTests(unittest.TestCase):
    def test_command_is_bounded_and_avoids_sensitive_sources(self) -> None:
        self.assertIn("head -c 32768", _SYSTEM_INFO_COMMAND)
        self.assertIn("HOMELAB_SYSTEM_INFO_V1_BEGIN", _SYSTEM_INFO_COMMAND)
        self.assertIn("emit uptime uptime -p", _SYSTEM_INFO_COMMAND)
        for forbidden in ("/proc/", "ps ", "env", "printenv", "journalctl"):
            self.assertNotIn(forbidden, _SYSTEM_INFO_COMMAND)

    def test_exact_command_completes_with_protocol_markers_under_sh(self) -> None:
        result = subprocess.run(
            ["sh", "-c", _SYSTEM_INFO_COMMAND],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
        self.assertTrue(result.stdout.startswith(b"HOMELAB_SYSTEM_INFO_V1_BEGIN\n"))
        self.assertTrue(result.stdout.endswith(b"HOMELAB_SYSTEM_INFO_V1_END\n"))

    def test_output_callback_exports_only_the_system_info_operation_stdout(self) -> None:
        host = object()

        class State:
            inventory = (host,)

            def __init__(
                self,
                name: str,
                stdout: str = "INTENDED_STDOUT",
                succeeded: bool = True,
            ) -> None:
                self.name = name
                self.stdout = stdout
                self.succeeded = succeeded

            def get_op_meta(self, op_hash):
                return SimpleNamespace(names={self.name})

            def get_op_data_for_host(self, selected_host, op_hash):
                if selected_host is not host:
                    raise AssertionError("callback selected an unexpected host")
                return SimpleNamespace(
                    operation_meta=SimpleNamespace(
                        did_succeed=lambda: self.succeeded,
                        stdout=self.stdout,
                        stderr="SENTINEL_STDERR",
                    )
                )

        output = io.StringIO()
        with patch("sys.stdout", output):
            _SystemInfoOutputCallback.operation_end(State("other operation"), "other")
            _SystemInfoOutputCallback.operation_end(
                State(
                    _SYSTEM_INFO_METADATA_NAME,
                    "SENTINEL_FAILED_STDOUT",
                    succeeded=False,
                ),
                "failed-system-info",
            )
            _SystemInfoOutputCallback.operation_end(
                State(_SYSTEM_INFO_METADATA_NAME), "system-info"
            )
            _SystemInfoOutputCallback.operation_end(
                State(_SYSTEM_INFO_METADATA_NAME, "ALREADY_TERMINATED\n"),
                "system-info",
            )

        self.assertEqual(output.getvalue(), "INTENDED_STDOUT\nALREADY_TERMINATED\n")

    def test_pinned_pyinfra_cli_exports_parseable_system_info_stdout(self) -> None:
        result = subprocess.run(
            [
                str(Path(sys.executable).with_name("pyinfra")),
                "--yes",
                "@local",
                "deploys/entrypoints/system_info.py",
            ],
            cwd=ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
        begin = b"HOMELAB_SYSTEM_INFO_V1_BEGIN\n"
        end = b"HOMELAB_SYSTEM_INFO_V1_END\n"
        self.assertTrue(result.stdout.startswith(begin))
        self.assertTrue(result.stdout.endswith(end))
        payload = result.stdout[len(begin) : -len(end)]
        for section in (
            "hostname",
            "uptime",
            "boot_time",
            "os_release",
            "kernel_arch",
            "cpu",
            "memory",
            "filesystems",
            "block_devices",
            "interfaces",
            "default_routes",
        ):
            section_begin = f"--- {section} BEGIN ---\n".encode()
            section_end = f"\n--- {section} END ---\n".encode()
            self.assertTrue(payload.startswith(section_begin))
            _, separator, payload = payload[len(section_begin) :].partition(section_end)
            self.assertEqual(separator, section_end)
        self.assertEqual(payload, b"")


if __name__ == "__main__":
    unittest.main()
