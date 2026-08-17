import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from deploys.lib.inventory import bootstrap_inventory, system_info_inventory
from deploys.lib.bootstrap import _SSHD_DROP_IN, _SUDOERS
from deploys.lib.system_info import _SYSTEM_INFO_COMMAND


ROOT = Path(__file__).parents[2]


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

    def test_system_info_accepts_only_fixed_shape(self) -> None:
        with tempfile.NamedTemporaryFile() as key, tempfile.NamedTemporaryFile() as known_hosts:
            value = self._base(key.name, known_hosts.name)
            with patch.dict(os.environ, {"HOMELAB_INVENTORY_JSON": json.dumps(value)}):
                address, data = system_info_inventory()[0]
            self.assertEqual(address, "node.example")
            self.assertEqual(data["ssh_known_hosts_file"], known_hosts.name)
            self.assertEqual(data["ssh_strict_host_key_checking"], "yes")
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
            ca.write("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITest ca\n")
            ca.flush()
            value = self._base(key.name, known_hosts.name) | {"ca_public_key_file": ca.name}
            with patch.dict(os.environ, {"HOMELAB_INVENTORY_JSON": json.dumps(value)}):
                data = bootstrap_inventory()[0][1]
            self.assertTrue(data["homelab_ca_public_key"].startswith("ssh-ed25519 "))
            self.assertEqual(data["ssh_known_hosts_file"], known_hosts.name)
            self.assertEqual(data["ssh_strict_host_key_checking"], "yes")

    def test_bootstrap_rejects_private_material(self) -> None:
        with tempfile.NamedTemporaryFile() as key, tempfile.NamedTemporaryFile() as known_hosts, tempfile.NamedTemporaryFile(mode="w+") as ca:
            ca.write("-----BEGIN OPENSSH " + "PRIVATE KEY-----\n")
            ca.flush()
            value = self._base(key.name, known_hosts.name) | {"ca_public_key_file": ca.name}
            with patch.dict(os.environ, {"HOMELAB_INVENTORY_JSON": json.dumps(value)}):
                with self.assertRaises(ValueError):
                    bootstrap_inventory()


class SystemInfoTests(unittest.TestCase):
    def test_command_is_bounded_and_avoids_sensitive_sources(self) -> None:
        self.assertIn("head -c 32768", _SYSTEM_INFO_COMMAND)
        self.assertIn("HOMELAB_SYSTEM_INFO_V1_BEGIN", _SYSTEM_INFO_COMMAND)
        self.assertIn("emit uptime uptime -p", _SYSTEM_INFO_COMMAND)
        for forbidden in ("/proc/", "ps ", "env", "printenv", "journalctl"):
            self.assertNotIn(forbidden, _SYSTEM_INFO_COMMAND)


if __name__ == "__main__":
    unittest.main()
