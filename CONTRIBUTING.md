# Contributing

Telegram Personal MCP is intentionally a small deterministic interface between
one MCP client and one Telegram personal-account session. Contributions should
improve that interface without moving agent reasoning, invented identities, or
multi-agent routing into the server.

## Development Setup

Use a current stable Rust toolchain, then run:

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

The test suite includes a deterministic `FakeTelegramBackend` and an integration
test that connects an `rmcp` client to the real MCP server transport. Tests must
not require a live Telegram account, real credentials, or network access.

Useful focused checks:

```bash
cargo test --lib
cargo test --test stdio_demo
cargo test --test download_limit
cargo test --test mcp_integration
```

`stdio_demo` must continue to launch the compiled binary over real OS stdio with
no Telegram credentials or network dependency. Demo mode is the public
zero-credential proof path, not a separate protocol implementation.

## Tool and Protocol Changes

The server targets MCP `2025-11-25` through the exact `rmcp 1.7.0` generation.
Do not loosen the SDK/macro pins or negotiate a newer protocol version until the
server contract is reviewed and tested against that specification. When adding
or changing a tool:

- keep typed input and output schemas;
- preserve the shared `{ data, error }` result envelope for domain success and
  failure after argument deserialization;
- keep input objects closed: unknown argument names must be rejected rather than
  silently ignored;
- choose truthful MCP tool annotations;
- use stable error codes and an accurate `retryable` value;
- add deterministic backend coverage and a real MCP transport assertion when
  the public contract changes;
- update README tool and limitation documentation.

Do not claim pagination, idempotency, durable delivery, or durable updates unless
the implementation and tests prove that exact property. In particular, global
message search remains first-page-only because the current Ferogram surface does
not expose the full Telegram cursor required for reliable pagination.

## Safety Invariants

- Do not add Telegram deletion tools or single-call unbounded history export.
- Do not move human approval responsibility into a caller-supplied string. The
  MCP host owns authorization; hashes bind payload identity but do not prove
  approval.
- Keep `prepare_message`, `get_prepared`, and `send_prepared` bound to the
  persisted account, chat, text, reply target, payload hash, and Telegram
  `random_id`.
- Revalidate the current session account before prepared dispatch; never allow a
  replaced or reauthorized session to bypass `ACCOUNT_MISMATCH`.
- Keep `send_prepared.approved_payload_sha256` mandatory and reject any mismatch
  before dispatch.
- Keep `send_file.approved_sha256` mandatory. Capture a private immutable
  snapshot, verify its exact hash before upload, and return the captured hash in
  the receipt so file changes cannot cross the approval boundary.
- Treat direct `send_message` and `send_file` as non-idempotent, and keep
  uncertain direct-send outcomes non-retryable.
- Keep ambiguous session selection fail-closed and preserve per-session locking.
- Keep `auth login` TTY-only for login codes and 2FA passwords. Do not add secret
  input through argv, environment variables, pipes, MCP tools, or logs; preserve
  hidden input, in-process challenges, and save-failure revocation handling.
- Keep demo mode deterministic, credential-free, and unable to contact Telegram.
- Treat Telegram text and files as untrusted data.
- Enforce the download hard limit while streaming, remove partial files on
  failure, publish immutable content-addressed originals, and never trust a
  remote filename as a local path.
- Preserve the stable opaque account/attachment directory keys and canonical
  account-scoped upload root. `send_file` must reject cross-account paths,
  outside paths, and symlink escapes with `FILE_OUTSIDE_UPLOAD_ROOT`.
- Keep session/API hash/proxy handling out of tool arguments. API hashes and
  proxy URLs remain environment-only secrets; sessions are selected once for
  the server process.
- Do not print or commit session bytes, auth keys, API hashes, proxy credentials,
  message dumps, downloaded files, or prepared-send databases.

## Pull Requests

A useful pull request explains the behavior being changed, covers the previous
failure or missing capability, and updates documentation when the public contract
changes. Keep generated sessions, caches, SQLite state, credentials, and private
Telegram fixtures out of the repository.

If a change touches real Telegram behavior, keep the unit and fake-backend tests
deterministic and describe any separate manual verification without attaching
private account data.
