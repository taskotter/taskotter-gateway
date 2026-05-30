# Repository Boundaries

## Owned Here

- Gateway protocol envelope and gateway-local typed request models.
- Capability surface definitions for MCP, model provider, and runner-tool routing.
- Gateway-facing policy check and usage event abstractions.
- Gateway planner behavior before execution dispatch.

## Owned Elsewhere

- Control-plane API, canonical resource schemas, policy payload contracts, identity, quotas, and audit storage are owned by the main product backend.
- Runner registration, runner capability inventory, job transport, cancellation, and local execution isolation are owned by the remote runner.
- Shared schemas must be consumed through generated packages, published artifacts, or explicit versioned schema references.

## Adapter Contract

- Gateway policy decisions serialize to the BOG-400 `PolicyDecision` schema: `effect + constraints[]`.
- Gateway usage events serialize to the BOG-400 `UsageEvent` schema: `id`, `working_group_id`, `principal`, `subject`, typed `units[]`, and `idempotency_key`.
- Gateway-local lifecycle state is represented by deterministic event ids and idempotency keys until the control-plane schema adds an explicit lifecycle field.
- Gateway adapters must treat `idempotency_key` as stable for retries of the same request, capability, and lifecycle stage.

## Confidentiality Boundary

Public-facing docs should describe the gateway at a high level. Detailed private roadmap strategy, pricing, unreleased enterprise positioning, and internal planning notes should stay out of public README and release notes.
