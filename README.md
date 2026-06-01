# TaskOtter Gateway

MVP foundation for the TaskOtter AI Gateway and MCP hosting boundary.

This service is intentionally a contract scaffold. It normalizes AI provider requests, applies a gateway policy hook supplied by the control plane, routes to stub provider adapters, models MCP endpoint hosting modes, and emits explicit versioned usage/audit events. It does not collect provider credentials, proxy production traffic, launch paid hosted MCP runtimes, or manage billing.

TaskOtter Gateway is also the execution-plane runtime boundary for model provider traffic and MCP hosting or brokering. Frontend clients must not call this service directly. The TaskOtter control plane authenticates actors, evaluates policy, issues scoped dispatch instructions, relays safe stream events, and owns durable usage, audit, and billing records.

## Local Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo run
```

The server listens on `127.0.0.1:8080` by default. Override with `TASKOTTER_GATEWAY_ADDR`.

## MVP Contracts

- `POST /v1/ai/relay` accepts a normalized provider request and returns a stub relay response with a gateway-local `gateway_relay_audit.v1` payload.
- `POST /v1/mcp/resolve` accepts an MCP endpoint model and returns the policy-visible lifecycle mode.
- `GET /healthz` returns service health.
- Versioned gateway protocol structs and JSON fixtures validate the gateway/control-plane contract.
- The alpha normalized schema ownership note lives in `docs/gateway-alpha-normalized-schema.md`.
- The deterministic fake provider adapter and MCP runtime host placeholders support local compatibility tests without provider credentials or paid resources.

Policy decisions use the control-plane canonical shape:
`allowed`, `decision_id`, optional `reason`, optional `max_tokens`, and optional
`max_cost_micro_usd`. The gateway treats `allowed` as authoritative and copies
`decision_id` into every usage/audit event.

`gateway_relay_audit.v1` is emitted in relay responses for succeeded, denied,
and timeout attempts. This is a gateway-local response audit summary, not the
control-plane `/v1/usage/events` ingestion event. It includes `request_id`,
optional `correlation_id`, subject, provider, `decision_id`,
`routing_reason_code`, `status`, token counts, and estimated cost in micro-USD.
The bounded `routing_reason_code` values are `primary_selected`,
`fallback_selected`, `policy_denied`, `quota_denied`, `provider_timeout`,
`provider_error`, and `cancelled`.

Durable control-plane usage ingestion remains owned by `taskotter/taskotter`
OpenAPI and uses the canonical `UsageEvent` envelope. Gateway-local relay audit
summaries must not be sent to `/v1/usage/events` without a mapper that produces
the control-plane envelope.

## Deterministic Simulation Fixtures

`fixtures/gateway/v0_1/gateway_simulation_eval.json` drives the repo-local
`simulator` harness. It validates deterministic provider routing and fallback,
streaming and non-streaming success, rate-limit and generic provider failure,
malformed chunks, partial streams, timeout, cancellation, policy denial, quota
denial, usage/audit lineage, and a gateway-hosted MCP lifecycle with one tool
call. The fixture uses only scoped credential references and does not require
live provider keys, hosted MCP runtime, private runner access, or paid resources.
Reusable alpha routing case names and adapter/routing/fallback usage are listed
in `fixtures/gateway/v0_1/README.md`.

## Local Metrics Verification

Gateway alpha metrics are modeled by the repo-local `metrics` module so local
and CI checks can verify observability behavior without a paid collector. The
bounded metric names are `gateway_provider_latency_ms`,
`gateway_stream_start_latency_ms`, `gateway_fallback_count`,
`gateway_policy_denial_count`, `gateway_usage_event_delivery_lag_ms`, and
`gateway_provider_error_count`.

Metric labels intentionally use bounded enums only: outcome, provider kind,
route type, routing reason code, normalized error code/class, event source, and
stream frame type. They must not include prompt text, raw provider messages,
request IDs, correlation IDs, working group IDs, provider secrets, credential
references, raw headers, customer data, or private payloads.

Verify locally or in CI-compatible mode with:

```sh
cargo test gateway_alpha_observability_metrics -- --nocapture
cargo test metrics -- --nocapture
```

## Safety Boundaries

- Real provider keys, paid provider calls, private endpoint credentials, and production secret storage are not part of this scaffold.
- Runtime credentials are represented only by scoped references such as `secret_ref` identifiers.
- Signed dispatch placeholders keep `gwi_*` instruction references separate from canonical `poldec_*` policy decision lineage.
- `UsageEvent` and `AuditEvent` fixtures use the BOG-425 control-plane event envelope with root `id`, `type`, `version`, `actor`, `resource`, `correlation_id`, `request_id`, and `payload` fields.

## Compatibility Checks

`contract-compatibility.json` declares the control-plane and gateway protocol versions this repository consumes. CI calls the repo-local compatibility tests `cargo test contract_compatibility_matrix_declares_supported_versions` and `cargo test rejects_unsupported_gateway_protocol_fixture` so unsupported gateway protocol fixtures fail before merge without requiring provider credentials or paid resources.

## Known Limitations

- Provider adapters are stubs. Hosted providers, OpenAI-compatible endpoints, local runner endpoints, and future adapters share the same routing contract but do not call external services.
- Streaming relay is represented by request/response contract fields only. No SSE/WebSocket relay is implemented yet.
- The policy hook is local and deterministic for tests. Production policy decisions must come from `taskotter/taskotter` control-plane dispatch or scoped policy decisions.
- Gateway-local `gateway_relay_audit.v1` response summaries are intentionally
  separate from the control-plane `/v1/usage/events` ingestion schema. The
  gateway does not deliver usage events to a ledger yet.
- MCP lifecycle states are modeled but no process supervisor, external SSE client, runner dispatch, or managed hosting runtime is started.

## Cross-Repo Dependencies

- `taskotter/taskotter` defines the source-of-truth OpenAPI contract for policy
  decisions, gateway usage events, remote usage reports, and audit ingestion.
- `taskotter-remote` must align on runner-hosted MCP and local-provider endpoint dispatch semantics.
- Shared schema publication is currently the generated control-plane OpenAPI
  artifact. A future generated schema package should be derived from that
  artifact rather than hand-copied between repositories.
