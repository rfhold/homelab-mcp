---
name: operate-kubernetes
description: Inspect configured Kubernetes clusters, supported resources, and bounded Pod logs; perform curated exact-object workload, CronJob, or Pod changes only after an explicit decision.
---
# Operate Kubernetes

1. Read the [action reference](references/actions.md) and tool schemas.
2. Discover configured clusters and kind capabilities, then inspect bounded resources and exact Pod logs under the user's investigation intent.
3. Identify the exact cluster, kind, namespace, name, and relevant current state. Read access or a diagnosis does not authorize a restart, scale, trigger, suspend, or deletion.
4. Require explicit authorization for one exact object and requested change. A dry run does not authorize real execution.
5. Execute once, then inspect current resource state and events. Never automatically retry mutations; inspect unknown outcomes before another explicit decision.

No arbitrary manifests, shell execution, or permission redesign are available through these tools.
