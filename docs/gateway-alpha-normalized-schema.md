# Gateway Alpha Normalized Schema

Ownership for the alpha gateway normalized schema lives in `src/contracts.rs`.
The compatibility fixture is
`fixtures/gateway/v0_1/alpha_normalized_contract_snapshot.json`.

## Versioning

- `protocol_version` is `gateway.v0.1` for gateway request, response, and
  stream-frame contracts.
- Usage and audit events keep the existing BOG-425 event envelope `version`
  value `0.1.0`.
- New alpha fields must be added as typed fields in `src/contracts.rs` and
  reflected in the compatibility snapshot before downstream children depend on
  them.

## Downstream Field Names

- Request: `request_id`, `correlation_id`, `working_group_id`, `actor`,
  `provider`, `model`, `messages`, `stream`, `policy`, `credential_ref`,
  optional `routing`.
- Response: `protocol_version`, `request_id`, `correlation_id`, `provider`,
  `model`, `content`, `finish_reason`, `usage`, `routing`.
- Stream frame: `protocol_version`, `request_id`, `correlation_id`, `sequence`,
  `frame_type`, `delta`, `usage`, `error`, optional `routing`.
- Usage event: root event envelope plus `payload.measurements` for
  `duration_ms`, token counts, `tool_invocations`, `estimated_cost_micros`,
  optional `metering_unit`, and optional `runtime_capability`.
- Audit event: root event envelope plus `payload.action`, `payload.outcome`,
  optional `runtime_capability`, optional `feature_flag`, and optional
  `approval_ref`.

## Routing Compatibility

`RoutingMetadata` is the alpha handoff point for routing, fallback, backend
relay, and observability work. It carries `provider`, `model`, `reason_code`,
`attempt`, and optional `fallback_from_provider`.

The current reason codes are:

- `primary_selected`
- `fallback_after_retryable_error`
- `fallback_after_timeout`
- `policy_denied`
- `capability_unsupported`

## Extension Points

Extensions are deliberately named in `ContractExtensionPoint` instead of
accepting arbitrary JSON fields. Unknown fields are rejected in normalized
contract structs with `serde(deny_unknown_fields)`.

Supported extension points are:

- `provider_metadata`
- `routing_policy`
- `stream_frame_metadata`
- `usage_measurements`
- `audit_payload`
