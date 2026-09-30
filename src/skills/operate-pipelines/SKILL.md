---
name: operate-pipelines
description: Inspect Tekton repositories, workflows, PipelineRuns, TaskRuns, and logs, then dispatch, rerun, or cancel only explicitly authorized exact workflows or runs.
---
# Operate Pipelines

1. Read the [action reference](references/actions.md) and tool schemas.
2. Discover repositories and workflows, then inspect bounded runs, status, tasks, and logs under the user's read intent.
3. Report the exact discovered identities and diagnosis. A failed run does not authorize a rerun, dispatch, or cancellation.
4. Require an explicit decision naming the exact repository/workflow/ref/parameters or namespace-qualified run before a consequential action.
5. Submit once. Never retry mutations automatically; inspect runs and status after acceptance or unknown outcomes before another decision.

Skill discovery and technical access do not expand user authority.
