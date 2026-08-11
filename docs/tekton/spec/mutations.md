# Tekton Mutation Specifications

## Shared Mutation Contract

`tekton_exec` will advertise every action as non-read-only, destructive, non-idempotent, and open-world. The current `mcp:use` scope will authorize all three actions.

Every call requires an explicit user decision. The service will make one upstream attempt and will never retry automatically.

A safe pre-dispatch validation failure or explicit upstream rejection will return a non-retryable rejection. An ambiguous failure after dispatch can begin will return non-retryable `mutation_outcome_unknown`.

Timeout, transport failure, MCP cancellation, and response failure after send can produce `mutation_outcome_unknown`. The caller must inspect current runs before another mutation.

## Workflow Dispatch

`workflow.dispatch` validates the repository selector under the [canonical repository key contract](repositories-workflows.md#repository-list). It also validates the exact triggerable workflow identity, Git reference, and bounded parameters before send. Only a workflow definition with an event whose exact value is `incoming` can dispatch. References are nonblank, control-free, and at most 512 bytes. At most 20 string parameters are accepted; keys are nonblank and at most 128 bytes, and values are at most 4,096 bytes.

The action will send one POST to the fixed PAC controller internal route `/incoming`. The body will contain the server-held PAC secret and validated dispatch data.

The caller cannot control the URL, method, headers, secret, namespace, internal PAC custom-resource identity, or repository mapping. The service will reject caller input that conflicts with those fixed values.

Any HTTP 2xx response means that PAC accepted the request. Acceptance does not prove that PAC created a `PipelineRun` or that the run succeeded.

## Run Rerun

`run.rerun` resolves an owned prior `PipelineRun` by exact namespace-qualified ID. It safely replays that run's prior branch and string parameters through the fixed PAC `/incoming` route under the same bounds as dispatch.

The caller cannot replace the prior branch, parameters, canonical repository relationship, workflow, namespace, or internal PAC identity. A rerun uses the same dispatch validation and outcome rules as `workflow.dispatch`.

## Run Cancel

`run.cancel` will resolve an owned `PipelineRun` by exact namespace-qualified ID. It will verify that the run remains active before mutation.

The service patches only `spec.status=Cancelled` together with the inspected `metadata.resourceVersion` concurrency precondition. It rejects terminal runs, ownership mismatches, and a patch conflict if the run changes after inspection.

A successful Kubernetes patch means that cancellation was requested. It does not prove immediate task termination or a terminal run state.

An explicit pre-patch rejection remains a safe rejection. Timeout, transport failure, MCP cancellation, or response ambiguity after patch send will return non-retryable `mutation_outcome_unknown`.
