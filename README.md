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
- Policy decisions are represented by a verifiable placeholder so future signed instructions or online checks can replace it without widening the gateway boundary.
- `UsageEvent` and `AuditEvent` fixtures use the control-plane `usage-event@0.1.0` and `audit-event@0.1.0` shapes from BOG-425.

## Checks

```sh
cargo fmt --check
cargo test
```
