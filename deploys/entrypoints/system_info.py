from pyinfra import state

from deploys.lib.system_info import _SystemInfoOutputCallback, system_info


state.add_callback_handler(_SystemInfoOutputCallback())
system_info()
