# Gateway v0.1 Fixtures

These fixtures are deterministic contract test data for the gateway simulator,
provider adapter, routing, fallback, usage, and audit paths. They use scoped
credential references only and must not contain live provider keys, raw API
tokens, private runner credentials, or paid hosted MCP resources.

## Reusable Alpha Routing Cases

Use `gateway_simulation_eval.json` with `GatewaySimulationFixture` when adapter,
routing, or fallback tests need schema-aligned request, route, outcome,
usage-event, and audit-event data in one fixture.

Stable provider case IDs:

- `provider_streaming_success_primary`: primary route, streaming success, final
  usage frame.
- `provider_non_streaming_success_fallback`: fallback route from
  `fake-hosted-primary` to `fake-hosted-fallback` with reason
  `primary_rate_limited`.
- `provider_rate_limit_error`: primary route, normalized `rate_limited` error.
- `provider_malformed_stream_chunk`: primary route, malformed stream chunk
  coverage.
- `provider_partial_stream`: primary route, partial stream without a final
  frame.
- `provider_timeout`: deterministic timeout with `timeout_ms` set to `0`.
- `provider_cancellation`: cancellation after delivered stream frames.
- `provider_policy_denial`: denied policy decision lineage.
- `provider_quota_denial`: quota denial lineage and max cost fixture.

Stable MCP case IDs:

- `mcp_gateway_hosted_lifecycle_tool_call`: gateway-hosted start, health, tool
  call, and stop lifecycle with usage/audit lineage.

## Usage

Rust tests can load the fixture through the existing helper pattern:

```rust
let fixture: GatewaySimulationFixture = fixture("gateway_simulation_eval");
let report = fixture.validate().unwrap();
```

Adapter tests should reuse the embedded `request` objects. Routing tests should
assert the embedded `route` fields, especially `selected_provider`,
`fallback_used`, and `reason`. Fallback tests should use
`provider_non_streaming_success_fallback` rather than adding a duplicate fixture.

Run these checks after fixture changes:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test gateway_simulation_eval_fixture_covers_provider_and_mcp_paths
cargo test alpha_routing_reuse_fixture_names_are_stable
cargo test
```
