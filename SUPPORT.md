# Support

Telegram Personal MCP is a small open-source project maintained around a
deterministic local/demo path and a real Telegram backend that requires the
user's own account credentials.

## Before opening an issue

- Reproduce the problem with the latest `main` or the latest prerelease.
- Run the focused demo proof:

  ```bash
  cargo test --test stdio_demo
  ```

- Include the exact version or commit, operating system, MCP host, command/tool,
  and a minimal reproduction.
- Remove API hashes, `.session` files, proxy credentials, message dumps,
  downloaded private files, and local absolute paths from logs.

Use the bug-report or feature-request form so the relevant context is captured.

## Security and privacy

Do not disclose vulnerabilities or private Telegram data in a public issue.
Follow [SECURITY.md](SECURITY.md) for private reporting.

## Scope

The project does not provide account recovery, Telegram signup, Desktop `tdata`
conversion, general-purpose message export, or a hosted service. Questions
outside those boundaries may be closed with a pointer to this scope.
