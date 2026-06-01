# TaskOtter Gateway

TaskOtter Gateway is the execution-plane runtime boundary for model provider traffic and MCP hosting or brokering.

This scaffold intentionally implements only:

- Versioned gateway protocol structs and JSON fixtures.
- A deterministic fake provider adapter for local tests.
- MCP host health and lifecycle placeholders for hosted, runner-hosted, and external modes.

Frontend clients must not call this service directly. The TaskOtter control plane authenticates actors, evaluates policy, issues scoped dispatch instructions, relays safe stream events, and owns durable usage, audit, and billing records.

## Safety Boundaries

- Real provider keys, paid provider calls, private endpoint credentials, and production secret storage are not part of this scaffold.
- Runtime credentials are represented only by scoped references such as `secret_ref` identifiers.
- Signed dispatch placeholders keep `gwi_*` instruction references separate from canonical `poldec_*` policy decision lineage.
- `UsageEvent` and `AuditEvent` fixtures use the BOG-425 control-plane event envelope with root `id`, `type`, `version`, `actor`, `resource`, `correlation_id`, `request_id`, and `payload` fields.

## Checks

```sh
cargo fmt --check
cargo test
```

`contract-compatibility.json` declares the control-plane and gateway protocol
versions this repository consumes. CI calls the repo-local compatibility tests
`cargo test contract_compatibility_matrix_declares_supported_versions` and
`cargo test rejects_unsupported_gateway_protocol_fixture` so unsupported gateway
protocol fixtures fail before merge without requiring provider credentials or
paid resources.
