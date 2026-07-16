# Telegram Personal MCP

[![CI](https://github.com/Stranmor/telegram-personal-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/Stranmor/telegram-personal-mcp/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Stranmor/telegram-personal-mcp?include_prereleases&sort=semver)](https://github.com/Stranmor/telegram-personal-mcp/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

A deterministic MCP server for Telegram personal accounts: find conversations,
recover exact message context, move attachments through a local workspace, and
send replies through a host-owned approval flow.

The server does not contain an AI agent and does not route work between agents.
The MCP host's agent or model supplies reasoning and invokes this server through
its MCP client. The human remains the approval authority; this server supplies a
bounded, typed Telegram interface over stdio.

```text
Human approval
        |
        v
Host agent / model
        |
        v
MCP client
        |
        | MCP over stdio
        v
telegram-personal-mcp ---- account-scoped attachment cache
        |                  prepared-send ledger + private file snapshots
        | MTProto
        v
     Telegram
```

## Live Codex Demo

[![Codex using Telegram Personal MCP](docs/assets/telegram-personal-mcp-demo.gif)](docs/assets/telegram-personal-mcp-demo.mp4)

This recording runs a real `codex exec` session against
`telegram-personal-mcp --demo`. The terminal view renders the live Codex event
stream rather than replaying a prewritten tool transcript: the host agent finds
the contract message, reads its context, downloads the PDF, creates a writable
sibling, binds its exact SHA-256, replies to the source message, and verifies the
reply with another context read.

The deterministic fixture uses no Telegram credentials, network connection, or
real message delivery. The source [asciinema cast](docs/assets/telegram-personal-mcp-demo.cast)
and [reproducible renderer](scripts/render_terminal_demo.sh) are included in the
repository.

The implementation uses the official Rust MCP SDK, pinned to `rmcp 1.7.0`,
and targets the implemented stable MCP `2025-11-25`. The pin deliberately
avoids claiming or negotiating MCP `2026-07-28` before that contract is
implemented here. A regression test requests that version and verifies the
server falls back to `2025-11-25`. All 14 tools have input/output schemas, MCP
annotations, and a stable structured result envelope after argument
deserialization.

`v0.1.0` is the first public prerelease. Release evidence is deterministic
and credential-free: CI verifies the real binary, package, stdio transport, and
demo/fake workflow, while live Telegram authentication and delivery remain an
explicit prerelease boundary.

## Try It Without Telegram

Demo mode runs the real MCP binary against a deterministic local fixture. It
does not need Telegram credentials or a session and does not contact Telegram.

The fastest end-to-end proof launches the compiled binary as a child process,
connects an `rmcp` client over real OS stdio, finds the contract message, reads
its context, downloads the fixture PDF, creates a modified copy, and sends that
copy as a reply with an exact SHA-256 binding:

```bash
cargo test --test stdio_demo
```

The test also emits two compact proof lines. The hashes below are stable for
the checked-in fixture and show both the negotiated protocol and the exact
downloaded/replied bytes:

```text
protocol-proof requested=2026-07-28 negotiated=2025-11-25
demo-proof tools=14 message_id=11 downloaded_sha256=8a9e0a420df39699e56b1cc65a12505d8300394566f3d7be014bc20d62a4901c file_reply_sha256=909a3dbb487550bd8d00c5d769649fe98f0212f0d7b6124f2ff839c4132ae60f
```

To explore the same fixture from an MCP host:

```bash
cargo build --release
```

Register the resulting executable as a stdio MCP server with the single
argument `--demo`, using that host's documented configuration format. MCP host
configuration formats are not universal, so this README does not present one
host's syntax as a portable standard.

A useful human prompt for the host agent is:

> Find Alex's contract message, read the surrounding context, download the PDF,
> save a modified copy beside the original, then show me the exact recipient,
> reply target, caption, and SHA-256 before asking whether to send it.

`TELEGRAM_MCP_DEMO=true` is equivalent to `--demo`. Demo mode starts the stdio
server directly and cannot be combined with `doctor`, `session`, or `auth`
subcommands. Its tools can still write fixture downloads and prepared-send
state to the configured local cache/state paths.

## What It Enables

The main workflow is deliberately practical:

1. Find the right dialog and message.
2. Read the exact message plus a bounded window around it.
3. Inspect and download its attachment into a managed local cache.
4. Let the calling agent modify the file with its normal local tools.
5. After the MCP host obtains approval for the exact destination and payload,
   reply with the resulting file.

No Telegram message text is treated as an instruction by the server. There are
no deletion tools, no single-call unbounded history export, and no background
update feed.

## Engineering Depth

The project is designed around failure modes that a thin Telegram wrapper
usually leaves to its caller:

| Failure mode | Mechanism |
| --- | --- |
| A misspelled send field silently changes behavior | Closed input schemas reject every unknown field at the MCP boundary |
| A process dies around a text send | Prepared drafts persist state, Telegram `random_id`, payload hash, and final receipt across restarts |
| The configured session is replaced before dispatch | `send_prepared` revalidates the live Telegram account against the account bound at prepare time |
| A local file changes after approval | `send_file` snapshots exact bytes, verifies `approved_sha256`, then uploads the immutable snapshot |
| A caller selects an arbitrary or cross-account file | Upload paths must resolve inside the selected account's canonical cache root |
| An SDK advertises a protocol contract this server does not implement | The SDK generation and server version are pinned, with a raw-stdio negotiation regression test |

These mechanisms do not replace host-side human approval. They make the
deterministic boundary beneath that approval inspectable, recoverable, and hard
to call incorrectly.

## Demo Workflow

These are MCP tool calls made by the client, not shell commands:

```text
search_dialogs({ query: "Alex" })
  -> chat_ref: "@alex"

search_messages({ chat_id: "@alex", query: "contract.pdf", limit: 20 })
  -> message_id: 11

read_context({ chat_id: "@alex", message_id: 11, before: 5, after: 5 })
  -> target message plus nearby messages

list_attachments({ chat_id: "@alex", message_id: 11 })
  -> contract.pdf, application/pdf

download_attachment({ chat_id: "@alex", message_id: 11 })
  -> local_path: ".../attachments/2a1c.../7b9f.../<sha256>-contract.pdf"
     bytes_written, sha256

# The downloaded original is immutable. The agent creates a new sibling file
# with tools outside this MCP, for example:
# ".../attachments/2a1c.../7b9f.../contract-updated.pdf"
# The host then obtains human approval for the exact chat, file, caption,
# reply target, delivery mode, and SHA-256.

send_file({
  chat_id: "@alex",
  file_path: ".../attachments/2a1c.../7b9f.../contract-updated.pdf",
  caption: "Updated contract",
  reply_to_message_id: 11,
  mode: "document",
  approved_sha256: "<sha256 of contract-updated.pdf>"
})
```

The in-process MCP integration test exercises the fuller shape, including
message search, context recovery, a PDF download, prepared text-send replay, a
file reply, and rejection of a file outside the managed upload root:

```bash
cargo test --test mcp_integration
```

## Tools

| Tool | Purpose | Important behavior |
| --- | --- | --- |
| `health` | Verify the selected Telegram account | Calls `get_me`; returns account metadata without exposing the local session path; does not read dialogs or messages |
| `list_dialogs` | List a bounded dialog slice | Default 30, maximum 100 |
| `search_dialogs` | Find dialogs by title, username, reference, or numeric ID | Preview-text matching is opt-in; scans at most 500 dialogs |
| `read_chat` | Read a bounded recent message page | Default 30, maximum 50; supports `offset_id` |
| `search_messages` | Search within one chat or across Telegram | In-chat search is pageable; global search is first-page-only |
| `get_message` | Fetch one exact message | Requires a chat reference and positive message ID |
| `read_context` | Read one message with nearby messages | Defaults to 5 before and 5 after; maximum 25 each |
| `list_attachments` | Inspect downloadable media on one message | Returns typed attachment metadata |
| `download_attachment` | Download message media to the managed cache | Streams into an opaque attachment directory, stops at the hard byte limit, removes partial files, and publishes an immutable content-addressed original with SHA-256 |
| `prepare_message` | Persist an immutable text-send draft | Binds account, chat, text, and optional reply target into a payload hash |
| `get_prepared` | Recover one prepared text draft | Returns the exact persisted payload and hash after a restart; may initialize or migrate the local ledger |
| `send_prepared` | Send a prepared text draft | Verifies the selected session is still the prepared account; repeated calls with the same `draft_id` are idempotent |
| `send_message` | Send text directly | Compatibility path; non-idempotent and not recommended for recoverable workflows |
| `send_file` | Upload and send a managed local file | Requires the exact approved SHA-256, captures a private immutable snapshot before upload, rejects cross-account/outside/symlink-escape paths, and remains non-idempotent |

`search_messages` returns `completeness` as `complete`, `page`, or
`first_page_only`. When `chat_id` is omitted, Ferogram does not expose the full
Telegram global-search cursor needed for reliable pagination, so the result is
intentionally labelled `first_page_only` and has no claimed next page. Use
dialog discovery followed by in-chat search when complete traversal matters.

### Canonical peer references

Every reference returned for later tool calls is round-trippable:

- positive numeric IDs identify users;
- negative numeric IDs identify basic groups;
- `-100...` IDs identify channels and supergroups;
- a dialog's `chat_ref` uses `@username` when one exists, otherwise its
  `canonical_chat_id`;
- message `chat_ref` and `sender_ref` fields use canonical numeric peer
  references.

The internal Ferogram dialog ID is not exposed through MCP. Tool inputs accept a
username or one of the canonical numeric forms above; phone numbers, Telegram
links, and invite links are rejected rather than guessed.

## Structured Results

Once arguments have deserialized, domain-level success and failure use the same
declared output schema:

```json
{
  "data": { "...": "tool-specific result" }
}
```

```json
{
  "error": {
    "code": "MESSAGE_NOT_FOUND",
    "message": "...",
    "retryable": false
  }
}
```

This lets an MCP client branch on typed domain error codes instead of parsing
prose. Malformed or unknown input fields are rejected by the MCP protocol
boundary itself and may be returned in the SDK's standard invalid-params error
shape rather than the tool outcome envelope.

## Quick Start

### 1. Requirements

- a current stable Rust toolchain;
- an MCP client that can launch a stdio server.

Demo mode needs nothing else. Real Telegram mode also needs Telegram `api_id`
and `api_hash` from the official
[`my.telegram.org` application page](https://my.telegram.org/apps);
`auth login` can create the authorized Ferogram `.session` file. Telegram
account signup and Desktop `tdata` import are not implemented.

### 2. Install

Install the current prerelease from its exact Git tag:

```bash
cargo install --locked \
  --git https://github.com/Stranmor/telegram-personal-mcp.git \
  --tag v0.1.0
```

The project is not published to crates.io. For a source checkout or local
development:

```bash
git clone https://github.com/Stranmor/telegram-personal-mcp.git
cd telegram-personal-mcp
cargo install --locked --path .
```

### 3. Create or select a Telegram session

Inject the API hash through a secret store or an ephemeral environment rather
than committing it to an MCP configuration file:

```bash
export TELEGRAM_API_ID=12345
read -rsp "Telegram API hash: " TELEGRAM_API_HASH; echo
export TELEGRAM_API_HASH
export TELEGRAM_MCP_SESSION_PATH="$HOME/.local/share/telegram-personal-mcp/account.session"
```

`.env.example` inventories the application-specific runtime settings, but the
binary does not automatically load `.env` files.

Create or refresh the selected session from an interactive terminal:

```bash
telegram-personal-mcp auth login
```

The command prompts for a phone number, requests a Telegram login code, and
handles optional two-step verification. The login code and 2FA password are read
without terminal echo and cannot be supplied through command arguments or a
pipe. On success it prints account metadata and the saved session path as JSON.

When no explicit path is set, `auth login` reuses the only session candidate or
creates `default.session` inside the resolved XDG data session directory when
the cache is empty. Multiple candidates remain fail-closed; select one explicitly
before retrying. An already-authorized selected session returns
`already_authorized` without requesting another code. On Unix, the session
parent directory must be owner-only; newly created parents use mode `0700` and
session/lock files use `0600`.

Inspect the resolved session without connecting to Telegram:

```bash
telegram-personal-mcp session list
```

Then verify the selected session and account with a real `get_me` call:

```bash
telegram-personal-mcp doctor
```

If more than one `.session` file is visible in the session cache, `doctor` and
Telegram tool calls fail with `AMBIGUOUS_SESSION` before opening an arbitrary
account. Select one explicitly with `TELEGRAM_MCP_SESSION_PATH` or
`--session-path`.

### 4. Register the stdio server

MCP client configuration formats are not universal. Configure your host to:

1. launch `telegram-personal-mcp` with no subcommand;
2. inherit `TELEGRAM_API_ID` and the optional session selection variables;
3. inject `TELEGRAM_API_HASH` from its secret store or process environment
   rather than embedding the value in repository or host configuration;
4. optionally inject `TELEGRAM_MCP_PROXY` through the environment when a
   SOCKS5 proxy is required.

Protocol messages use stdout; diagnostics use stderr. `TELEGRAM_API_HASH` and
`TELEGRAM_MCP_PROXY` intentionally have no command-line flags so secrets do
not appear in process arguments.

## Recommended Text-Send Flow

Use `prepare_message` followed by `send_prepared` instead of calling
`send_message` directly:

1. Call `prepare_message` with the exact chat, text, and optional reply target.
2. Present the returned account, chat, text, reply target, and `payload_sha256`
   to the user, then obtain approval for that exact payload and destination in
   the current session. If the process restarted, recover the same record with
   `get_prepared(draft_id)` instead of reconstructing it from chat memory.
3. Call `send_prepared` with its `draft_id` and the exact returned hash as
   `approved_payload_sha256`. A mismatch fails with
   `PAYLOAD_HASH_MISMATCH` before dispatch. If the session now belongs to a
   different Telegram account, dispatch fails with `ACCOUNT_MISMATCH`.
4. If the result is `DELIVERY_UNCERTAIN`, retry `send_prepared` with the same
   `draft_id` and `approved_payload_sha256`; do not create a new draft.

```text
prepare_message({ chat_id: "@alex", text: "Approved reply", reply_to_message_id: 418 })
  -> draft_id: "...", payload_sha256: "8d..."

get_prepared({ draft_id: "..." })
  -> exact persisted payload and payload_sha256: "8d..."

send_prepared({
  draft_id: "...",
  approved_payload_sha256: "8d..."
})
```

The SQLite ledger preserves the Telegram `random_id` and final receipt across
process restarts. Replaying a sent draft returns its recorded message ID. This
reduces duplicate-send risk, but it does not turn Telegram network delivery into
a general distributed transaction.

`approved_payload_sha256` makes the send call technically match the immutable
draft that was shown for approval. Human authorization remains entirely
host-owned: the MCP host must show the exact recipient and payload, obtain
current approval, and invoke the tool with the matching hash. The server does
not accept a caller-supplied string that pretends to prove approval.

`send_file` similarly requires `approved_sha256` for the exact local bytes.
It snapshots those bytes before upload so a concurrent file change cannot alter
the approved payload. `send_file` and direct `send_message` are
non-idempotent; a host must not retry either after `DELIVERY_UNCERTAIN`.

## Configuration

| Environment variable | CLI option | Default / meaning |
| --- | --- | --- |
| `TELEGRAM_MCP_DEMO` | `--demo` | Deterministic local MCP fixture; no credentials or Telegram connection |
| `TELEGRAM_API_ID` | `--api-id` | Required outside demo mode: Telegram application ID |
| `TELEGRAM_API_HASH` | — (environment only) | Required outside demo mode: Telegram application secret |
| `TELEGRAM_MCP_SESSION_PATH` | `--session-path` | Exact `.session` file to use |
| `TELEGRAM_MCP_SESSION_CACHE_DIR` | `--session-cache-dir` | XDG data path described below |
| `TELEGRAM_MCP_PROXY` | — (environment only) | Optional SOCKS5 proxy URL |
| `TELEGRAM_MCP_ATTACHMENT_CACHE_DIR` | `--attachment-cache-dir` | XDG cache path described below |
| `TELEGRAM_MCP_MAX_DOWNLOAD_BYTES` | `--max-download-bytes` | `67108864` bytes (64 MiB) |
| `TELEGRAM_MCP_MAX_UPLOAD_BYTES` | `--max-upload-bytes` | `67108864` bytes (64 MiB) |
| `TELEGRAM_MCP_STATE_DB` | `--state-db-path` | XDG state path described below |
| `RUST_LOG` | — | Optional stderr log filter |

The server exposes one configured account/session to every MCP tool. Session
resolution order is:

1. `TELEGRAM_MCP_SESSION_PATH` / `--session-path`;
2. otherwise, the only `.session` file in
   `TELEGRAM_MCP_SESSION_CACHE_DIR`.

There is no tool-level `session_path`; an ambiguous cache fails closed with
`AMBIGUOUS_SESSION`.

A selected session is protected by an adjacent `.session.lock` file. Do not run
multiple MCP processes against the same Telegram session.

CLI control commands are:

- `telegram-personal-mcp auth login` — interactively create or refresh a
  session;
- `telegram-personal-mcp session list` — inspect candidate paths without
  connecting;
- `telegram-personal-mcp doctor` — connect and verify the selected account;
- `telegram-personal-mcp` or `telegram-personal-mcp --demo` — serve MCP over
  stdio.

### Local data paths

The session cache defaults to:

1. `$XDG_DATA_HOME/telegram-personal-mcp/sessions` when set;
2. otherwise `$HOME/.local/share/telegram-personal-mcp/sessions`;
3. otherwise the operating system temporary directory.

Attachments default to:

1. `$XDG_CACHE_HOME/telegram-personal-mcp/attachments` when set;
2. otherwise `$HOME/.cache/telegram-personal-mcp/attachments`;
3. otherwise the operating system temporary directory.

The prepared-send SQLite ledger defaults to:

1. `$XDG_STATE_HOME/telegram-personal-mcp/state.sqlite` when set;
2. otherwise `$HOME/.local/state/telegram-personal-mcp/state.sqlite`;
3. otherwise the operating system temporary directory.

Each downloaded attachment is stored as:

```text
<attachment-cache>/<account-key>/<attachment-key>/<full-sha256>-<sanitized-filename>
```

The two directory keys are deterministically derived from the selected account
and attachment identity, but do not expose the raw account/chat/message
references in the path. Completed originals are content-addressed and immutable
(`0400` on Unix); private cache directories use `0700`. Do not overwrite the
downloaded `local_path`. Create a new sibling path for a modified copy that
will be passed to `send_file`.

Before upload, `send_file` resolves the selected account, then canonicalizes both
that account's cache root and the requested file. An outside path, a file cached
for another account, or a symlink that resolves outside the account root fails
with `FILE_OUTSIDE_UPLOAD_ROOT`. This prevents the tool from becoming an
unbounded path selector and blocks direct cross-account cache reuse. It is not a
filesystem sandbox: a caller that can already copy arbitrary bytes into the
account cache can still ask the tool to send that copied file, so the MCP host
must keep untrusted Telegram content separate from local-file and send authority.

Before upload, `send_file` copies the selected file into a private outbox
directory beside the ledger (the default is `state.outbox/<uuid>/`; a custom
database path has its extension replaced with `.outbox`), hashes the snapshot,
compares it with `approved_sha256`, makes the snapshot immutable, and uploads
that exact copy. The directory is removed after the send attempt. A process
crash can leave a private snapshot behind; inspect and remove stale outbox
entries only while the server is stopped.

The attachment cache contains downloaded and modified Telegram files. The state
database contains prepared message text, canonical chat references, payload
hashes, Telegram `random_id` values, delivery state, and receipts; it does not
store session paths. Opening a legacy ledger removes the obsolete
`session_path` column while preserving draft and receipt data. State directories
use `0700` and the SQLite database uses `0600` on Unix when the project creates
their default/private parent. A custom pre-existing parent keeps its existing
permissions and must already be protected by the operator. Neither store is
encrypted or automatically purged by this project. Apply an appropriate
retention policy.

## Operational Error Semantics

Important stable error codes include:

- `ACCOUNT_MISMATCH`: the currently selected Telegram account differs from the
  account bound to a prepared draft;
- `FILE_HASH_MISMATCH`: the captured upload snapshot does not match
  `approved_sha256`;
- `CACHE_CONFLICT`: an immutable content-addressed download path already
  exists with different bytes and is preserved;
- `FLOOD_WAIT`: Telegram requires a delay; the result includes
  `retry_after_seconds`;
- `DELIVERY_UNCERTAIN`: dispatch may have reached Telegram.

For `send_prepared`, retry `DELIVERY_UNCERTAIN` with the same `draft_id` and
hash so the stable Telegram `random_id` can prevent a duplicate. For
`send_message` or `send_file`, do not retry blindly because those compatibility
paths are non-idempotent. A deterministic rejection returns `SEND_REJECTED`;
a known pre-dispatch retryable failure returns `SEND_RETRYABLE`.

## Scope and Limitations

- No embedded model, autonomous agent, agent identity, or multi-agent routing.
- No durable Telegram update subscription or no-loss event queue.
- Interactive login supports existing Telegram accounts; signup and automatic
  `tdata` conversion remain out of scope.
- Global message search exposes only the first result page.
- Prepared/idempotent delivery currently covers text, not files.
- No delete tool and no single-call unbounded history export.
- Downloads stop during streaming at the configured hard limit and are hashed,
  but are not malware-scanned.
- File upload is intentionally restricted to canonical paths inside the selected
  account's managed attachment cache.
- Telegram text and files are external, untrusted data.

See [SECURITY.md](SECURITY.md) for the trust boundaries and local-data details.

## Development

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo package --locked --no-verify
```

Focused boundary tests include:

```bash
cargo test --test stdio_demo
cargo test --test download_limit
cargo test --test mcp_integration
```

The `TelegramBackend` trait keeps the MCP layer testable without live Telegram
credentials. See [CONTRIBUTING.md](CONTRIBUTING.md) before changing tool schemas,
send behavior, session selection, or local persistence.

Project resources: [Changelog](CHANGELOG.md) · [Support](SUPPORT.md) ·
[Contributing](CONTRIBUTING.md) · [Security](SECURITY.md)

## License

Licensed under either of:

- Apache License, Version 2.0
- MIT license

at your option.
