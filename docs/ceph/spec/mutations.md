# Ceph Dashboard Mutation Specifications

## Shared Mutation Contract

`ceph_exec` exposes only the five typed OSD actions in this document. Every call requires an exact configured cluster and action-specific typed fields. The service makes one Dashboard attempt and never retries automatically.

A caller cannot supply a Ceph command, Dashboard route, HTTP method, arbitrary request body, force option, or additional action parameter. Dashboard acceptance does not prove that the cluster reached the requested state.

## OSD State and Weight

`osd.mark` targets one exact OSD and accepts only `in`, `out`, or `down`. It does not expose `up`, `lost`, or arbitrary mark values.

`osd.reweight` targets one exact OSD and requires a finite weight from `0` through `1`, inclusive. It does not infer a weight or accept an out-of-range value.

## OSD Scrub

`osd.scrub` targets one exact OSD and accepts only `normal` or `deep`. The caller cannot select a placement group, repair mode, scheduling option, or arbitrary scrub parameter.

## OSD Destroy and Purge

`osd.destroy` and `osd.purge` target one exact OSD. Before either dispatch, the service makes a fresh `osd.safe-to-destroy` Dashboard check for that OSD in the same call. It proceeds only when the normalized result explicitly reports it safe.

Each action requires a case-sensitive `confirmation` string that exactly matches `<action> osd.<id> on <cluster>`. The action value is `destroy` or `purge`. The ID is the validated decimal OSD ID without leading zeroes. The cluster is the exact configured selector. For example, purging OSD 7 in Romulus requires `purge osd.7 on romulus`. The service rejects whitespace, case, cluster, action, or OSD mismatches before mutation dispatch.

Neither action exposes or uses force. `osd.destroy` retains the OSD identity for a later authorized replacement workflow. `osd.purge` removes the OSD identity. Physical device replacement remains outside this feature.

## Completion and HTTP 202

Ceph 19 Squid implements the five current OSD mutations as synchronous Dashboard controller operations. A successful synchronous response reports `status: completed`. For `osd.mark` and `osd.reweight`, the service returns that result immediately after HTTP 200 without issuing a follow-up state query, so completion does not claim that the cluster has already converged on the requested state.

If a Dashboard returns HTTP 202, the service accepts the response only when it contains a safe normalized task identity. The result reports accepted asynchronous status and that identity for `task.list` follow-up. HTTP 202 without a safe task identity returns non-retryable `mutation_outcome_unknown` because dispatch can already have occurred.

## Deferred Cluster Flag Mutation

`ceph_exec` does not advertise `flags.set`. Ceph 19 Squid Dashboard exposes cluster flag mutation only as full-list replacement. A read-modify-write wrapper could overwrite concurrent operator changes, so global cluster flag mutation remains deferred.

## Outcome Semantics

Input validation, failed safe-to-destroy evaluation, capacity exhaustion before dispatch, and an explicit Dashboard rejection produce a definite safe error. The service does not retry a definite rejection.

Timeout, transport failure, MCP cancellation, ambiguous status, response-read failure, malformed success data, or unsafe HTTP 202 task data returns non-retryable `mutation_outcome_unknown` after dispatch can begin. The service does not infer failure from an ambiguous response and does not retry.

After `mutation_outcome_unknown`, the caller must inspect `task.list` and the relevant `status.get`, `osd.get`, or `flags.get` state in the same cluster before deciding whether another mutation is safe. Destroy and purge recovery must also repeat `osd.safe-to-destroy` and obtain a new explicit user decision before another exec call.
