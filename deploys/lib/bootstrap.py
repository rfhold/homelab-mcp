from io import StringIO
import shlex

from pyinfra import host
from pyinfra.api import deploy
from pyinfra.facts.server import LinuxDistribution
from pyinfra.operations import server


_SSHD_DROP_IN = "TrustedUserCAKeys /etc/ssh/trusted-user-ca-keys.pem\n"
_SUDOERS = "homelab ALL=(ALL:ALL) NOPASSWD: ALL\n"


class _ReusableInput(StringIO):
    def readlines(self, hint: int = -1) -> list[str]:
        self.seek(0)
        return super().readlines(hint)


def _bootstrap_command(distro: str, ca_public_key: str) -> str:
    if distro in {"debian", "ubuntu"}:
        packages = (
            "export DEBIAN_FRONTEND=noninteractive; "
            "apt-get update; apt-get install -y openssh-server sudo"
        )
    elif distro in {"arch", "arch linux"}:
        packages = "pacman -Sy --noconfirm --needed openssh sudo"
    else:
        raise ValueError(f"unsupported Linux distribution: {distro or 'unknown'}")

    sudoers = shlex.quote(_SUDOERS)
    ca_key = shlex.quote(ca_public_key)
    sshd_drop_in = shlex.quote(_SSHD_DROP_IN)
    script = (
        "set -eu; "
        f"{packages}; "
        "if id -u homelab >/dev/null 2>&1; then "
        "homelab_group=$(id -gn homelab); usermod -d /home/homelab -s /bin/sh homelab; "
        "elif getent group homelab >/dev/null 2>&1; then "
        "homelab_group=homelab; useradd -m -d /home/homelab -s /bin/sh -g homelab homelab; "
        "else useradd -m -d /home/homelab -s /bin/sh homelab; "
        "homelab_group=$(id -gn homelab); fi; "
        "install -d -o homelab -g \"$homelab_group\" -m 0755 /home/homelab; "
        "umask 077; staging=; "
        "trap '[ -z \"$staging\" ] || rm -rf \"$staging\"' 0 HUP INT TERM; "
        "staging=$(mktemp -d /tmp/homelab-mcp.XXXXXXXXXX); "
        "candidate=$staging/sshd_config; changed=0; "
        f"printf %s {sudoers} >\"$staging/sudoers\"; "
        f"printf %s {ca_key} >\"$staging/user-ca.pub\"; "
        f"printf %s {sshd_drop_in} >\"$staging/user-ca.conf\"; "
        "visudo -cf \"$staging/sudoers\" >/dev/null; "
        "if ! cmp -s \"$staging/sudoers\" /etc/sudoers.d/homelab 2>/dev/null; then "
        "install -o root -g root -m 0440 \"$staging/sudoers\" /etc/sudoers.d/homelab; fi; "
        "sed \"s#/etc/ssh/trusted-user-ca-keys.pem#$staging/user-ca.pub#\" "
        "\"$staging/user-ca.conf\" >\"$staging/user-ca.validate.conf\"; "
        "ssh-keygen -l -f \"$staging/user-ca.pub\" >/dev/null; "
        "cat /etc/ssh/sshd_config >\"$candidate\"; "
        "printf '\\nInclude %s/user-ca.validate.conf\\n' \"$staging\" >>\"$candidate\"; "
        "sshd -t -f \"$candidate\"; "
        "if ! cmp -s \"$staging/user-ca.pub\" /etc/ssh/trusted-user-ca-keys.pem 2>/dev/null; then "
        "install -o root -g root -m 0644 \"$staging/user-ca.pub\" /etc/ssh/trusted-user-ca-keys.pem; changed=1; fi; "
        "if ! cmp -s \"$staging/user-ca.conf\" /etc/ssh/sshd_config.d/90-homelab-user-ca.conf 2>/dev/null; then "
        "install -d -o root -g root -m 0755 /etc/ssh/sshd_config.d; "
        "install -o root -g root -m 0644 \"$staging/user-ca.conf\" /etc/ssh/sshd_config.d/90-homelab-user-ca.conf; changed=1; fi; "
        "if [ \"$changed\" -eq 1 ]; then sshd -t; "
        "if systemctl --quiet is-active sshd.service; then systemctl reload sshd.service; "
        "elif systemctl --quiet is-active ssh.service; then systemctl reload ssh.service; fi; fi"
    )
    return f"sudo -S -k -p '' -- sh -c {shlex.quote('exec </dev/null; ' + script)}"


def _bootstrap_operation(
    distro: str, ca_public_key: str, sudo_password: str
) -> tuple[str, _ReusableInput]:
    return _bootstrap_command(distro, ca_public_key), _ReusableInput(sudo_password + "\n")


@deploy("Bootstrap homelab administrator")
def bootstrap_homelab() -> None:
    sudo_password = host.data.homelab_sudo_password.reveal()
    distro = host.get_fact(LinuxDistribution).get("name", "").lower()
    ca_public_key = host.data.homelab_ca_public_key
    command, stdin = _bootstrap_operation(distro, ca_public_key, sudo_password)
    server.shell(
        name="Bootstrap and activate homelab security configuration",
        commands=command,
        _stdin=stdin,
        _timeout=300,
    )
