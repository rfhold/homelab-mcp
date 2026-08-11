# Repository and Workflow Specifications

## Repository List

`repository.list` reads PAC `Repository` custom resources only from namespace `pipelines-as-code`. It does not enumerate Forgejo organizations or infer authority from Forgejo membership. Its result limit defaults to 50 and accepts values from 1 through 100.

The action will exclude a custom resource without a valid repository URL. This exclusion covers non-repository resources such as `global-git-config`.

Each result will contain:

- `id` as the exact canonical `org/repo` repository key;
- the normalized organization and repository names; and
- the normalized URL under fixed origin `https://git.holdenitdown.net`.

Normalization accepts only repository URLs that resolve to that fixed Forgejo origin and one organization/repository pair. It safely percent-decodes UTF-8 path components, accepts case-preserved RFC unreserved repository characters, strips a terminal `.git`, and rejects encoded separators or other characters outside that grammar. It derives `org/repo` from the normalized pair. Every repository selector and external repository relationship uses this canonical key.

PAC custom-resource names remain internal authority and adapter values. Callers cannot select repositories with those names, and results cannot expose them as relationships. No legacy repository aliases exist.

Authority catalog reads follow Kubernetes continuation tokens for at most 500 source objects. The catalog fails closed if that ceiling is exceeded.

If multiple valid PAC custom resources resolve to one canonical key, authority catalog construction fails closed. Results sort by canonical repository key.

## Workflow Discovery

`workflow.list` first resolves an authorized repository from its canonical key. Forgejo then reads only direct root files that match `.tekton/*.yaml` or `.tekton/*.yml` in that PAC-configured repository. Its result limit defaults to 100 and accepts values from 1 through 200.

The action will not recurse below `.tekton/`. It will not read workflow definitions from another origin, organization enumeration, or an unregistered repository.

One call inspects at most 50 PAC repositories and has one 30-second action deadline. Each repository permits at most 32 workflow files, 256 KiB per file, 4 MiB across decoded files, and 32 YAML documents per file. Default-branch revisions and file paths are at most 512 bytes, workflow names are at most 253 bytes, and each definition has at most 20 event names of 64 bytes each. A repository that exceeds or fails these bounds is reported as an independent `discovery_failed` partial failure; result-limit or repository-limit omission sets `truncated: true`.

Each YAML document can define multiple PAC events. The action will list every valid `PipelineRun` definition and each declared event.

A workflow event will be triggerable only when its event value equals exact string `incoming`. Similar values, combined expressions, and other PAC events will remain visible but non-triggerable.

Workflow identity binds these source facts:

- the canonical repository key;
- the repository revision used for discovery;
- the direct `.tekton` file path;
- the zero-based YAML document index; and
- the `PipelineRun` definition identity.

Events are grouped on that definition result. The serialized ID is `workflow/<base64url-sha256>` over the canonical repository key, default-branch revision, file path, zero-based YAML document index, and definition identity. Dispatch repeats discovery and requires an exact ID match against the same authorized source facts.

The canonical repository key changes the hash input from the prior internal identity. Workflow IDs therefore rotate, and clients must rediscover workflows before dispatch.

## Forgejo Boundary

Forgejo serves only repository content discovery for PAC-authorized repositories. The service will use the fixed origin `https://git.holdenitdown.net` and a server-held token.

The caller cannot select the Forgejo origin, API path, token, headers, organization, or an arbitrary repository. Redirects must not receive credentials.

Results will omit repository credentials, raw file bodies, unrecognized YAML fields, and Forgejo response wrappers.
