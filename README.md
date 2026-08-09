# Telegram Personal MCP

[![CI](https://github.com/Stranmor/telegram-personal-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/Stranmor/telegram-personal-mcp/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Stranmor/telegram-personal-mcp?include_prereleases&sort=semver)](https://github.com/Stranmor/telegram-personal-mcp/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

A typed MCP control plane for AI agents working inside a Telegram personal
account: discover conversations, recover exact context, handle files and
incoming events, and execute verified actions.

The MCP host supplies the reasoning; this server supplies the account-scoped
Telegram workspace through a stdio proxy backed by one persistent per-user
daemon. Optional approval mode binds consequential sends to an exact prepared
payload, a configured operator callback, and terminal readback.

## Personal Telegram, as an Agent Workspace

[![Telegram Personal MCP agent workspace workflow](docs/assets/telegram-personal-mcp-demo.gif)](docs/assets/telegram-personal-mcp-demo.mp4)

The animation proves the core job in a deterministic isolated consumer: an MCP
host finds the right dialog and source message, reconstructs its exact context,
and freezes a typed reply. Approval mode then blocks direct dispatch, renders
one operator card, accepts the bound callback, records one fixture-backend
delivery, and reads the exact target back. A repeated callback must still leave
exactly one delivery.

This credential-free artifact proves workflow bindings, approval gating,
idempotency, and consumer readback; it does not claim a live Telegram round
trip. The checked-in [proof receipt](docs/assets/telegram-personal-mcp-demo-proof.json),
[integration harness](examples/approval_demo.rs), [visual surface](docs/demo/index.html),
and [renderer](scripts/render_demo.sh) make every claim reproducible.

The production runtime keeps reasoning and Telegram actuation separate:

```text
MCP host agent
  -> stdio proxy
  -> persistent per-user daemon
       -> bounded cache and event cursors
       -> optional approval bot and configured operator
       -> MTProto personal account
```

The implementation uses the official Rust MCP SDK, pinned to `rmcp 1.7.0`,
and targets the implemented stable MCP `2025-11-25`. The pin deliberately
avoids claiming or negotiating MCP `2026-07-28` before that contract is
implemented here. A regression test requests that version and verifies the
server falls back to `2025-11-25`. All 307 tools have input/output schemas, MCP
annotations, and a stable structured result envelope after argument
deserialization. The binary advertises 23 common tools directly and keeps the
full 307-tool catalog behind `search_tools`, `describe_tool`,
`invoke_read_only_tool`, and `invoke_tool`, avoiding a multi-megabyte discovery
payload for every MCP client without removing capability.

`v0.1.0` is the first public prerelease. Release evidence is deterministic
and credential-free: CI verifies the real binary, package, stdio transport, and
demo/fake workflow, while live Telegram authentication and delivery remain an
explicit prerelease boundary.

## Try It Without Telegram

Demo mode runs the real MCP binary against a deterministic local fixture. It
does not need Telegram credentials or a session and does not contact Telegram.

The fastest end-to-end proof launches the compiled binary as a child process,
connects an `rmcp` client over real OS stdio, finds the contract message, reads
its context, resolves and downloads provider-backed custom emoji, downloads the
fixture PDF, creates a modified copy, and sends that copy as a reply with exact
SHA-256 bindings:

```bash
cargo test --test stdio_demo
```

The test also emits two compact proof lines. The hashes below are stable for
the checked-in fixture and show both the negotiated protocol and the exact
downloaded/replied bytes:

```text
protocol-proof requested=2026-07-28 negotiated=2025-11-25
demo-proof tools=17 message_id=11 downloaded_sha256=8a9e0a420df39699e56b1cc65a12505d8300394566f3d7be014bc20d62a4901c custom_emoji_sha256=15a75fab8959ee6e9d73e0719d6d857adff498c56157431d0f8f77d2b1154678 file_reply_sha256=909a3dbb487550bd8d00c5d769649fe98f0212f0d7b6124f2ff839c4132ae60f
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

`TELEGRAM_MCP_DEMO=true` is equivalent to `--demo`. The default command still
uses the persistent daemon and exposes it to the MCP host through stdio; the
hidden `direct` subcommand is reserved for recovery and isolated tests. Demo
mode cannot be combined with `doctor`, `session`, or `auth`, and its tools can
still write fixture downloads, message-cache rows, and prepared-send state to
the configured local paths.

## What It Enables

The main text-send workflow is deliberately practical:

1. Find the right dialog and message.
2. Read the exact message plus a bounded window around it.
3. Persist the exact destination, reply target, text, account, source
   provenance, and payload hash with `prepare_message`.
4. Every prepared text or Rich Text send and every scheduled message must bind
   one explicit source state: an exact Telegram message, a validated public-web
   page, or `self_originated`. Telegram sources are read from the provider;
   public-web metadata remains inert and is never opened by the server.
5. When approval mode is configured, call `request_send_approval`; the dedicated
   bot renders the exact human decision surface and Send/Cancel buttons. The
   source identity, date, relation, and direct reference are always visible;
   legacy rows with no source are labelled explicitly instead of silently
   omitting provenance.
6. The configured operator clicks Send. The callback worker validates the exact
   user, configured approval chat, bot message, approval ID, draft, hash, expiry,
   and account, then dispatches through the personal MTProto account.
7. Read the durable final state with `get_send_approval`. Deployments without
   approval mode retain direct compatibility sends and their weaker guarantees.

No Telegram message text is treated as an instruction by the server. Reads are
bounded, consequential writes use immutable prepared state, and approval mode
adds the operator-backed callback worker. The server exposes no single-call
unbounded live-history export. Instead, one account-scoped SQLite cache records
bounded reconciliation, passive read-through, and Telegram update differences
with explicit freshness, provenance, and completeness state.

## Engineering Depth

The project is designed around failure modes that a thin Telegram wrapper
usually leaves to its caller:

| Failure mode | Mechanism |
| --- | --- |
| A misspelled send field silently changes behavior | Closed input schemas reject every unknown field at the MCP boundary |
| A process dies around a text send | Prepared drafts persist state, Telegram `random_id`, payload hash, and final receipt across restarts |
| An agent tries to bypass the button | Approval mode rejects direct `send_message`, `send_prepared`, and file sends; only the validated callback can invoke the internal prepared-send path |
| A callback is copied, stale, or clicked by another user | The worker binds the configured operator, configured approval chat, bot message ID, opaque approval UUID, expiry, draft, hash, and account before actuation |
| Telegram accepts a send before the process records it | Restart recovery reuses the prepared draft's stable `random_id`, reads back the same target receipt, and never creates a replacement draft |
| The configured session is replaced before dispatch | The internal prepared-send path revalidates the live Telegram account against the account bound at prepare time |
| An agent blindly invokes callback data or follows a button URL | `get_message_buttons` returns a typed untrusted inventory; `press_button` accepts only a callback that still exactly matches the approved message position and SHA-256/length snapshot, never opens URLs/native UI, and never persists raw callback bytes |
| A local file changes after approval | `send_file` snapshots exact bytes, verifies `approved_sha256`, then uploads the immutable snapshot |
| A caller selects an arbitrary or cross-account file | Upload paths must resolve inside the selected account's canonical cache root |
| An SDK advertises a protocol contract this server does not implement | The SDK generation and server version are pinned, with a raw-stdio negotiation regression test |

The Bot API is only the approval control surface. The actual outbound message is
still sent by the configured personal account over MTProto after the validated
button transition.

The default executable is a byte-transparent stdio proxy to one singleton
per-user Unix daemon. The first proxy starts the daemon; later MCP clients reuse
the same backend, MTProto session, approval worker, update cursor, and message
cache. Closing a client transport does not stop those background workers. The
daemon directory is owner-only, and its socket, lock, log, and configuration
identity are owner-readable only. A proxy reuses a live daemon only when the
full effective runtime configuration matches, including retention and absolute
path meaning. A mismatch fails closed: stop the existing daemon or select a
separate `TELEGRAM_MCP_DAEMON_DIR`. During a compatible frontend-only rollout,
`TELEGRAM_MCP_DAEMON_EXECUTABLE` may bind the proxy to one exact absolute daemon
binary. The proxy hashes those bytes and, if the daemon is absent, starts that
same binary; it never opens the authenticated session through the newer
frontend executable by implication. Telegram readiness is proved by the real
connection and account-health request—matching host and SOCKS egress addresses
are diagnostic information, not a startup failure.

Bot API updates also have one owner. When another service is the canonical
`getUpdates` poller, set `TELEGRAM_MCP_APPROVAL_FANOUT_DB` to its owner-only,
typed callback fan-out database. The MCP reads callbacks there while retaining
outbound approval-card calls. Without fan-out mode, a token-scoped filesystem
lease prevents a second direct poller from reaching Telegram.

Incoming-message waits reuse the daemon's single global Telegram update cursor;
they do not start a second update stream. `wait_for_new_message` waits for new
eligible private messages and returns every pending private chat with its count
and last message ID. `wait_for_settled_message` returns one deterministic chat
burst after its quiet interval, while its per-chat cursor state preserves any
interleaved burst for the next call. Only incoming, non-self, non-bot private
messages with an exact related-user identity are journalled; groups, channels,
outgoing messages, edits, and identity-ambiguous updates are excluded.

Omitting `cursor` creates a durable checkpoint at the current journal head, so
the call waits only for future events. A returned opaque cursor can be reused
for deterministic at-least-once observation by another client, and successor
cursors survive daemon restarts. The journal and cursor-checkpoint sets are each
bounded to 100,000 rows per account. When retention overtakes a cursor, the tool
fails with `CACHE_EVENT_CURSOR_STALE` instead of silently skipping events.
Checkpoint-retention, cache-clear, and Telegram `too_long`/resync invalidations
leave bounded private tombstones, so a genuinely expired cursor remains
distinguishable from a fabricated token while that bounded invalidation history
is retained. A resync never carries an event cursor across the unknowable
provider gap: active waits wake and fail stale, while a new cursor is rejected
with `CACHE_EVENT_STREAM_RESYNC_REQUIRED` until the daemon has rebound the
Telegram update cursor.

Every asynchronous cache writer captures the account generation before its
Telegram fetch. Global differences additionally compare their exact input
update cursor; chat-bound history, export, reconciliation, and passive reads
compare an exact per-chat mutation revision as well as their frontier and
checkpoint where applicable. Provider deletion tombstones also outrank later
history/search replies. Therefore `clear_cache` is a real linearization
boundary, and a same-generation stale read cannot overwrite a newer edit or
restore a deleted message.

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

get_custom_emoji_documents({ document_ids: [8001] })
  -> safe sticker metadata and opaque sticker_handle; no access_hash/file_reference

download_custom_emoji({ document_id: 8001 })
  -> account-scoped immutable custom-emoji file, bytes_written, sha256

send_gif({ chat_id: "@alex", file_path: ".../animation.gif", approved_sha256: "<sha256>" })
  -> immutable GIF mutation with magic-byte validation and operator approval

get_dice_emojis({})
  -> live Telegram dice/game emoji capability catalog and reusable hash

send_location({ chat_id: "@alex", latitude: 55.7558264, longitude: 37.6172999 })
  -> e7-normalized immutable location mutation with operator approval and exact readback

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

The internal catalog contains 307 closed-schema tools and is verified
in-process. The compiled stdio surface advertises 23 common first-class tools;
agents find and invoke the remainder through the four typed discovery/gateway
tools. Thus all 307 tools are gateway-reachable, while the live MCP `tools/list`
response remains authoritative for the compact direct surface.

| Family | Representative tools | Contract |
| --- | --- | --- |
| Dialogs and context | `list_dialogs`, `get_unread`, unread mention/reaction/poll-vote tools, `search_messages`, `read_context`, `read_topic_messages`, `get_message_link`, `get_web_preview` | Bounded live reads with explicit completeness where Telegram pagination is unavailable |
| Update cursors | `get_updates_state`, `get_updates`, `get_channel_updates` | Exact stateless global and channel polling with bounded current TL requests, canonical next/resync cursors, explicit `empty`/complete/`slice`/`too_long` states, grouped deletion metadata, safe update kinds, channel snapshots, and no raw provider payloads or access hashes |
| Persistent message cache and events | `search_regex`, `export_chat`, `clear_cache`, `sync_messages`, `chat_analytics`, `message_timeline`, `today_messages`, `export_cached_messages`, `wait_for_new_message`, `wait_for_settled_message` | Account-scoped SQLite WAL storage with passive write-through, bounded dialog reconciliation, generation/global-cursor/per-chat-revision CAS commits, tombstone-wins edit/delete ingestion, optional continuously enforced UTC age retention, restart and fail-closed `too_long` recovery, regex/analytics/export views, plus durable private-human incoming-event waits. Opaque cursors are restart-safe and independently replayable; debounce consumes one chat burst without stealing interleaved chats; clear and resync wake active waits, invalidate old event cursors with typed stale state, and never admit a late pre-boundary cache write |
| Messages and schedules | `prepare_message`, `prepare_forward_messages`, `prepare_delete_messages`, `prepare_restore_deleted_messages`, `send_typing`, scheduled-message tools, edit/pin/reaction tools, `translate_message`, transcription tools, and fact-check tools | Exact target and prior-state binding before approval; deletion is snapshot-first and fails closed when content cannot be recreated unless irreversible deletion is explicitly allowed; restoration recreates the approved content under new Telegram message IDs with deterministic idempotency keys and explicit sender-change opt-in; typing/upload/cancel indicators never replay an uncertain provider call, quota-bearing translation and initial transcription are approval-gated, transcription polling requires a prior applied request, and fact-check changes use terminal readback |
| Message buttons | `get_message_buttons`, `press_button` | Exact or recent-message discovery; full callback/URL/switch/profile/copy/request-button typing; approval binds canonical chat, message, position, label, callback SHA-256 and length; dispatch refetches the live markup, rejects non-callback and 2FA buttons, never opens returned URLs, and never replays an uncertain callback |
| Inline bots | `get_inline_bot_results`, `send_inline_bot_result` | Empty-query and offset pagination support with typed result/media/send previews (including exact E7 geo/live-location controls), bot/context identity, switch-PM/WebView metadata, and decimal query IDs; sends accept only a live account-scoped result cached by this MCP, cap approval TTL to query expiry, bind target/reply/flags/stable `random_id`, bind a fully represented draft snapshot before `clearDraft`, never open URLs, and recover uncertain receipts idempotently without duplicate delivery |
| Bot account commands | `get_bot_commands`, `set_bot_commands` | Exact authenticated-bot identity binding for the competitor default scope and `en` default language, typed ephemeral command readback, full-list replacement/reset through private hash approval, stale-precondition rejection, and idempotent recovery after an uncertain provider receipt |
| Account emoji status | `get_emoji_status`, `list_emoji_statuses`, `set_emoji_status`, `clear_recent_emoji_statuses` | Hash-aware default/recent/channel-default/collectible catalogues; exact decimal IDs, expiry, title/slug, pattern document and four collectible colors; current-account readback; approval TTL capped to status expiry; stale-state rejection and idempotent uncertain-receipt recovery for set/clear operations |
| Profile identity | `get_account_profile`, `set_profile_color`, `set_personal_channel` | Full user/profile readback includes exact simple or collectible name/profile colors and featured channel/message identity; writes use private hash approval, canonical channel resolution, provider readback, and non-overwriting stale/uncertain recovery |
| Dialog folders | `list_folders`, folder CRUD/reorder/membership, `get_suggested_folders`, `toggle_folder_tags` | Full ordered folder filters and Telegram-suggested templates retain descriptions, categories and peer sets; Premium tag changes bind exact prior state, require operator approval, and recover an uncertain receipt without duplicate writes |
| Global privacy | `get_global_privacy_settings`, `set_global_privacy_settings` | Reads every field in the current TL constructor, including paid-message Stars and disallowed-gift controls; partial writes preserve omitted fields, support exact set/clear semantics, and reject stale or third-state recovery writes |
| Account sessions | `get_sessions`, `prepare_terminate_session`, `prepare_terminate_all_other_sessions` | Exactly one current authorization is required; single-session and reset-all mutations bind immutable device/app/IP/activity snapshots, never include the active MCP session, reject newly created or changed sessions after preparation, and recover uncertain reset receipts by full provider readback |
| Chat auto-delete | `get_auto_delete`, `set_auto_delete` | Exact private/group/channel history-TTL readback; `chatId`/`period` competitor aliases; operator hash approval for enable/change/disable; stale-state rejection; and idempotent uncertain-receipt recovery without overwriting a later third state |
| Chat moderation | `get_chat_moderation_settings`, `set_chat_permissions`, `set_chat_reactions`, `toggle_anti_spam`, `toggle_forum_mode` | Exact current TL readback for default banned rights, available reactions, reaction limits, native anti-spam, forum mode and forum tabs; permission patches merge allowed-capability semantics without losing omitted rights; disabling a forum requires explicit confirmation; all writes use operator approval, stale-state rejection, provider readback, and no duplicate call after an uncertain receipt |
| Drafts | `list_drafts`, `save_draft`, `clear_draft`, `clear_all_drafts` | Per-dialog and account-wide workflows; every draft exposes a SHA-256 of the complete TL state (including reply/entities/media/effect); global clear binds the full canonical set and never erases a draft created or changed after approval |
| Saved Messages | `search_saved_messages`, `get_saved_dialogs`, `get_saved_history`, `delete_saved_history`, `toggle_saved_dialog_pin`, Saved reaction-tag reads/updates, and default tag reactions | Origin-scoped pagination and tag/date filters; destructive range deletion, pin state, and tag-title changes bind exact prior state, require operator approval, and recover uncertain outcomes by provider readback without replay |
| Quick replies | `get_quick_replies`, `get_quick_reply_messages` | Hash-aware shortcut and stored-message reads with competitor `shortcutId`/`ids` aliases; complete top-message summaries, regular/service distinction, service action kind, shortcut binding, reply metadata, and related chat/user context are preserved |
| Business chat links | `get_business_chat_links`, `resolve_business_chat_link`, `create_business_chat_link`, `edit_business_chat_link`, `delete_business_chat_link` | Exact current TL contracts preserve slug, untrusted URL, rendered message, exact entity-vector SHA-256, title, views and related peers; writes use immutable operator approval and provider readback, create never replays an uncertain receipt, and edit/delete recover without duplicate calls; returned URLs are never opened automatically |
| Business account settings | `get_account_profile`, `set_business_hours`, `set_business_location`, `set_business_greeting`, `set_business_away`, `set_business_intro` | Full-profile readback preserves server-derived open state, normalized minute-of-week schedules, E7 location, exact recipients, quick-reply shortcut bindings, away schedule, and safe intro sticker handles; writes use immutable operator approval, reject raw document credentials and non-writable `open_now`, and recover uncertain receipts without duplicate provider calls |
| Group calls | `get_group_call`, `get_group_call_participants` | Exact active/discarded call metadata plus bounded initial and paginated participant reads; peer/source filters, current participant flags, video/presentation source groups, raise-hand and paid-Stars state, and related chats/users are preserved without exposing Telegram access hashes |
| Stars and gifts | `get_stars_status`, `get_stars_transactions`, `get_available_star_gifts`, `get_saved_star_gifts`, `get_stars_topup_options`, `get_stars_subscriptions`, `save_star_gift`, `convert_star_gift`, `change_stars_subscription` | Exact wallet balances, transaction flags/cursors, available and saved regular/unique gift metadata, current saved-gift filters, top-up tiers, and subscription pages; show/hide and subscription changes use immutable operator approval plus idempotent readback recovery, while irreversible gift conversion binds the exact gift/value/balance and disables replay after an uncertain provider receipt; provider bot payloads, chat-invite hashes, prepaid-upgrade hashes, addresses, and access hashes are omitted or reduced to safe presence/hash metadata |
| Stickers and custom emoji | `get_recent_stickers`, `add_recent_sticker`, `get_sticker_set`, `search_sticker_sets`, favourite/install/uninstall tools, `send_sticker`, and custom-emoji lookup/download | Ordinary and attached recent catalogues remain separate and hash-aware; add/remove binds an exact opaque sticker-set handle, requires operator approval, rejects incomplete observations, and recovers an uncertain receipt by terminal catalogue readback without replay; raw access hashes and file references never enter MCP output |
| Files and rich media | `list_attachments`, `download_attachment`, custom-emoji lookup/download, `prepare_file_message`, `send_gif`, `get_dice_emojis`, `send_location`, `send_venue`, `send_contact`, `send_dice`, `send_file` | Managed account cache, provider re-resolution without caller-supplied access hashes, immutable byte snapshots, live capability validation, canonical e7 coordinates, magic-byte media validation, streaming size bounds, SHA-256 binding, stable send IDs and typed readback |
| Chats and members | lifecycle, settings, typed/filterable admin log, pageable participant inventory, member/admin/restriction, durable batch invitation, invite-link and join-request tools | Canonical peer identities; detailed `recent`/`admins`/`kicked`/`banned`/`bots`/`contacts` pages with exact totals, search and next offsets; distinct kicked/restricted state; ordered 1..50-user invitations with one immutable operator approval, singleton provider calls, durable per-user results, safe no-effect retries, and readback-only recovery after uncertain receipts; opaque invite handles; issuer-scoped invite-link enumeration; source-bound admin event categories/cursors; and stale-precondition rejection |
| Account and organization | profile, sessions, privacy, notifications, folders, drafts, polls and forum topics | Typed provider state plus approval-backed changes and post-write verification |
| Stories | list/by-id/archive/viewers/link, view/read/react/pin/stealth, send/edit/report/delete | Full current competitor story union, including exact media hashes and non-replaying uncertain reports |
| Statistics and boosts | `get_channel_stats`, `get_my_boosts`, `get_boost_status`, `list_boosts`, `apply_boost` | Broadcast/supergroup typed statistics and graph state, cursor-bounded boost reads, exact slot snapshots, operator approval, cooldown checks, idempotent recovery, and provider readback |
| Approval and operator bot control plane | `send_operator_bot_rich_message`, `request_send_approval`, `request_mutation_approval` and durable status readers | Bot-originated operator delivery is structurally fixed to the configured agent bot and private operator chat, uses native `sendRichMessage`, and requires provider rich-message readback; approval flows retain exact account/operator/chat/card/hash binding with restart recovery and no blind replay of non-idempotent actions; scheduled approvals fail closed when their delivery time passes and require a new exact time rather than silently sending immediately |

The continuously updated [competitor parity ledger](docs/competitor-parity.md)
records which broader Telegram families remain incomplete. Compatibility aliases
share the same typed implementation and do not count as independent parity proof.

`search_messages` and other bounded discovery tools return `completeness` as `complete`, `page`, or
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

The no-subcommand path connects stdio to the singleton daemon and starts that
daemon automatically when needed. Protocol messages use stdout; diagnostics use
stderr. `TELEGRAM_API_HASH` and
`TELEGRAM_MCP_PROXY` intentionally have no command-line flags so secrets do
not appear in process arguments.

## Recommended Text-Send Flow

With approval mode configured, use `prepare_message` followed by
`request_send_approval`:

1. Call `prepare_message` with the exact chat, text, and optional reply target.
2. Call `request_send_approval` with that `draft_id` and exact
   `payload_sha256`. A mismatch fails before the bot is contacted.
3. The dedicated bot sends one card to the configured approval chat containing
   the complete exact text, recipient, reply target, hash, expiry, and
   Send/Cancel buttons. Repeating the MCP call reuses the active approval instead
   of sending another card.
4. The bound operator clicks Send. No agent-side follow-up call is required.
   The callback worker transitions the ledger before actuation and sends through
   the personal account with the prepared draft's stable Telegram `random_id`.
5. Use `get_send_approval` to read the durable state and target `message_id`.

```text
prepare_message({ chat_id: "@alex", text: "Approved reply", reply_to_message_id: 418 })
  -> draft_id: "...", payload_sha256: "8d..."

request_send_approval({
  draft_id: "...",
  approved_payload_sha256: "8d..."
})
  -> approval_id: "...", state: "pending", approval_message_id: 123

get_send_approval({ approval_id: "..." })
  -> state: "sent", target_message_id: 456
```

The SQLite ledger preserves the approval state, Bot API update offset, Telegram
`random_id`, bounded retry schedule, and both bot/target receipts across process
restarts. A callback received twice or recovered after a crash cannot create a
second target message.

The callback poller runs inside the persistent daemon so it shares the one
exclusively locked MTProto session. Closing an individual stdio proxy does not
stop it. If the daemon itself is stopped, a still-pending Bot API callback is
handled after the daemon resumes and polls it.

`send_file` similarly requires `approved_sha256` for the exact local bytes.
It snapshots those bytes before upload so a concurrent file change cannot alter
the approved payload. File-card approval is not implemented, so approval mode
blocks `send_file` rather than falling back to weaker host-only confirmation.

## Configuration

| Environment variable | CLI option | Default / meaning |
| --- | --- | --- |
| `TELEGRAM_MCP_DEMO` | `--demo` | Deterministic local MCP fixture; no credentials or Telegram connection |
| `TELEGRAM_API_ID` | `--api-id` | Required outside demo mode: Telegram application ID |
| `TELEGRAM_API_HASH` | — (environment only) | Required outside demo mode: Telegram application secret |
| `TELEGRAM_MCP_SESSION_PATH` | `--session-path` | Exact `.session` file to use |
| `TELEGRAM_MCP_SESSION_CACHE_DIR` | `--session-cache-dir` | XDG data path described below |
| `TELEGRAM_MCP_PROXY` | — (environment only) | Optional SOCKS5 proxy URL used by both MTProto and the approval Bot API; Bot API hostname lookup is performed through the proxy |
| `TELEGRAM_MCP_ATTACHMENT_CACHE_DIR` | `--attachment-cache-dir` | XDG cache path described below |
| `TELEGRAM_MCP_MAX_DOWNLOAD_BYTES` | `--max-download-bytes` | `67108864` bytes (64 MiB) |
| `TELEGRAM_MCP_MAX_UPLOAD_BYTES` | `--max-upload-bytes` | `67108864` bytes (64 MiB) |
| `TELEGRAM_MCP_STATE_DB` | `--state-db-path` | XDG state path described below |
| `TELEGRAM_MCP_MESSAGE_CACHE_DB` | `--message-cache-db-path` | `message-cache.sqlite` beside the state database |
| `TELEGRAM_MCP_MESSAGE_CACHE_MAX_AGE_DAYS` | `--message-cache-max-age-days` | Optional whole-day UTC retention for cached message history; allowed `1..=36500`, unset keeps history until explicit clearing |
| `TELEGRAM_MCP_DAEMON_DIR` | — (environment only) | Private daemon socket/lock/log directory; resolution order described below |
| `TELEGRAM_MCP_APPROVAL_BOT_TOKEN` | — (environment only) | Dedicated approval bot token; enables button approval together with the two IDs below |
| `TELEGRAM_MCP_APPROVAL_CHAT_ID` | — | Exact configured bot chat that receives approval cards |
| `TELEGRAM_MCP_APPROVAL_USER_ID` | — | Only Telegram user ID whose callback may authorize a send |
| `TELEGRAM_MCP_APPROVAL_TTL_SECONDS` | — | Approval lifetime; default `86400`, allowed 60 seconds to 7 days |
| `TELEGRAM_MCP_APPROVAL_POLL_SECONDS` | — | Bot API long-poll timeout; default `5`, allowed 1–50 seconds |
| `TELEGRAM_MCP_APPROVAL_API_BASE` | — | Default `https://api.telegram.org`; HTTP is accepted only for a loopback test server |
| `TELEGRAM_MCP_APPROVAL_FANOUT_DB` | — | Optional absolute mode-0600 SQLite path owned by the sole Bot API update poller; reads typed callback rows instead of calling `getUpdates` |
| `RUST_LOG` | — | Optional stderr log filter |

Use a private one-to-one conversation with the approval bot from the configured
operator account before requesting the first card. The current implementation
binds the exact chat and operator IDs but does not validate the Telegram chat
type, so a group ID would expose card contents to that group even though only the
bound operator can authorize the callback. Give each token exactly one update
owner: either configure the typed fan-out database or let this process hold the
direct-poller lease. The bot token is read only from the environment and is
never written to the approval ledger or logs. Supplying only part of the
required bot/chat/user triple fails startup instead of silently disabling
approval.

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
- `telegram-personal-mcp` or `telegram-personal-mcp serve` — proxy MCP stdio to
  the persistent daemon, starting it when absent;
- `telegram-personal-mcp daemon` — run the singleton daemon in the foreground;
- `telegram-personal-mcp direct` — hidden recovery/test mode that serves one
  non-persistent MCP instance directly on stdio.

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

The persistent message cache defaults to `message-cache.sqlite` beside that
ledger. It uses SQLite WAL and is scoped by the authenticated Telegram account.
When `TELEGRAM_MCP_MESSAGE_CACHE_MAX_AGE_DAYS` is set, the daemon prunes rows
whose Telegram message timestamp is strictly older than the current UTC cutoff.
It sweeps every account already bound to the local database at daemon startup,
before Telegram/session access, then covers newly bound accounts and rechecks at
most once per 24 hours after a successful sweep. Concurrent first readers wait
for that transaction to complete.
The same cutoff is enforced inside every cache write transaction, so later
history, reconciliation, search, or live-update ingestion cannot reinsert
expired history. Affected dialog checkpoints become partial; dialogs, provider
deletion tombstones, update cursors, and the incoming-event journal are retained
independently.

The daemon directory defaults to:

1. `TELEGRAM_MCP_DAEMON_DIR` when set;
2. `$XDG_RUNTIME_DIR/telegram-personal-mcp`;
3. `$XDG_STATE_HOME/telegram-personal-mcp/daemon`;
4. otherwise `$HOME/.local/state/telegram-personal-mcp/daemon`;
5. otherwise a UID-scoped directory under the operating system temporary
   directory.

The daemon directory is mode `0700`; `daemon.sock`, `daemon.lock`, `daemon.log`,
and `daemon.identity` are mode `0600` on Unix. The identity file contains only a
format marker, PID, and SHA-256 of the effective configuration—never raw
credentials or paths. Relative path options are resolved against each process's
working directory for this comparison. Singleton ownership is acquired before
the backend, retention worker, or approval poller starts, so a losing daemon
process cannot perform those side effects.

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
database contains prepared message text, canonical chat references, immutable
source-provenance JSON, payload hashes, Telegram `random_id` values, approval
bindings, Bot API update offsets, retry state, delivery state, and receipts; it
stores neither the bot token nor session paths. Opening a legacy ledger removes the obsolete
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
- `APPROVAL_NOT_CONFIGURED`: the button tool was called without the complete
  approval bot/operator configuration;
- `APPROVAL_REQUIRED`: a direct text-send tool was called while button approval
  is active;
- `FILE_APPROVAL_NOT_IMPLEMENTED`: a file send was attempted while approval
  mode is active;
- `PAYLOAD_HASH_MISMATCH`: the approval request does not match the immutable
  prepared text payload;
- `FILE_HASH_MISMATCH`: the captured upload snapshot does not match
  `approved_sha256`;
- `CACHE_CONFLICT`: an immutable content-addressed download path already
  exists with different bytes and is preserved;
- `FLOOD_WAIT`: Telegram requires a delay; the result includes
  `retry_after_seconds`;
- `DELIVERY_UNCERTAIN`: dispatch may have reached Telegram.

The approval worker owns bounded recovery for `DELIVERY_UNCERTAIN` and always
reuses the same draft and Telegram `random_id`. Do not create a replacement
draft while an approval is `sending` or `delivery_uncertain`. In deployments
without approval mode, direct compatibility sends retain their older retry
limitations.

## Scope and Limitations

- No embedded model, autonomous agent, agent identity, or multi-agent routing.
- Approval callbacks use a durable local Bot API offset, but the Bot API is not
  treated as a general no-loss Telegram event queue.
- Interactive login supports existing Telegram accounts; signup and automatic
  `tdata` conversion remain out of scope.
- Global message search exposes only the first result page.
- Direct compatibility sends outside approval mode retain weaker uncertainty
  semantics than mutation-ledger-backed operations.
- Immediate button dispatch, passive cache ingestion, and wake-up of future
  incoming-message waits require the persistent daemon to be running; closing
  an individual stdio client does not stop it. Already-journalled events remain
  queryable while ingestion is paused.
- No single-call unbounded live-history export.
- Cached message history is retained until `clear_cache` unless the optional
  whole-day UTC age policy is configured. The incoming-event journal and its
  cursor checkpoints use separate fixed per-account row caps; message-history
  retention never silently advances an event cursor, overtaken cursors fail
  closed, and `clear_cache` removes both datasets.
- Downloads stop during streaming at the configured hard limit and are hashed,
  but are not malware-scanned.
- File upload is intentionally restricted to canonical paths inside the selected
  account's managed attachment cache.
- Telegram text and files are external, untrusted data.

See [SECURITY.md](SECURITY.md) for the trust boundaries and local-data details.

## Development

The normal release entrypoint is the shared typed controller:

```bash
deploy check
deploy
deploy status
```

It binds the exact Cargo version/tag, source commit, release notes, workflow
bytes, Git remote and GitHub Actions/readback. The workflow remains the public
publisher; direct `git push` or `gh release` commands are diagnostics/recovery
only. A release is not attempted while the current Cargo version still maps to
an existing tag at another commit.

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
