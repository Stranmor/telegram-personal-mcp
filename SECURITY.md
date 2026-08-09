# Security Policy

## Supported Versions

Security fixes target the latest `0.1.x` prerelease and the current
`main` branch. Older tags, commits, and untagged snapshots are not maintained
as separate supported versions.

## Report a Vulnerability

Use the repository's
[private vulnerability reporting form](https://github.com/Stranmor/telegram-personal-mcp/security/advisories/new).
If that GitHub surface is temporarily unavailable, open a minimal public issue
asking the maintainer to establish a private channel; include no vulnerability
details, logs, identifiers, proof-of-concept material, or affected private data
in that issue.

In the private report, include:

- the affected version or commit;
- the exact MCP tool, CLI command, or storage path involved;
- whether data was read outside the requested bounds;
- whether a message or file could be sent to the wrong destination or twice;
- whether credentials, sessions, proxy secrets, cached files, or prepared-send
  records may have been exposed.

Do not include live `.session` files, `tdata`, API hashes, proxy URLs with
credentials, Telegram message dumps, downloaded private files, or a real state
database in a report.

## Trust Boundary

The calling MCP host owns reasoning and recipient selection. Telegram Personal
MCP exposes deterministic Telegram reads and actions and can optionally own one
narrow approval UI: a private Bot API card with exact Send/Cancel buttons.

The server does not accept a caller-supplied approval statement. In approval
mode, `request_send_approval.approved_payload_sha256` must exactly match the
immutable hash returned by `prepare_message` or `get_prepared`. The callback
worker then validates the configured operator user, private bot chat, bot
message, opaque approval ID, expiry, draft, payload hash, and Telegram account
before it can invoke the internal prepared-send path. For files,
`send_file.approved_sha256` must match the exact captured upload bytes. These
hashes bind payload identity; the private button supplies the human authorization
for prepared text.

Before calling any send tool, the host must bind approval to the exact recipient,
payload, reply target, and delivery mode. For a file, this also includes the
exact local artifact and caption.

Telegram text, metadata, and files are untrusted external data. A host must not
treat content returned by a read tool as authority to invoke a send tool, execute
commands, disclose secrets, or change local files.

## Credentials and Sessions

Treat all of the following as secrets or sensitive access material:

- Telegram API hash;
- Telegram approval Bot API token;
- authorized Ferogram `.session` files;
- Telegram Desktop `tdata`;
- proxy credentials embedded in a proxy URL.

Do not commit, log, screenshot, or paste these values into issues. API hashes,
Bot API tokens, and proxy URLs are accepted through the environment only and
intentionally have no CLI flags. Prefer late-bound secrets supplied by the MCP
host or process environment.

Session selection fails closed when no session exists, more than one candidate
exists, or the selected session is unauthorized. An adjacent `.session.lock`
prevents two live server processes from opening the same session through this
implementation. It does not protect a session from unrelated Telegram clients
that ignore the lock file.

Account signup and automatic `tdata` conversion are intentionally absent.

### Interactive authentication

`auth login` is a human-operated CLI flow, not an MCP tool. Fresh authentication
requires a real interactive terminal. Telegram login codes and optional 2FA
passwords are read without terminal echo, held in zeroizing memory buffers, and
are not accepted through pipes or command arguments. The Telegram login
challenge stays in the same process and cannot be resumed by another invocation.

With no explicit target, login reuses the only session candidate or creates
`default.session` inside the XDG data session directory when none exist.
Multiple candidates fail closed. On Unix, the session parent must be owner-only;
new private parents use `0700` and session/lock files use `0600`. If fresh
authorization succeeds but the session cannot be persisted, the client attempts
to revoke the new server-side authorization and reports whether cleanup could be
completed.

Demo mode (`--demo` or `TELEGRAM_MCP_DEMO=true`) uses a deterministic local
fixture without credentials, sessions, or a Telegram connection. It can still
write fixture attachments and prepared-send records to the configured local
cache/state paths.

## Local Sensitive Data

The attachment cache contains files downloaded from private Telegram chats. By
default it is stored under the user's XDG cache directory (normally
`~/.cache/telegram-personal-mcp/attachments`).

