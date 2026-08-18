import json
import os
import re
from getpass import getpass
from ipaddress import ip_address
from pathlib import Path
from typing import Any

from deploys.lib.password_auth import AgentFirstAuthStrategy, LazySecret
from deploys.lib.ssh_credentials import validate_ed25519_public_key


_HOST_KEYS = {"address", "user", "port", "ssh_key", "known_hosts"}
_BOOTSTRAP_HOST_KEYS = {"address", "user", "port", "known_hosts"}
_DNS_LABEL = re.compile(r"^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$")
_SAFE_USER = re.compile(r"^[a-z_][a-z0-9_-]{0,31}$")


def _load_object() -> dict[str, Any]:
    raw = os.environ.get("HOMELAB_INVENTORY_JSON")
    if raw is None:
        raise ValueError("HOMELAB_INVENTORY_JSON is required")
    if len(raw.encode()) > 16_384:
        raise ValueError("inventory JSON exceeds 16384 bytes")
    value = json.loads(raw)
    if not isinstance(value, dict):
        raise ValueError("inventory must be a JSON object")
    return value


def _validate_path(value: Any, field: str) -> Path:
    if not isinstance(value, str):
        raise ValueError(f"{field} must be a string")
    path = Path(value)
    if not path.is_absolute() or not path.is_file():
        raise ValueError(f"{field} must name an existing absolute file")
    return path


def _validate_address(value: Any) -> str:
    if not isinstance(value, str) or not value or len(value) > 253:
        raise ValueError("host.address must be a valid bounded DNS name or IP address")
    try:
        ip_address(value)
        return value
    except ValueError:
        pass
    if ":" in value or all(character.isdigit() or character == "." for character in value):
        raise ValueError("host.address must be a valid bounded DNS name or IP address")
    name = value[:-1] if value.endswith(".") else value
    if not name or any(not _DNS_LABEL.fullmatch(label) for label in name.split(".")):
        raise ValueError("host.address must be a valid bounded DNS name or IP address")
    return value


def _host(value: Any) -> tuple[str, dict[str, Any]]:
    if not isinstance(value, dict) or set(value) != _HOST_KEYS:
        raise ValueError(f"host must contain exactly {sorted(_HOST_KEYS)}")
    address = _validate_address(value["address"])
    user = value["user"]
    port = value["port"]
    if not isinstance(user, str) or not _SAFE_USER.fullmatch(user):
        raise ValueError("host.user is invalid")
    if not isinstance(port, int) or isinstance(port, bool) or not 1 <= port <= 65535:
        raise ValueError("host.port must be an integer from 1 through 65535")
    key = _validate_path(value["ssh_key"], "host.ssh_key")
    known_hosts = _validate_path(value["known_hosts"], "host.known_hosts")
    return address, {
        "ssh_user": user,
        "ssh_port": port,
        "ssh_key": str(key),
        "ssh_known_hosts_file": str(known_hosts),
        "ssh_strict_host_key_checking": "yes",
    }


def _bootstrap_host(value: Any, separate_sudo_password: bool) -> tuple[str, dict[str, Any]]:
    if not isinstance(value, dict) or set(value) != _BOOTSTRAP_HOST_KEYS:
        raise ValueError(f"bootstrap host must contain exactly {sorted(_BOOTSTRAP_HOST_KEYS)}")
    address = _validate_address(value["address"])
    user = value["user"]
    port = value["port"]
    if not isinstance(user, str) or not _SAFE_USER.fullmatch(user):
        raise ValueError("host.user is invalid")
    if not isinstance(port, int) or isinstance(port, bool) or not 1 <= port <= 65535:
        raise ValueError("host.port must be an integer from 1 through 65535")
    known_hosts = _validate_path(value["known_hosts"], "host.known_hosts")
    ssh_secret = LazySecret(
        lambda: getpass(f"SSH password for {user}@{address}: "),
        "SSH password must not be empty",
    )
    if separate_sudo_password:
        sudo_secret = LazySecret(
            lambda: getpass(f"sudo password for {user}@{address}: "),
            "sudo password must not be empty",
        )
    else:
        sudo_secret = LazySecret(
            lambda: ssh_secret.reveal()
            if ssh_secret.is_set
            else getpass(f"sudo password for {user}@{address}: "),
            "sudo password must not be empty",
        )
    return address, {
        "ssh_user": user,
        "ssh_port": port,
        "ssh_allow_agent": False,
        "ssh_look_for_keys": False,
        "ssh_config_file": "/dev/null",
        "ssh_paramiko_connect_kwargs": {
            "auth_strategy": AgentFirstAuthStrategy(user, ssh_secret.reveal),
        },
        "ssh_known_hosts_file": str(known_hosts),
        "ssh_strict_host_key_checking": "yes",
        "homelab_sudo_password": sudo_secret,
    }


def system_info_inventory() -> list[tuple[str, dict[str, Any]]]:
    value = _load_object()
    if set(value) != {"version", "host"} or value["version"] != 1:
        raise ValueError("system-info inventory requires only version 1 and host")
    address, data = _host(value["host"])
    return [(address, data)]


def bootstrap_inventory() -> list[tuple[str, dict[str, Any]]]:
    value = _load_object()
    expected = {"version", "host", "ca_public_key_file", "separate_sudo_password"}
    if set(value) != expected or value["version"] != 1:
        raise ValueError(
            "bootstrap inventory requires version 1, host, ca_public_key_file, "
            "and separate_sudo_password"
        )
    separate_sudo_password = value["separate_sudo_password"]
    if not isinstance(separate_sudo_password, bool):
        raise ValueError("separate_sudo_password must be a boolean")
    ca_path = _validate_path(value["ca_public_key_file"], "ca_public_key_file")
    ca_public_key = validate_ed25519_public_key(ca_path.read_bytes())
    address, data = _bootstrap_host(value["host"], separate_sudo_password)
    data["homelab_ca_public_key"] = ca_public_key
    return [(address, data)]
