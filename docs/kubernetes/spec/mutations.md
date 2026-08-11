# Kubernetes Mutation Specifications

## Shared Mutation Contract

`kubernetes_exec` will expose only exact-object actions. Every action will require a configured cluster, exact namespace, exact name, and typed action-specific fields.

Each call requires an explicit user decision. The service will launch one fixed `kubectl` process and will not retry automatically.

Each action will accept optional `dry_run`. When true and the Kubernetes API supports the operation, the command will request server dry-run. Dry-run still requires the same runtime RBAC as the real mutation.

`dry_run: true` validates API admission but does not prove that a later real mutation will succeed. A successful real process reports acceptance, not eventual workload health or controller completion.

## Workload Restart

`workload_restart` will target one Deployment, StatefulSet, or DaemonSet. It will use the fixed rollout-restart operation for that exact object.

ReplicaSet, Job, CronJob, Pod, and arbitrary resource restart requests will fail input validation.

## Workload Scale

`workload_scale` will target one Deployment or StatefulSet. `replicas` will accept values from 0 through 1,000.

DaemonSet, ReplicaSet, Job, CronJob, and arbitrary scale requests will fail input validation.

## CronJob State and Trigger

`cronjob_suspend` will set the exact CronJob suspension state from required Boolean field `suspended`. The same action supports suspend and resume without a general patch surface.

`cronjob_trigger` will create one Job from one exact CronJob. The server will construct the Job name. The caller cannot supply a generated name, template, or Job body.

## Pod Delete

`pod_delete` will delete one exact namespaced Pod. It will not accept selectors, grace overrides, propagation overrides, force, or multiple names.

The action requests ordinary Kubernetes deletion. It does not guarantee that a controller will not create a replacement Pod.

## Outcome Semantics

A validation failure or process spawn failure before dispatch returns `mutation_rejected`. The caller can correct definite pre-dispatch state before another call.

Every non-success process outcome after spawn, including a numeric exit, signal, timeout, cancellation, wait failure, or output/pipe failure, returns non-retryable `mutation_outcome_unknown`. The service does not parse stderr to infer a definite API rejection, and the mutation can already exist in cluster state.

After `mutation_outcome_unknown`, the caller must use `kubernetes_query` to inspect the exact object before another mutation. CronJob trigger recovery must inspect Jobs and the source CronJob.

No mutation will expose stdout, stderr, raw objects, command lines, credentials, or API origins.