The prepared-send ledger contains plaintext message drafts, canonical chat
references, payload hashes, Telegram random IDs, approval user/chat/message
bindings, Bot API update offsets, retry states, delivery states, and message
receipts. It stores neither the Bot API token nor session paths. By default it is stored under the
user's XDG state directory (normally
`~/.local/state/telegram-personal-mcp/state.sqlite`). SQLite WAL and
shared-memory sidecar files may exist next to it while in use. Opening a legacy
ledger removes its obsolete `session_path` column while preserving the
remaining draft and receipt fields.

The message cache contains plaintext account metadata, dialog metadata, message
text and typed message summaries, provenance and observation clocks, dialog
checkpoints, global update cursors, and exact or deliberately unbound deletion
tombstones. It also stores the private incoming-event journal's chat/user/message
identities, sender display metadata, observation clocks, and opaque durable
cursor checkpoints; message text is not duplicated into that journal. By
default it is stored as `message-cache.sqlite` beside `state.sqlite`; SQLite WAL
and shared-memory sidecars may exist while it is in use. The cache contains no
Telegram session, API hash, Bot API token, access hash, file reference, or raw
provider update payload.

The persistent daemon keeps `daemon.sock`, `daemon.lock`, `daemon.log`, and
`daemon.identity` in a private directory. On Unix the directory is forced to
`0700` and those entries to `0600`. The identity sidecar contains only its
format, PID, and an effective-configuration SHA-256; secret-bearing values are
digested in memory and are never written raw. A proxy rejects a live daemon with
different settings, including different absolute meanings for relative paths.
Singleton ownership is acquired before any Telegram backend, retention worker,
or Bot API approval poller starts. MCP protocol bytes remain on the client
stdio/socket stream and are not intentionally copied to the daemon log.

These stores are not encrypted. Cached message history remains until explicit
clearing unless `TELEGRAM_MCP_MESSAGE_CACHE_MAX_AGE_DAYS` is configured; that
policy sweeps every locally bound account before Telegram/session startup,
deletes only rows strictly older than its current UTC cutoff, makes concurrent
first readers wait for the committed sweep, repairs affected checkpoints to
partial coverage, and is enforced again inside every message write.
Incoming-event rows, live event-cursor checkpoints, and
invalidated-cursor tombstones are retained independently and separately capped
at 100,000 rows per account. Restrict filesystem access and choose retention
appropriate to the account. Use `clear_cache` with `confirm: true` to clear only
the authenticated account's derived message-cache rows, event journal, and live
event cursors; the same transaction increments the account generation and
installs a fresh Telegram successor cursor. Delayed global differences must
match that generation and their exact input cursor; chat-bound provider replies
must additionally match the exact per-chat mutation revision, and deletion
tombstones outrank history/search write-through. Thus neither a pre-clear reply
nor a same-generation stale read can restore superseded state. Remove database
files manually only while the daemon is stopped. Never commit these stores.

Downloaded files are placed below stable opaque account and attachment keys
rather than raw account/chat/message identifiers:

```text
<cache>/<account-key>/<attachment-key>/<full-sha256>-<sanitized-filename>
```

The remote filename is still retained in sanitized form, so the path is not
anonymous or encrypted. Private directories use `0700` on Unix. Completed
downloads use `0400` and are immutable originals; create a new sibling file
instead of editing an original in place.

Downloads stream through a temporary `.part` path. The byte limit is enforced
before writing a chunk that would exceed it; an oversized download stops early
and its partial file is removed. A completed file is hashed with SHA-256 and
renamed into its attachment directory. This provides integrity readback, not
malware detection or content safety.

