## Summary

<!-- What user-visible behavior or public-surface change does this PR deliver? -->

## Motivation and scope

- Problem or workflow:
- Why this belongs in the current MCP boundary:
- Related issue:

## Verification

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace --all-targets --locked`
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`
- [ ] `cargo package --locked --no-verify`
- [ ] Relevant demo, transport, recovery, or boundary test added/updated

## Safety and recovery

- [ ] No credentials, session bytes, private Telegram content, or local absolute paths are included.
- [ ] Unknown MCP input fields remain rejected.
- [ ] External sends preserve approval binding, idempotency/uncertainty semantics, and account checks.
- [ ] File changes preserve canonical-root, size-limit, snapshot, and hash invariants.
- [ ] Documentation and changelog match the implemented contract.

## Manual boundaries

<!-- State explicitly whether real Telegram/auth/manual external testing was performed. -->
