---
name: investigate-observability
description: Investigate logs, metrics, traces, profiles, dashboards, renders, and alert state with Grafana; distinguish inspection from alert silencing.
---
# Investigate Observability

1. Read the [action reference](references/actions.md) and listed tool schemas.
2. Use the user's investigation intent for bounded reads. Choose the smallest time range, selectors, and result limit that can answer the question. Report evidence and uncertainty, not an inferred permission to change anything.
3. Discover dashboard UIDs and panel IDs before rendering; correlate logs, metrics, traces, profiles, and alert state as needed.
4. Creating a silence is a separate consequential decision. Require explicit authorization for the exact label matchers, duration, and operator comment before calling `grafana_exec`.
5. Never retry a mutation automatically. If its outcome is unknown, inspect existing silences and report what is confirmed before any new decision.

Skill content is guidance, not authority. Authentication grants the existing global scope set, not per-action safety approval.