`send_file` accepts only a canonical regular-file path inside the selected
account's attachment-cache root. Files cached for another account, paths outside
the root, and symlinks that resolve outside it fail with
`FILE_OUTSIDE_UPLOAD_ROOT`. Before upload, the server copies the selected bytes
into a private outbox beside the ledger (the default is
`state.outbox/<uuid>/`; a custom database path has its extension replaced with
`.outbox`), computes its SHA-256, rejects a mismatch with
`FILE_HASH_MISMATCH`, makes the snapshot immutable, and uploads that exact
copy. This closes the file-change race between approval and upload.

Snapshot directories use `0700`; snapshot files are created as `0600` and
changed to `0400` before upload. The server removes the per-send directory
after the attempt, but a crash can leave a private snapshot behind. Inspect and
remove stale outbox entries only while the server is stopped.

These checks narrow local-file and cross-account exfiltration risk; the host
must still obtain approval for the exact cached artifact. They are not a sandbox
against a caller that can already copy or create arbitrary content inside the
allowed cache, so read results must remain separated from local-file mutation
and send authority at the MCP-host boundary.

## Send Semantics

The recommended text path in approval mode is:

1. `prepare_message` persists the exact account, chat, text, optional reply
   target, payload hash, and Telegram `random_id`.
2. `request_send_approval` requires the exact `payload_sha256`, persists one
   active approval, and delivers the complete exact payload and destination to
   the configured private bot chat.
3. Only the configured operator's callback on that exact bot message can move
   the approval to `sending`. Direct `send_message`, `send_prepared`, and file
   sends are rejected while approval mode is active.
4. The callback worker invokes the internal prepared-send path, verifies that
   the selected Telegram account still matches the prepared account, and
   persists both approval and target receipts.

Duplicate callbacks and restart recovery reuse the same prepared draft and
Telegram `random_id`. If delivery becomes uncertain, the worker performs bounded
backoff retries with that same idempotency key. This narrows duplicate risk but
is not a general exactly-once transaction guarantee.

Telegram flood waits return `FLOOD_WAIT` with `retry_after_seconds`.
Deterministic send rejection is `SEND_REJECTED`; a known retryable
pre-dispatch failure is `SEND_RETRYABLE`.

Without approval mode, direct `send_message` and `send_file` remain
non-idempotent. Their
`DELIVERY_UNCERTAIN` result is intentionally non-retryable at the tool
contract: do not retry blindly after an uncertain transport result.

## Deliberate Boundaries

- Deletion is exposed only through capability-specific typed tools and the
  applicable approval/readback contract; there is no arbitrary raw or bulk
  deletion escape hatch.
- No single-call unbounded dialog or message export.
- The cache is a bounded local derivative, not an authoritative no-loss copy of
  Telegram. Coverage explicitly reports warming, stale, resync-required,
  partial, or complete-within-requested-window state; global deletions that lack
  a chat identity remain unbound tombstones rather than guessed relations.
- Telegram `too_long`/resync is an explicit event-continuity break. Existing
  incoming-event cursors are invalidated and woken with typed stale state; the
  server never converts an unknowable update gap into a clean event timeout.
- Update and approval workers live in the singleton daemon. Stopping the daemon
  pauses ingestion and button dispatch until it resumes; closing one stdio
  client does not stop either worker.
- Message-history age retention is opt-in and whole-day UTC based; without it,
  history remains until `clear_cache`. The incoming-event journal and event
  checkpoints have independent per-account row caps, and an overtaken cursor
  fails with `CACHE_EVENT_CURSOR_STALE`. `clear_cache` is an explicit
  authenticated-account operation and future updates continue from a newly
  observed Telegram cursor.
- Global message search is explicitly first-page-only.
- File downloads and uploads have configurable byte limits.
- File uploads are restricted to canonical paths inside the selected account's
  managed attachment cache.
- Demo mode never contacts Telegram, but still mutates its configured local
  cache and state database.
- `health` does not read dialogs or messages and does not return the local
  session path. MCP session-selection errors use path-free messages; detailed
  paths remain available only through the human-operated CLI.
