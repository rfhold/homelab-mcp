from io import StringIO

from pyinfra import host
from pyinfra.api import deploy
from pyinfra.facts.server import LinuxDistribution
from pyinfra.operations import apt, pacman, server


_SSHD_DROP_IN = "TrustedUserCAKeys /etc/ssh/trusted-user-ca-keys.pem\n"
_SUDOERS = "homelab ALL=(ALL:ALL) NOPASSWD: ALL\n"


class _ReusableInput(StringIO):
    def readlines(self, hint: int = -1) -> list[str]:
        self.seek(0)
        return super().readlines(hint)


@deploy("Bootstrap homelab administrator")
def bootstrap_homelab() -> None:
    distro = host.get_fact(LinuxDistribution).get("name", "").lower()
    if distro in {"debian", "ubuntu"}:
        apt.packages(
            name="Install sudo and OpenSSH server",
            packages=["openssh-server", "sudo"],
            update=True,
            cache_time=3600,
            _sudo=True,
        )
    elif distro in {"arch", "arch linux"}:
        pacman.packages(
            name="Install sudo and OpenSSH",
            packages=["openssh", "sudo"],
            update=True,
            _sudo=True,
        )
    else:
        raise ValueError(f"unsupported Linux distribution: {distro or 'unknown'}")

    server.user(
        name="Create homelab administrator",
        user="homelab",
        home="/home/homelab",
        shell="/bin/sh",
        create_home=True,
        _sudo=True,
    )
    ca_public_key = host.data.homelab_ca_public_key
    sudoers_size = len(_SUDOERS.encode("ascii"))
    ca_size = len(ca_public_key.encode("ascii"))
    sshd_drop_in_size = len(_SSHD_DROP_IN.encode("ascii"))
    server.shell(
        name="Validate and activate bootstrap security configuration",
        commands=(
            "set -eu; umask 077; staging=; "
            "trap '[ -z \"$staging\" ] || rm -rf \"$staging\"' 0 HUP INT TERM; "
            "staging=$(mktemp -d /tmp/homelab-mcp.XXXXXXXXXX); "
            "candidate=$staging/sshd_config; changed=0; "
            f"dd bs=1 count={sudoers_size} of=\"$staging/sudoers\" 2>/dev/null; "
            f"dd bs=1 count={ca_size} of=\"$staging/user-ca.pub\" 2>/dev/null; "
            f"dd bs=1 count={sshd_drop_in_size} of=\"$staging/user-ca.conf\" 2>/dev/null; "
            f"[ \"$(wc -c <\"$staging/sudoers\")\" -eq {sudoers_size} ]; "
            f"[ \"$(wc -c <\"$staging/user-ca.pub\")\" -eq {ca_size} ]; "
            f"[ \"$(wc -c <\"$staging/user-ca.conf\")\" -eq {sshd_drop_in_size} ]; "
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
        ),
        _stdin=_ReusableInput(_SUDOERS + ca_public_key + _SSHD_DROP_IN),
        _sudo=True,
        _timeout=30,
    )
