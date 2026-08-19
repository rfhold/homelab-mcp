import sys

from pyinfra.api import BaseStateCallback, deploy
from pyinfra.operations import server


_SYSTEM_INFO_OPERATION = "Collect bounded system information"
_SYSTEM_INFO_METADATA_NAME = f"{_SYSTEM_INFO_OPERATION} | {_SYSTEM_INFO_OPERATION}"
_SYSTEM_INFO_COMMAND = r"""set -eu
export LC_ALL=C
emit() {
    key=$1
    shift
    printf -- '--- %s BEGIN ---\n' "$key"
    "$@" 2>/dev/null | head -c 32768 || true
    printf -- '\n--- %s END ---\n' "$key"
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


class _SystemInfoOutputCallback(BaseStateCallback):
    @staticmethod
    def operation_end(state, op_hash) -> None:
        if state.get_op_meta(op_hash).names != {_SYSTEM_INFO_METADATA_NAME}:
            return
        hosts = tuple(state.inventory)
        if len(hosts) != 1:
            raise RuntimeError("system-info requires exactly one host")
        operation_meta = state.get_op_data_for_host(hosts[0], op_hash).operation_meta
        if not operation_meta.did_succeed():
            return
        output = operation_meta.stdout
        if output:
            sys.stdout.write(output)
            if not output.endswith("\n"):
                sys.stdout.write("\n")
            sys.stdout.flush()


@deploy(_SYSTEM_INFO_OPERATION)
def system_info() -> None:
    server.shell(
        name=_SYSTEM_INFO_OPERATION,
        commands=_SYSTEM_INFO_COMMAND,
        _timeout=30,
    )
