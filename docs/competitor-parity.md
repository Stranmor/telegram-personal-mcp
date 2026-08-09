# Telegram MCP competitor parity ledger

Snapshot date: **2026-07-23**

This ledger is the working release view for the requested competitor-parity
goal; the exact public tool catalogues at the pinned source snapshots below are
the authority for competitor capability. A large tool count is not sufficient:
every source-backed row must reach `complete`, its public MCP schema must be
exercised through the actual transport, and externally mutating paths must
retain hash-bound human approval and terminal Telegram readback. A Telegram API
method that no compared project exposes publicly does not become parity work
merely because it is adjacent to an implemented family.

## Primary comparison set

| Project | Snapshot | Why it is in the comparison set |
| --- | --- | --- |
| [beautyfree/mcp-telegram](https://github.com/beautyfree/mcp-telegram) | `f61b257` | Exposes 111 catalogued tools including stories, moderation, folders, privacy, stickers, boosts, raw MTProto invocation, Saved Messages sub-dialogs, and tool allow/deny/read-only controls. |
| [mcp-telegram/mcp-telegram](https://github.com/mcp-telegram/mcp-telegram) | `ee6675b` | Exposes 181 registered tools spanning messages, drafts, topics, polls, reactions, rich media, groups, folders, privacy, sessions, statistics, stories, boosts, business, update polling, group calls, Stars/gifts, and quick replies. |
| [chigwell/telegram-mcp](https://github.com/chigwell/telegram-mcp) | `700f578` | Exposes 116 decorated tools with multi-account, event waits, group, message, contact import/export, media, profile, folder, and draft workflows. The newer snapshot adds transport-host hardening without changing the public tool union. |
| [jgalea/telegram-mcp](https://github.com/jgalea/telegram-mcp) | `a6ba2d1` | Exposes 54 tools plus a passive SQLite cache/daemon, update ingestion, analytics, export, rate limiting, and bounded history/search behaviour. |

The comparison set is intentionally the union of demonstrated functionality,
not the intersection. A capability is not excluded merely because only one
competitor implements it.

## Status vocabulary

- `complete`: implemented with a typed MCP contract and representative
  transport-level E2E coverage; a live Telegram-only receipt may still be
  tracked separately until credentials are available.
- `partial`: some meaningful operations exist, but the competitor union or an
  acceptance boundary remains incomplete.
- `missing`: no eligible public implementation yet.
- `blocked`: only for a genuine user-owned or provider-owned boundary, with the
  exact blocker recorded. No row is currently classified this way.

## Capability matrix

| Capability family | Current status | Implemented now | Remaining parity work |
| --- | --- | --- | --- |
| Account health and identity | partial | health/account profile, name/about/username/birthday/presence changes, exact simple/collectible name and profile-color readback plus approval-backed set/reset, personal-channel/message readback plus approval-backed set/clear, profile-photo history/upload/delete, current emoji-status readback, hash-aware default/recent/channel-default/collectible catalogues with full collectible metadata, and approval-backed status set/clear plus exact recent-list clearing | QR login/logout |
| Authorization sessions | partial | list sessions, hash-bound single termination, and exact all-other-session reset with current-session protection, stale-set rejection, readback, and idempotent uncertain-receipt recovery | logout/QR flows and explicit local-session retirement |
| Multi-account operation | partial | deterministic session selection and ambiguity errors | explicit account inventory, browser login/logout/settings, per-call account selector, isolated same-account connection pools, and account-scoped approval routing |
| Dialog discovery and search | partial | bounded list/search/global aliases, unread aggregation, detailed chat/channel lookup and exact username resolution | pageable type/folder/archive filters, fuzzy public-directory search, richer counters, and complete inventory beyond the current bounded first page |
| Dialog state and folders | partial | mark read/unread, archive/unarchive, pin/unpin, folder CRUD/reorder/membership, full server-suggested folder filters/descriptions, and approval-backed Premium folder-tag toggling with readback | explicit dialog reorder, delete-history, all-pinned retrieval, and unpin-all parity |
| Message read/search/context | partial | bounded history/search/context, exact message/discussion/replies, topic-scoped history, unread mentions/reactions/poll votes, scheduled messages, polls, read receipts, outgoing private-message read dates, public message links, web previews, eligible discussion groups, approval-bound exact-message translation, approval-bound voice/video-note transcription with approved-only idempotent polling/rating, and typed fact-check reads | sender/date/chat-type/regex filters, topic-specific read cursors, richer media edits, and remaining competitor-specific filters |
| Saved Messages | complete | origin sub-dialog listing, origin-scoped history, text/date/origin/reaction-tag search, reaction-tag titles/counts, default tag reactions, and approval-backed range deletion, dialog pinning, and tag-title changes with exact prior-state binding, bounded terminal readback, and non-replaying uncertain-outcome recovery | — |
| Text sending | partial | durable private-button flow, approval-bound typing/photo-upload/document-upload/cancel indicators, forwarding, scheduled send/edit/delete/send-now, replies, silent/no-forward controls where Telegram exposes them | Markdown/HTML entities, explicit topic binding, send-as/effects, inline keyboards, and link-preview controls |
| File and rich-media sending | partial | immutable SHA-256 photo/document/GIF upload, opaque-handle sticker sends, live hash-aware dice capability discovery, approval-backed location/venue/contact/dice delivery, OGG/Opus voice, MP4 video notes, and mixed photo/video albums through the same idempotent ledger; coordinates are e7-normalized, GIF mode validates actual GIF87a/GIF89a bytes, and every kind has typed terminal readback | richer media attributes and provider thumbnail controls |
| Message mutation | partial | edit/forward/pin/reaction/draft/read-state and fact-check mutations with exact snapshots, private approval, and provider readback; deletion captures recoverable HTML/media/thread/group state before approval and fails closed for unsupported content unless irreversibility is explicit; restoration recreates exact selected content under new message IDs with deterministic idempotency, album preservation, sender-change opt-in, and old-to-new receipts; exact/recent message-button inventory and approval-backed callback actuation use live markup revalidation, callback fingerprint binding, and non-replaying uncertain outcomes | media edits and paid reactions |
| Attachments and downloads | partial | `list_attachments`, `download_attachment`, provider-resolved custom-emoji lookup/download with streaming limits and immutable content-addressed cache | bulk chat-media download, media-group/album preservation, arbitrary peer/profile media, thumbnails, and resume/range controls exposed at MCP layer |
| Contacts | partial | list/search/common chats and exact add/edit/delete contact mutation | pending contact requests, bulk contact import, and phone-only import-send-cleanup workflows |
| Blocking and moderation | partial | block/unblock, add/remove/ban/restrict/promote members, durable 1..50-user batch invitations, join-request processing, typed channel/supergroup admin-log history, exact default member-rights merge, exact allowed-reaction policy, and native supergroup anti-spam read/write with approval, stale-state checks, provider readback, and no-replay uncertain recovery | report spam, delete-user-history, and ownership transfer |
| Participants and administrators | partial | exact single-member/details/admins/online count, authenticated-account role lookup, pageable detailed participant inventory with `recent`/`admins`/`kicked`/`banned`/`bots`/`contacts` filters, name/username/phone/id search, exact totals and next offsets, distinct kicked-versus-restricted state, exact membership/admin/restriction mutations, durable ordered 1..50-user invitations with one immutable approval and per-user outcomes/retry/readback state, and typed/filterable admin history | ownership transfer |
| Group/channel lifecycle | partial | create/join/leave/delete, title/about/photo/username, core channel toggles, and exact forum-mode/forum-tabs read/write with explicit disable confirmation and uncertain-outcome recovery | basic-group migration, linked-discussion configuration, forum-at-create, and ownership transfer |
| Invite links and join requests | complete | opaque export/list/get/revoke/delete links, issuer-scoped enumeration by administrator username or canonical user id with authenticated-account default, issuer-bound handle roundtrip, joiner pages, and approve/hide one/all requests | — |
| Forum topics | partial | list/get/create/edit/pin/delete with exact snapshots, topic-scoped history, and exact forum-mode/forum-tabs control | topic-scoped send and topic-specific mark-read parity |
| Polls and quizzes | complete | create/vote/results/voter pages/close with exact option-byte bindings | — |
| Reactions | partial | list/add/remove, mark reactions read, account default read/update with approval and live readback, plus hash-aware top/recent catalogues and Saved Messages tag/default catalogues | paid-reaction spend/privacy surfaces |
| Stickers and custom emoji | complete | installed/recent/favourite/search catalogues with not-modified hashes, full set contents, opaque sticker handles, approval-backed exactly-once sticker sends, favourite/install/uninstall mutations, and add/remove mutation for separate ordinary and attached recent lists with exact hash-zero observations and non-replaying uncertain-receipt recovery; custom-emoji lookup and bounded immutable downloads require no caller-supplied provider credentials, and explicit GIF delivery is included | — |
| Privacy | complete | read and exact full-rule replacement for every current Telegram privacy key, plus account-wide global privacy read/partial-write for all current TL fields (five legacy flags, gifts button, paid-message Stars threshold, and disallowed-gift controls) with exact set/clear, stale-state rejection, and uncertain-receipt recovery | — |
| Notifications and chat settings | partial | read/update notifications; exact private/group/channel auto-delete TTL readback and approval-backed set/disable with stale/uncertain-state protection; slow mode, signatures, pre-history and join-request toggles; exact default rights, allowed reactions, anti-spam, forum mode, and forum-tabs read/write | themes and default send-as |
| Drafts | partial | list/save/clear per-dialog drafts plus approval-backed global clear; every draft includes a complete TL-state SHA-256, and account-wide deletion binds the exact canonical set with stale/uncertain-state protection | rich draft authoring beyond plain text; the no-webpage flag is preserved on reads but is not yet writable |
| Stories | complete | all/peer/by-id/archive/viewers/link, view/read/react/pin/top-pin/stealth, SHA-bound send/edit, multi-step report and delete | — |
| Statistics and boosts | complete | typed channel/supergroup statistics with date ranges, current/previous counters, graph state/data, top posters/admins/inviters, recent interactions, plus boost slots/status/list pagination and approval-backed exact-slot application with cooldown checks, immutable prior-state binding, idempotent uncertain-outcome recovery, and provider readback | — |
| Bot and inline interactions | complete | `get_user_info` is a typed superset of the competitor bot-info read; exact inline/reply keyboard inventory and approval-backed non-2FA callback answers; typed inline-bot query pages preserve bot/context identity, empty queries, offsets, gallery/cache/switch metadata, normal/media result details and send previews including exact E7 geo/live-location controls; result sends require a still-live account-scoped cached query/result, bind exact target/reply/flags, a fully represented draft precondition for `clearDraft`, and stable random ID, cap approval TTL to query expiry, never open URLs/WebViews, and recover uncertain receipts without duplicate delivery; authenticated-bot default-scope commands support typed readback plus approval-backed exact replacement/reset with stale-state rejection and idempotent recovery | — |
| Business and quick replies | complete | hash-aware quick-reply shortcut/message reads; Business chat-link list/resolve/create/edit/delete; and exact work-hours, location, greeting, away-message, and intro writes with full account-profile readback. Work hours use canonical minute-of-week ranges and server-derived `open_now`; recipients, supported inactivity days, schedules, E7 location, and opaque intro sticker handles are typed. Every write is private-approval bound and uses exact before/target/readback recovery without duplicate calls. The pinned competitor snapshots expose no Business-connection tools or quick-reply mutations, so adjacent Telegram methods are not treated as parity requirements | — |
| Calls and group calls | partial | exact active/discarded group-call metadata plus bounded initial and paginated participant reads with peer/source filters, full current flags, video/presentation source groups, raise-hand and paid-Stars state, related chats/users, and no caller-visible access hash | call capability/status, group-call lifecycle, join/leave, recording/stream controls |
| Payments, Stars, gifts | partial | exact Stars wallet status and filtered transaction history; available and saved regular/unique Star Gifts with current metadata and filters; top-up tier reads; paginated subscription reads; approval-backed show/hide and subscription cancel/restore with exact snapshots and idempotent uncertain-receipt recovery; irreversible gift conversion with exact value/balance binding and mandatory no-replay uncertainty handling; legacy `excludeLimited` compatibility without cursor loss; raw bot payloads, invite/prepaid hashes, addresses, and access hashes are omitted or reduced to safe typed metadata | invoices, paid media, pre-checkout/shipping, and other payment flows; every spend or irreversible value path requires an explicit paid-resource boundary |
| Profile and account settings | partial | profile text/photo/username/birthday/presence, full account emoji-status catalogue/read/set/clear workflows, exact simple/collectible name/profile colors, and personal-channel/message readback with approval-backed set/reset | global account settings |
| Raw MTProto escape hatch | missing | — | schema-versioned allowlisted raw invocation with typed request/response validation; never accept arbitrary untrusted action payloads |
| Realtime updates and local cache | complete | exact `updates.getState`, bounded global `updates.getDifference`, and channel/supergroup `updates.getChannelDifference` polling; one singleton daemon owns passive ingestion, bounded 50-dialog/200-message reconciliation, generation/global-cursor/per-chat-revision CAS commits, tombstone-wins new/edit/delete application, explicit `warming`/`stale`/`resync_required`/partial/complete coverage, fail-closed `too_long` and restart recovery, persistent account-scoped SQLite WAL storage, confirmed clear with a fresh successor cursor, passive live-read write-through, regex search, analytics, timelines/today views, bounded JSON/CSV export, and optional continuously enforced whole-day UTC message retention. `wait_for_new_message` and per-chat `wait_for_settled_message` use a separately bounded restart-safe private-human event journal, independent immutable opaque cursors, deterministic replay, mixed-mode/interleaved-chat preservation, exact account/checkpoint revalidation, typed retention tombstones, and prompt clear/resync invalidation. LIKE is covered semantically by bounded cache query/export rather than duplicated as a second public search name | — |
| Transport and connection runtime | partial | byte-transparent stdio proxy to a singleton per-user Unix daemon, automatic bounded startup, shared long-lived Telegram/cache/approval workers, pre-side-effect singleton lease, owner-only socket/lock/log/config-identity paths, fail-closed effective-config matching (including cwd-bound relative paths), stale-socket validation, graceful signal shutdown, and fail-closed local session selection | shared long-lived HTTP/SSE MCP host, app-owned network ping/reconnect semantics, stable configurable Telegram device identity, and broader per-account proxy routing |
| Tool gating and deployment policy | partial | read/write annotations and approval-mode fail-closed sends | allow/deny lists, global read-only mode, per-capability policy, proactive shared sliding-window rate limits, and account/tool quotas |
| Reliability and recovery | partial | durable prepared sends and generic mutation ledger, exact approval/card/account binding, callback offset, stable send/file idempotency keys, no-expiry-after-acceptance typestate, bounded uncertainty recovery, session ambiguity protection, atomic cache commits with generation, exact global cursor, and per-chat mutation-revision CAS; deletion tombstones prevent stale history resurrection; daemon singleton/lifecycle proof, restart-persistent cache/event readback, independent event cursors, mixed-mode frontier compaction, typed cursor invalidation, and post-registration database rechecks that close notification races | remaining mutation-family receipts and live multi-account stress proof |

## Current MCP surface

The deterministic fixture and transport tests currently exercise **304** public
tools in the authoritative in-process catalog. `tests/mcp_integration.rs`
asserts the exact count, closed input schemas, typed output schemas, and
representative canonical names. The compiled stdio server deliberately exposes
23 common first-class entries in `tools/list`; `tests/stdio_demo.rs` proves that
compact direct surface over a child process and reaches the remaining catalog
through the typed discovery/gateway tools. The 304-tool catalog is therefore
gateway-reachable, while only 17 tools are first-class in the direct stdio
inventory.

The current surface includes complete core families for story operations,
Saved Messages, polls, invite links/join requests, scheduled messages,
profile-photo handling, folder CRUD, and the shared durable approval ledger. Compatibility aliases are
kept discoverable only when they preserve the same typed implementation and
safety contract; they do not count as separate parity evidence.

## Completion contract

The parity goal is complete only when:

1. every matrix row is `complete` or has an explicit user-owned/provider-owned
   blocker;
2. every public tool has closed input schemas, typed output schemas, bounded
   pagination/size semantics, and stable typed errors;
3. every mutation is immutable-hash-bound to an exact account, target, payload,
   mode, and expiry before the human approval actuator can consume it;
4. uncertain non-idempotent outcomes never trigger blind duplicate actions;
5. unit, backend, official in-process MCP, and real child-process stdio tests
   cover representative success, validation, denial, crash/restart, and
   uncertainty paths;
6. live Telegram E2E proves account identity and terminal readback for every
   mutation family that can be exercised without crossing a user-owned paid,
   public, credential, or destructive boundary;
7. documentation, tool counts, examples, and security claims match the exact
   shipped surface;
8. every `partial` or `missing` item remains traceable to a public tool or
   durable runtime capability in one of the pinned competitor snapshots; a
   derived ledger entry without current source evidence cannot block release.
