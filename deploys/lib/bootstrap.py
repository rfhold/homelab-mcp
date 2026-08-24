from io import StringIO
import shlex

from pyinfra import host
from pyinfra.api import deploy
from pyinfra.facts.server import LinuxDistribution, Users
from pyinfra.operations import apt, files, pacman, server


_SSHD_DROP_IN = "TrustedUserCAKeys /etc/ssh/trusted-user-ca-keys.pem\n"
_SUDOERS = "homelab ALL=(ALL:ALL) NOPASSWD: ALL\n"
_SUDO_SHELL = "sudo -S -k -p '' -- sh"


class _ReusableInput(StringIO):
    def readlines(self, hint: int = -1) -> list[str]:
        self.seek(0)
        return super().readlines(hint)


def _privileged_arguments(sudo_password: str) -> dict:
    return {
        "_shell_executable": _SUDO_SHELL,
        "_stdin": _ReusableInput(sudo_password + "\n"),
    }


def _sudoers_operation(sudo_password: str) -> tuple[str, _ReusableInput]:
    sudoers = shlex.quote(_SUDOERS)
    script = (
        "set -eu; umask 077; target=/etc/sudoers.d/homelab; "
        "if [ -d \"$target\" ] && [ ! -L \"$target\" ]; then "
        "printf '%s\\n' 'refusing to replace sudoers directory' >&2; exit 1; fi; "
        "staging=$(mktemp /tmp/homelab-sudoers.XXXXXXXXXX); "
        "trap 'rm -f \"$staging\" /etc/sudoers.d/.homelab.tmp.$$' 0 HUP INT TERM; "
        f"printf %s {sudoers} >\"$staging\"; "
        "visudo -cf \"$staging\" >/dev/null; "
        "metadata=$(LC_ALL=C stat -c '%F:%u:%g:%a' -- \"$target\" 2>/dev/null || :); "
        "content_matches=0; "
        "if [ -f \"$target\" ] && cmp -s \"$staging\" \"$target\"; then content_matches=1; fi; "
        "if [ \"$content_matches\" -ne 1 ] || "
        "[ \"$metadata\" != 'regular file:0:0:440' ]; then "
        "install -o root -g root -m 0440 \"$staging\" /etc/sudoers.d/.homelab.tmp.$$; "
        "mv -fT /etc/sudoers.d/.homelab.tmp.$$ \"$target\"; fi"
    )
    command = f"sudo -S -k -p '' -- sh -c {shlex.quote('exec </dev/null; ' + script)}"
    return command, _ReusableInput(sudo_password + "\n")


