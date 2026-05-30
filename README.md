# TaskOtter Gateway

MVP foundation for the TaskOtter AI Gateway and MCP hosting boundary.

This service is intentionally a contract scaffold. It normalizes AI provider requests, applies a gateway policy hook supplied by the control plane, routes to stub provider adapters, models MCP endpoint hosting modes, and emits explicit versioned usage/audit events. It does not collect provider credentials, proxy production traffic, launch paid hosted MCP runtimes, or manage billing.

## Local Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo run
```

The server listens on `127.0.0.1:8080` by default. Override with `TASKOTTER_GATEWAY_ADDR`.

## MVP Contracts

- `POST /v1/ai/relay` accepts a normalized provider request and returns a stub relay response with a `usage_audit_event.v1` payload.
- `POST /v1/mcp/resolve` accepts an MCP endpoint model and returns the policy-visible lifecycle mode.
- `GET /healthz` returns service health.

## Known Limitations

- Provider adapters are stubs. Hosted providers, OpenAI-compatible endpoints, local runner endpoints, and future adapters share the same routing contract but do not call external services.
- Streaming relay is represented by request/response contract fields only. No SSE/WebSocket relay is implemented yet.
- The policy hook is local and deterministic for tests. Production policy decisions must come from `taskotter/taskotter` control-plane dispatch or scoped policy decisions.
- Usage and audit events are serialized locally but not delivered to a ledger or audit ingestion API yet.
- MCP lifecycle states are modeled but no process supervisor, external SSE client, runner dispatch, or managed hosting runtime is started.

## Cross-Repo Dependencies

- `taskotter/taskotter` must define the source-of-truth policy decision, dispatch instruction, provider/model registry, usage ledger ingestion, and audit event ingestion contract.
- `taskotter-remote` must align on runner-hosted MCP and local-provider endpoint dispatch semantics.
- Shared schema publication should eventually move these request, policy, MCP, and usage event shapes into a versioned schema package or OpenAPI contract.
