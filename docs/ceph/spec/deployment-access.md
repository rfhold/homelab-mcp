# Ceph Dashboard Deployment and Access Contract

## Status

This document defines implemented preview-only Pulumi declarations and external prerequisites. Local Pulumi build and mock-test evidence covers the declarations. An authorized targeted preview apply seeded four Pulumi Stashes from the Rook-generated shared Dashboard administrator credentials. Pulumi also updated the two Kubernetes provider state records from the pipeline kubeconfig path to the local kubeconfig path. The apply did not update the application Secret, Deployment, or another Kubernetes resource. No Dashboard account creation, authenticated Ceph read, live Ceph mutation, or preview rollout has occurred. Production configuration explicitly disables Ceph through an empty catalog; no production Ceph resource or apply has occurred.

## Dashboard Destinations and Accounts

The preview catalog declares `pantheon` at `https://ceph.pantheon.holdenitdown.net` and `romulus` at `https://ceph.romulus.holdenitdown.net`. Each entry requires Ceph major release 19. The runtime validates fixed HTTPS origins, uses normal certificate verification, disables redirects, and sends credentials only to the selected configured origin.

Existing authenticated TLS routes already provide the required network entry points. This feature requires no Ceph monitor exposure, new public Dashboard route, or change to sibling homelab routes. It does not add direct Ceph CLI or monitor networking.

Preview currently uses each cluster's Rook-generated shared Dashboard `admin` credential under an explicit user-approved exception. The two clusters retain different passwords, but each credential has broader permissions than the fixed MCP catalog. Before production approval, an authorized cluster operator must replace them with separate dedicated Dashboard users. Each replacement account must have only the permissions required by the fixed query and mutation catalogs.

## Credential Delivery

Pulumi preview declarations define four Stashes, with separate username and password seeds for each cluster. `CEPH_DASHBOARD_PANTHEON_USERNAME`, `CEPH_DASHBOARD_PANTHEON_PASSWORD`, `CEPH_DASHBOARD_ROMULUS_USERNAME`, and `CEPH_DASHBOARD_ROMULUS_PASSWORD` provide operator-controlled seed input. Only Stash outputs reach the runtime application Secret. Seed values do not enter stack configuration, rendered documentation, logs, image layers, or repository files.

The runtime will map each projected credential only to its matching fixed Dashboard origin. Credential creation, retrieval, seeding, rotation, revocation, and deletion remain external target-specific actions; declarations do not grant authority to perform them.

## Preview and Production Boundary

Only the preview stack declares Ceph configuration, Stashes, Secret projection, runtime variables, and fixed egress for this integration. Production Ceph declarations remain disabled, and the production stack remains unapplied.

The existing preview service route and hosted OAuth routes do not change. Ceph traffic is outbound from the MCP runtime to the two existing authenticated Dashboard HTTPS routes through the deployment's existing TCP 443 egress. The feature does not add Ceph monitor ports or sibling route changes.

## Independent Approval Gates

Passing one gate does not authorize a later gate.

| Gate | Required authority and evidence |
| --- | --- |
| Dashboard account creation | No account was created for preview; the Stashes use Rook's shared administrator credentials. Separate authorization remains required before creating replacement dedicated users. |
| Local Pulumi apply | The authorized targeted apply created only four Ceph Stashes and updated two provider state records. A full preview apply remains separately gated. |
| Authenticated live reads | Separate authorization for the exact preview deployment and cluster before calling Ceph reads. Start with `cluster.list`, then authorize cluster-backed reads independently from deployment. |
| Representative live mutations | Separate authorization for every individual live mutation call, including its cluster, action, OSD target, requested value, and recovery observation. Approval for one `osd.mark`, `osd.reweight`, `osd.scrub`, `osd.destroy`, or `osd.purge` does not authorize another. |

Destroy and purge authorization must follow a fresh safe-to-destroy result and must include the exact confirmation string. For a synchronous completed result, validation must inspect native state. For an accepted HTTP 202 result, validation must preserve the task identity and inspect task and native state before any follow-up.

No repository plan, declaration, test, pipeline, authenticated session, or prior read grants any of these external permissions.