def _ssh_trust_command(ca_public_key: str, ssh_service: str) -> str:
    ca_key = shlex.quote(ca_public_key)
    sshd_drop_in = shlex.quote(_SSHD_DROP_IN)
    script = (
        "set -eu; "
        "umask 077; staging=; producer_pid=; "
        "trap '[ -z \"$producer_pid\" ] || { "
        "kill \"$producer_pid\" 2>/dev/null || :; "
        "wait \"$producer_pid\" 2>/dev/null || :; }; "
        "[ -z \"$staging\" ] || rm -rf \"$staging\"; "
        "rm -f /etc/ssh/.trusted-user-ca-keys.pem.tmp.$$ "
        "/etc/ssh/sshd_config.d/.90-homelab-user-ca.conf.tmp.$$' 0 HUP INT TERM; "
        "staging=$(mktemp -d /tmp/homelab-mcp.XXXXXXXXXX); "
        "candidate=$staging/sshd_config; content_changed=0; "
        "ca_target=/etc/ssh/trusted-user-ca-keys.pem; "
        "drop_in_target=/etc/ssh/sshd_config.d/90-homelab-user-ca.conf; "
        "if [ -d \"$ca_target\" ] && [ ! -L \"$ca_target\" ]; then "
        "printf '%s\\n' 'refusing to replace SSH CA directory' >&2; exit 1; fi; "
        "if [ -d \"$drop_in_target\" ] && [ ! -L \"$drop_in_target\" ]; then "
        "printf '%s\\n' 'refusing to replace sshd drop-in directory' >&2; exit 1; fi; "
        f"printf %s {ca_key} >\"$staging/user-ca.pub\"; "
        f"printf %s {sshd_drop_in} >\"$staging/user-ca.conf\"; "
        "sed \"s#/etc/ssh/trusted-user-ca-keys.pem#$staging/user-ca.pub#\" "
        "\"$staging/user-ca.conf\" >\"$staging/user-ca.validate.conf\"; "
        "ssh-keygen -l -f \"$staging/user-ca.pub\" >/dev/null; "
        "cat /etc/ssh/sshd_config >\"$candidate\"; "
        "printf '\\nInclude %s/user-ca.validate.conf\\n' \"$staging\" >>\"$candidate\"; "
        "sshd -t -f \"$candidate\"; "
        "ca_metadata=$(LC_ALL=C stat -c '%F:%u:%g:%a' -- \"$ca_target\" 2>/dev/null || :); "
        "ca_content_matches=0; "
        "if [ -f \"$ca_target\" ] && cmp -s \"$staging/user-ca.pub\" \"$ca_target\"; "
        "then ca_content_matches=1; fi; "
        "if [ \"$ca_content_matches\" -ne 1 ] || "
        "[ \"$ca_metadata\" != 'regular file:0:0:644' ]; then "
        "install -o root -g root -m 0644 \"$staging/user-ca.pub\" /etc/ssh/.trusted-user-ca-keys.pem.tmp.$$; "
        "mv -fT /etc/ssh/.trusted-user-ca-keys.pem.tmp.$$ \"$ca_target\"; "
        "if [ \"$ca_content_matches\" -ne 1 ]; then content_changed=1; fi; fi; "
        "drop_in_metadata=$(LC_ALL=C stat -c '%F:%u:%g:%a' -- \"$drop_in_target\" 2>/dev/null || :); "
        "drop_in_content_matches=0; "
        "if [ -f \"$drop_in_target\" ] && "
        "cmp -s \"$staging/user-ca.conf\" \"$drop_in_target\"; "
        "then drop_in_content_matches=1; fi; "
        "if [ \"$drop_in_content_matches\" -ne 1 ] || "
        "[ \"$drop_in_metadata\" != 'regular file:0:0:644' ]; then "
        "install -d -o root -g root -m 0755 /etc/ssh/sshd_config.d; "
        "install -o root -g root -m 0644 \"$staging/user-ca.conf\" /etc/ssh/sshd_config.d/.90-homelab-user-ca.conf.tmp.$$; "
        "mv -fT /etc/ssh/sshd_config.d/.90-homelab-user-ca.conf.tmp.$$ "
        "\"$drop_in_target\"; "
        "if [ \"$drop_in_content_matches\" -ne 1 ]; then content_changed=1; fi; fi; "
        "sshd -t; "
        "effective_fifo=$staging/sshd.effective.fifo; "
        "mkfifo \"$effective_fifo\"; "
        "(LC_ALL=C; export LC_ALL; exec sshd -T >\"$effective_fifo\") & producer_pid=$!; "
        "{ head -c 131073 >\"$staging/sshd.effective\"; cat >/dev/null; } "
        "<\"$effective_fifo\"; "
        "if wait \"$producer_pid\"; then producer_status=0; else producer_status=$?; fi; "
        "producer_pid=; rm -f \"$effective_fifo\"; "
        "[ \"$producer_status\" -eq 0 ]; "
        "effective_size=$(wc -c <\"$staging/sshd.effective\"); "
        "[ \"$effective_size\" -le 131072 ]; "
        "grep -Fqx -- 'trustedusercakeys /etc/ssh/trusted-user-ca-keys.pem' "
        "\"$staging/sshd.effective\"; "
        "if [ \"$content_changed\" -eq 1 ]; then "
        f"if systemctl --quiet is-active {ssh_service}.service; then "
        f"systemctl reload {ssh_service}.service; fi; fi"
    )
    return script


def _configure_bootstrap(
    distro: str, users: dict, ca_public_key: str, sudo_password: str
) -> None:
    if distro in {"debian", "ubuntu"}:
        package_operation = apt.packages
        packages = ["openssh-server", "sudo"]
        ssh_service = "ssh"
    elif distro in {"arch", "arch linux"}:
        package_operation = pacman.packages
        packages = ["openssh", "sudo"]
        ssh_service = "sshd"
    else:
        raise ValueError(f"unsupported Linux distribution: {distro or 'unknown'}")

    package_operation(
        name="Install OpenSSH and sudo packages",
        packages=packages,
        update=True,
        **_privileged_arguments(sudo_password),
    )
    server.user(
        name="Reconcile homelab administrator account",
        user="homelab",
        home="/home/homelab",
        shell="/bin/sh",
        create_home=True,
        ensure_home=False,
        **_privileged_arguments(sudo_password),
    )
    home_group = users.get("homelab", {}).get("group", "homelab")
    files.directory(
        name="Reconcile homelab home directory",
        path="/home/homelab",
        user="homelab",
        group=home_group,
        mode="0755",
        **_privileged_arguments(sudo_password),
    )
    sudoers_command, sudoers_stdin = _sudoers_operation(sudo_password)
    server.shell(
        name="Validate and install homelab sudoers policy",
        commands=sudoers_command,
        _stdin=sudoers_stdin,
        _timeout=30,
    )
    server.shell(
        name="Validate and activate SSH user CA trust",
        commands=_ssh_trust_command(ca_public_key, ssh_service),
        _timeout=300,
        **_privileged_arguments(sudo_password),
    )


@deploy("Bootstrap homelab administrator")
def bootstrap_homelab() -> None:
    distro = host.get_fact(LinuxDistribution).get("name", "").lower()
    users = host.get_fact(Users)
    _configure_bootstrap(
        distro,
        users,
        host.data.homelab_ca_public_key,
        host.data.homelab_sudo_password.reveal(),
    )
