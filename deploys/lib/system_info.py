from pyinfra.api import deploy
from pyinfra.operations import server


_SYSTEM_INFO_COMMAND = r"""set -eu
export LC_ALL=C
emit() {
    key=$1
    shift
    printf '--- %s BEGIN ---\n' "$key"
    "$@" 2>/dev/null | head -c 32768 || true
    printf '\n--- %s END ---\n' "$key"
}
printf 'HOMELAB_SYSTEM_INFO_V1_BEGIN\n'
emit hostname hostname
emit uptime uptime -p
emit boot_time uptime -s
emit os_release sh -c "grep -E '^(ID|NAME|PRETTY_NAME|VERSION|VERSION_ID)=' /etc/os-release"
emit kernel_arch uname -srm
emit cpu lscpu -J -e=CPU,CORE,SOCKET,ONLINE
emit memory free -b
emit filesystems df -B1 -P -x tmpfs -x devtmpfs
emit block_devices lsblk -J -b -o NAME,TYPE,SIZE,FSTYPE,MOUNTPOINTS
emit interfaces ip -j -details address show
emit default_routes ip -j route show default
printf 'HOMELAB_SYSTEM_INFO_V1_END\n'
"""


@deploy("Collect bounded system information")
def system_info() -> None:
    server.shell(
        name="Collect bounded system information",
        commands=_SYSTEM_INFO_COMMAND,
        _timeout=30,
    )
