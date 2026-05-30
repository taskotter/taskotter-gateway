# TaskOtter Gateway

TaskOtter Gateway is the foundation for routing AI and MCP tool activity through explicit protocol, policy, and usage-metering contracts.

This repository currently contains an MVP Rust library scaffold. It does not call live model providers, run hosted MCP servers, or require third-party credentials.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
./scripts/check-file-size.sh
```

## License

This development repository uses the PolyForm Strict License 1.0.0 baseline. See `LICENSE`.
