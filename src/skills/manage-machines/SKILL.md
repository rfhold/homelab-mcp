---
name: manage-machines
description: Discover machine inventory and approved deploys; change exact inventory records, explicit SSH host trust, or run one approved deploy only with target-specific authorization.
---
# Manage Machines

1. Read the [action reference](references/actions.md) and tool schemas.
2. Use `machines.list` and `deploys.list` for discovery under the user's read intent, despite their mixed-tool mutation annotations.
3. Identify the exact machine UUID and approved deploy ID. Inventory updates, deletions, trust changes, and deploy runs each require a separate explicit decision for their exact target and values.
4. Never clear trust to bypass a mismatch. Trust replacement needs independent verification of the intended public key, not a key supplied only by the failing connection.
5. Execute the authorized action once. Never automatically retry mutations or deploy runs; inspect inventory and available outcome evidence before any new decision after an unknown result.

Skills do not supply credentials, host keys, inventory, bootstrap commands, or deploy authority. Technical access is not operational approval.
