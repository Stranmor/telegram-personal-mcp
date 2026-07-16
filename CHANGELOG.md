# Changelog

All notable changes to Telegram Personal MCP are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and releases use [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-07-16

This is the first public prerelease. The deterministic fake backend, demo binary,
MCP-over-stdio path, and local safety boundaries are verified in CI. A real
Telegram account, login flow, and message delivery were not live-tested for this
release.

### Added

- Fourteen typed MCP tools for bounded dialog/message discovery, context recovery,
  attachment handling, prepared text sends, and file replies.
- A zero-credential deterministic demo backend and real child-process stdio proof.
- Crash-recoverable prepared text sends with stable Telegram `random_id` values,
  payload hashes, account revalidation, and delivery-uncertain state.
- Account-scoped attachment caching with streaming limits, immutable
  content-addressed originals, canonical path checks, and SHA-256-bound file
  snapshots before upload.
- TTY-only interactive Telegram authentication with hidden, zeroizing secret
  input and save-failure cleanup handling.
- Fail-closed single-session selection, owner-only local permissions, canonical
  peer references, closed MCP input schemas, typed errors, and protocol
  negotiation regression coverage.

### Documentation and maintenance

- Public README, security policy, contribution guide, support path, issue forms,
  code of conduct, Dependabot configuration, reproducible formatting, and CI
  package checks.

### Known boundaries

- Global Telegram search is intentionally first-page-only.
- There is no durable update feed, deletion tool, unbounded single-call export,
  signup flow, or automatic Telegram Desktop `tdata` conversion.
- Direct text/file sends are non-idempotent; do not retry them blindly after an
  uncertain transport result.
- The release does not claim general exactly-once delivery or live Telegram E2E
  verification.

[Unreleased]: https://github.com/Stranmor/telegram-personal-mcp/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Stranmor/telegram-personal-mcp/releases/tag/v0.1.0
