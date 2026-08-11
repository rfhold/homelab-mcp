# Tekton Tools

These documents define the locally implemented contracts for bounded Tekton and Pipelines as Code access. Runtime code and deployment declarations exist in the worktree; they have not been deployed or verified end to end.

| Document | Covers |
| --- | --- |
| [Shared contract](spec/common.md) | Tool surfaces, authorization, identities, output policy, errors, and evidence status. |
| [Repositories and workflows](spec/repositories-workflows.md) | PAC repository authority, Forgejo file discovery, workflow enumeration, and bounds. |
| [Runs, tasks, and logs](spec/runs-tasks-logs.md) | Newest-run lookup, failed-run diagnosis, bounded run waits, owned resource reads, normalized output, log limits, and confidentiality. |
| [Mutations](spec/mutations.md) | Dispatch, rerun, cancellation, fixed destinations, and uncertain outcomes. |
| [Deployment](../operations/deployment.md#tekton-access) | Declared credentials, workload identity, RBAC, network destinations, and approval boundaries. |
| [Testing](../quality/testing.md#tekton-contract-coverage) | Current and required local, mock, deployment, and live evidence. |
