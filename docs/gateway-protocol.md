# Gateway Protocol Assumptions

## Versioning

- Current scaffold protocol: `gateway.taskotter.dev/v1alpha1`.
- All gateway requests carry a protocol version, request id, Working Group boundary, actor id, and principal id.
- Control-plane schemas are owned by BOG-400 and referenced through stable schema ids:
  - `https://schemas.taskotter.dev/v1/gateway-protocol.schema.json`
  - `https://schemas.taskotter.dev/v1/policy-decision.schema.json`
  - `https://schemas.taskotter.dev/v1/usage-event.schema.json`
- Shared schemas must be consumed through generated packages, published artifacts, or explicit schema references; they must not be copied manually between repositories.

## Capability Surfaces

The MVP gateway recognizes three surfaces:

- `model_provider`: hosted, local, or OpenAI-compatible model endpoints.
- `mcp_server`: hosted MCP, runner-hosted MCP, or externally connected MCP endpoints.
- `runner_tool`: tool execution delegated to a registered runner.

Each capability declares input and output schema references, network access expectations, and whether secret material is required. Secrets are resolved outside the gateway by integration and policy systems.

## Policy Integration

Before dispatch, the gateway builds a `PolicyCheck` containing:

- Working Group boundary.
- User actor and acting principal.
- Tool call details.
- Estimated usage units.

The control-plane policy engine returns the canonical BOG-400 `PolicyDecision` shape:

- `effect`: final `allow` or `deny`.
- `constraints[]`: named policy constraints with individual `allow` or `deny` effects and reasons.

Policy decisions compose with AND semantics: every applicable constraint must allow a request before gateway dispatch can continue. The gateway does not treat prompt instructions, model output, or tool metadata as an authorization boundary.

## Usage Events

The gateway emits usage events for every policy decision:

- `reserved` when a request is allowed and capacity is preflighted.
- `rejected` when policy denies or the request cannot proceed.
- `committed` is reserved for the later execution adapter once actual usage is known.

The serialized event uses the canonical BOG-400 `UsageEvent` shape:

- `id`
- `working_group_id`
- `principal`
- `subject`
- typed `units[]`
- `idempotency_key`

Gateway-local lifecycle stage is encoded into the event id and idempotency key, not as an extra serialized field, because the current canonical schema has `additionalProperties: false`.

Idempotency key format for MVP pre-dispatch events:

```text
gateway:{request_id}:{capability_id}:{reserved|rejected|committed}
```

Allowed unit kinds come from the BOG-400 schema: `input_token`, `output_token`, `tool_invocation`, `runtime_millisecond`, `stored_byte`, and `estimated_cost_microusd`.

## Known Gaps

- No live MCP transport, provider client, runner dispatch, or streaming implementation is included yet.
- No generated shared schema package is imported yet; Rust types are aligned with BOG-400 schema seeds until codegen or a published schema crate exists.
- Policy and usage integrations are traits only. Production adapters must add retries, timeouts, redaction, idempotency-key persistence, and audit correlation.
- Model output safety, prompt injection handling, and tool result validation need dedicated tests when execution adapters are added.

## Test Strategy

- Unit tests validate request invariants and policy/usage behavior.
- Serialization tests cover the current canonical policy decision and usage event shapes.
- Contract tests should replay allow, deny, quota-exceeded, missing-secret, and unknown-capability cases against the control-plane policy API.
- Integration tests should cover hosted MCP, runner-hosted MCP, external MCP, local model, and hosted model routing once transports exist.

## PR Closure Conditions

- Keep policy decision and usage event serialization compatible with BOG-400 schema seeds.
- Preserve retry-safe idempotency for `reserved`, `rejected`, and future `committed` usage events.
- Add generated schema validation or consumer-driven contract tests once BOG-400 publishes a shared schema artifact.
- Do not merge into the parent integration branch until the contract reconciliation blocker is cleared by review.
