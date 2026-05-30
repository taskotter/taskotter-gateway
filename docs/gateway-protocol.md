# Gateway Protocol Assumptions

## Versioning

- Current scaffold protocol: `gateway.taskotter.dev/v1alpha1`.
- All gateway requests carry a protocol version, request id, Working Group boundary, actor id, and principal id.
- Shared schemas must be referenced by versioned identifiers such as `taskotter.schema.tool-call.v1alpha1`; they must not be copied manually between repositories.

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

The control-plane policy engine returns an auditable `PolicyDecision` with `allow` or `deny`, a decision id, and reason. The gateway does not treat prompt instructions, model output, or tool metadata as an authorization boundary.

## Usage Events

The gateway emits usage events for every policy decision:

- `reserved` when a request is allowed and capacity is preflighted.
- `rejected` when policy denies or the request cannot proceed.
- `committed` is reserved for the later execution adapter once actual usage is known.

MVP usage uses generic units so later adapters can map model tokens, tool calls, hosted MCP runtime duration, request count, or estimated cost into specialized ledgers without changing the request envelope.

## Known Gaps

- No live MCP transport, provider client, runner dispatch, or streaming implementation is included yet.
- No shared schema package is imported yet; schema references are placeholders until the control-plane contract is published.
- Policy and usage integrations are traits only. Production adapters must add retries, timeouts, redaction, and audit correlation.
- Model output safety, prompt injection handling, and tool result validation need dedicated tests when execution adapters are added.

## Test Strategy

- Unit tests validate request invariants and policy/usage behavior.
- Serialization tests should be added when shared schemas are published.
- Contract tests should replay allow, deny, quota-exceeded, missing-secret, and unknown-capability cases against the control-plane policy API.
- Integration tests should cover hosted MCP, runner-hosted MCP, external MCP, local model, and hosted model routing once transports exist.
