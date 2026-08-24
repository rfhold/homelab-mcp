# Pyinfra 3.x Authoring

Use this reference for pyinfra mechanics. The repository pins Python 3.13 and `pyinfra==3.5.0` in [`pyproject.toml`](../../../../pyproject.toml).

## Execution Model

Pyinfra uses prepare and execute phases. Prepare runs deploy code per host, reads facts, and determines operation order and expected changes. Execute reevaluates each operation, generates commands, and applies each operation across hosts before advancing.

Deploy Python branches see pre-execution state. Branch only on immutable facts, such as operating system family or architecture.

For execution-time dependencies, pass a callable to `_if`. Use `OperationMeta.did_change`, `did_not_change`, `did_succeed`, or `did_error` as the predicate. Do not pass a resolved value to `_if`.

## Operations

- Prefer declarative operations that compare facts with desired state.
- Give every operation a clear `name` for output and execution-order identification.
- Use operation global arguments only for cross-cutting execution controls.
- Put host and environment differences in inventory, group data, or host data.
- Access operation output only after execution through a callback.

Treat `server.shell`, `server.script`, and `python.call` as bounded escape hatches. Document why no declarative operation fits. Constrain inputs, outputs, time, privileges, and failure behavior.

Custom operations must yield pyinfra command objects. Build shell commands with `StringCommand` and wrap interpolated values with `QuoteString`. Never place untrusted values in formatted shell strings.

## Reusable Deploys

Use `@deploy` to package reusable operation groups. Let deploy global arguments apply shared execution controls. Define `data_defaults` for overridable defaults, then let inventory or group and host data supply environment values.

Keep entrypoints thin. Current examples delegate to reusable functions in [`deploys/entrypoints/`](../../../../deploys/entrypoints/) and [`deploys/lib/`](../../../../deploys/lib/).

## Inspection And Execution

- `--debug-operations` prints operation order and metadata, then exits without mutations.
- `--dry` connects, gathers facts, reports changes, and skips mutations.
- Neither flag prints final shell commands because execute-time generation has not occurred.
- `-vv` displays commands during actual execution and is not a non-mutating check.

Repository authority gates still apply when a non-mutating mode contacts a target. Use repository source-only checks for routine validation.

## Official Sources

- [Pyinfra 3.x documentation](https://docs.pyinfra.com/en/3.x/) - versioned documentation index.
- [Using operations](https://docs.pyinfra.com/en/3.x/using-operations.html) - phases, facts, `_if`, operation metadata, output, and callbacks.
- [Packaging deploys](https://docs.pyinfra.com/en/3.x/api/deploys.html) - `@deploy`, global arguments, and `data_defaults`.
- [Global arguments](https://docs.pyinfra.com/en/3.x/arguments.html) - execution, privilege, conditions, concurrency, and retry controls.
- [Writing operations](https://docs.pyinfra.com/en/3.x/api/operations.html) - operation generators and safe command construction.
