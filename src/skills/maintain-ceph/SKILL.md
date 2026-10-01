---
name: maintain-ceph
description: Inspect native Ceph health, metrics, OSDs, devices, flags, and tasks; perform only explicitly authorized exact-OSD maintenance with fresh destructive safety checks.
---
# Maintain Ceph

1. Read the [action reference](references/actions.md) and resource templates/tool schemas.
2. Discover configured clusters, then inspect health, flags, OSDs, devices, and tasks under the user's read intent.
3. Report the exact cluster and OSD, current state, safety evidence, and risks. Investigation does not authorize maintenance.
4. Require an explicit decision for the exact OSD action and parameters. Destruction and purge additionally require the exact confirmation string and a fresh safe-to-destroy check; safety is not permission.
5. Submit once and inspect OSD state, health, and tasks. Never retry mutations automatically. Unknown outcomes require inspection before any new decision.

These tools do not provide general cluster flag writes or arbitrary Ceph commands.
