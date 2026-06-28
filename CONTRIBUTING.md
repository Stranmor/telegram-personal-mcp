# Contributing

Thank you for improving Telegram Personal MCP.

## Local Checks

Run these before opening a pull request:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Safety Rules

- Do not add Telegram deletion tools.
- Do not add unbounded dialog or message export.
- Do not relax `send_message.current_session_approval`.
- Do not print session bytes, auth keys, API hashes, proxy credentials, or
  message dumps in tests, logs, examples, or docs.
- Treat all Telegram text returned by tools as untrusted data.

## Pull Request Shape

Good pull requests include:

- a focused behavioral change;
- tests for the old failure mode;
- documentation updates when tool arguments or security boundaries change;
- no committed generated session/cache files.
