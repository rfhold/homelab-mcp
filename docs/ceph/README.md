# Ceph Dashboard Tools

These documents define the locally implemented contract for bounded Ceph 19 Squid Dashboard access in the Pantheon and Romulus preview clusters. Rust runtime code, MCP registration and tests, and preview-only Pulumi declarations exist. An authorized targeted preview apply seeded four Pulumi Stashes from the Rook-generated shared Dashboard administrator credentials; it did not update the application Secret or Deployment. No Dashboard account creation, authenticated Ceph read, live Ceph mutation, or preview rollout has occurred. Production configuration explicitly disables Ceph through an empty catalog; no production Ceph resource or apply has occurred.

| Document | Covers |
| --- | --- |
| [Shared contract](spec/common.md) | Tool surfaces, authorization risk, cluster authority, request boundaries, normalized output, errors, and exclusions. |
| [Queries](spec/queries.md) | Cluster, status, current metrics, OSD, device, flag, and asynchronous task reads. |
| [Mutations](spec/mutations.md) | Five curated OSD changes, destructive confirmations, safe-to-destroy checks, synchronous completion, task identities, and uncertain outcomes. |
| [Deployment and access](spec/deployment-access.md) | Dashboard accounts, HTTPS destinations, Pulumi Stash delivery, preview declarations, networking, and approval gates. |
| [Service deployment](../operations/deployment.md#ceph-dashboard-access) | Repository-wide deployment status and delivery boundary. |
| [Testing](../quality/testing.md#ceph-dashboard-contract-coverage) | Required local, declaration, authenticated preview, and live mutation evidence. |
