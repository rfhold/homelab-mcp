# Kubernetes Tools

These documents define the locally implemented contract for bounded Kubernetes access. Runtime code, OAuth wiring, Pulumi declarations, container support, and local tests exist. Deployment, browser, live-cluster, and effective-RBAC evidence remain pending. Production remains unapplied.

| Document | Covers |
| --- | --- |
| [Shared contract](spec/common.md) | Tool surfaces, authorization, cluster authority, limits, output policy, errors, and exclusions. |
| [Queries and resources](spec/queries-resources.md) | Query actions, supported resource kinds, selectors, normalization, and truncation. |
| [Mutations](spec/mutations.md) | Exact-object restart, scale, CronJob, and pod-delete actions, dry-run behavior, and uncertain outcomes. |
| [Deployment and RBAC](spec/deployment-rbac.md) | Cluster catalog configuration, runtime credentials, exact RBAC, and bootstrap separation. |
| [Service deployment](../operations/deployment.md#kubernetes-access) | Repository deployment status, credential delivery boundaries, and approval gates. |
| [Testing](../quality/testing.md#kubernetes-contract-coverage) | Required local, declaration, preview, and live evidence. |
