# Changelog

All notable changes to Telegram Personal MCP are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and releases use [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Added `prepare_schedule_file_message`, an immutable approval-backed scheduled
  photo/media mutation with exact artifact hashes, source/target provenance,
  caption parse mode, schedule-time expiry, non-replaying uncertain recovery,
  and provider scheduled-message plus attachment readback.
- Scheduled-message approvals now expire at the scheduled delivery time even
  when their generic approval TTL is longer. The worker proactively closes
  legacy pending cards, callback and provider paths fail closed after the
  deadline, and the terminal card states that nothing was sent and a new exact
  time requires a new approval.
- Added mandatory typed source provenance for prepared plain-text, Rich Text,
  and scheduled messages. Approval cards now always show an exact Telegram
  message, a validated public-web source with date and relation, or an explicit
  self-originated state. Provenance is hash-bound and persisted; legacy rows
  remain readable and are visibly labelled when they lack source metadata.
- Added an exact compatible-daemon executable binding for frontend-only
  rollouts. A newer MCP transport can reuse or respawn one byte-identified
  daemon generation without opening the same authenticated Telegram session in
  a second process or weakening the daemon configuration identity check.
- Added recoverable Telegram message deletion and restoration. Deletion now
  captures canonical HTML, thread/group bindings, flags, and immutable media
  bytes before approval; unsupported content fails closed unless
  `allow_irrecoverable=true` is explicitly approved. The new
  `prepare_restore_deleted_messages` mutation recreates exact selected content
  with new Telegram IDs, deterministic idempotency keys, album preservation,
  provider readback, original-to-restored ID receipts, and explicit opt-in when
  the authenticated account would replace a different original sender.
- Expanded the typed MCP surface to 306 tools, covering story send/edit/report,
  polls, scheduled messages, member and chat lifecycle operations, profile and
  folder management, privacy/notifications, unread discovery, message links,
  webpage previews, eligible discussion-group discovery, unread
  mentions/reactions/poll votes, topic-scoped history, outgoing-message read
  dates, authenticated-account role lookup, recent/searchable sticker
  catalogues, full sticker-set contents, approval-backed sticker sends,
  favourites, sticker-set install/uninstall lifecycle, custom-emoji document
  lookup, bounded immutable custom-emoji downloads, magic-validated GIF sends,
  live dice capability discovery, and approval-backed location, venue, contact,
  dice, voice, video-note, and album delivery through the shared mutation ledger,
  channel/supergroup statistics, Premium boost readback, and approval-backed
  exact-slot boost assignment with cooldown validation and idempotent recovery,
  bounded/filterable channel and supergroup admin-log history, approval-bound
  message translation and initial voice/video-note transcription, safe
  transcription polling/rating, and typed fact-check read/edit/delete with
  exact readback, plus origin-scoped Saved Messages dialogs, history/search,
  reaction-tag catalogues/defaults, and approval-backed history deletion,
  dialog pinning, and reaction-tag title changes with non-replaying uncertain
  outcome recovery, plus approval-bound typing/photo-upload/document-upload/
  cancel indicators whose uncertain provider calls are never replayed, plus
  full typed message-button inventories and approval-backed callback presses.
  Callback selection supports exact row/column, flat index, case-insensitive
  exact label, and base64 data matching; raw callback bytes never enter the
  mutation ledger, non-callback/2FA buttons are rejected, and uncertain presses
  are never replayed.
  Added typed inline-bot result queries and approval-backed result sends with
  empty-query/offset pagination, bot/context identity, short-lived account-bound
  query caches, exact result/reply/flag snapshots, E7 geo/live-location
  previews, approval expiry capped to the query lifetime, fully represented
  draft preconditions for `clearDraft`, stable `random_id` recovery, and no
  automatic URL/WebView opening.
  Added authenticated-bot command read/replace/reset parity for the default
  scope, including exact bot identity, language and ephemeral-command binding,
  private approval, stale-state rejection, provider readback, and idempotent
  uncertain-receipt recovery.
  Added account emoji-status parity with hash-aware default, recent,
  channel-default and collectible catalogues; full collectible metadata;
  current-status readback; approval-backed set/clear; expiry-capped approvals;
  exact recent-list snapshots; stale-state rejection; and idempotent
  uncertain-receipt recovery.
  Added exact all-other-session termination parity using
  `auth.resetAuthorizations`, with one-current-session validation, full
  non-current device snapshots, private approval, changed/new-session
  precondition rejection, provider readback, and idempotent uncertain-receipt
  recovery.
  Added peer-scoped chat auto-delete readback and approval-backed set/disable
  parity for private chats, groups, and channels, including competitor
  `chatId`/`period` aliases, exact canonical peer identities, stale-state
  rejection, and non-overwriting uncertain-receipt recovery.
  Added account-wide draft clearing through `messages.clearAllDrafts`, bound to
  a complete canonical draft set. Draft reads now carry a SHA-256 of the full TL
  object so reply, entity, media, effect, suggested-post, and rich-message state
  cannot change invisibly; uncertain recovery never deletes a later draft.
  Added account name/profile color and personal-channel parity through exact
  `account.updateColor`/`account.updatePersonalChannel` requests. Full-profile
  readback preserves simple and collectible color variants plus the featured
  channel/message identity; stale and uncertain recovery never overwrites a
  later third state.
  Added suggested-folder and Premium folder-tag parity through
  `messages.getSuggestedDialogFilters`/`messages.toggleDialogFilterTags`,
  preserving each suggestion's server description and complete typed filter.
  Added account-wide global privacy read/partial-write parity. The read model
  covers every field in the current TL constructor—not only the competitor's
  original five flags—including the gifts button, paid-message Stars threshold,
  and all disallowed-gift controls; omitted fields survive exact merge, while
  set/clear, stale-state, and uncertain-receipt paths are approval/readback bound.
  Added hash-aware quick-reply shortcut and stored-message reads through exact
  `messages.getQuickReplies`/`messages.getQuickReplyMessages` contracts. The
  typed output preserves complete top-message summaries, shortcut IDs,
  regular/service distinction, service action kind, reply metadata, and related
  chat/user context; optional ID filters are unique and bounded, with competitor
  `shortcutId`, `ids`, and `messageIds` aliases.
  Added exact active group-call and participant reads through
  `phone.getGroupCall`/`phone.getGroupParticipants`, including metadata-only or
  bounded initial pages, opaque-offset pagination, peer/source filters, current
  participant flags, video/presentation source groups, raise-hand and paid-Stars
  state, and related chat/user context. Public results deliberately omit raw
  Telegram call access hashes and expose only re-resolvable call/chat identity.
  Expanded `list_participants` into a detailed, backward-compatible member
  inventory with exact `recent`, `admins`, `kicked`, `banned`, `bots`, and
  `contacts` filters; name/username/phone/id search; integer pagination; exact
  totals; full role, actor, rank, admin-rights, restriction-rights, and join
  metadata; and explicit kicked-versus-restricted state. Channel reads bind the
  current `channels.getParticipants` constructors, while basic groups are
  filtered from their complete participant vector; incomplete provider joins
  fail closed instead of silently shrinking a page.
  Added durable batch invitations for 1 to 50 users through
  `prepare_invite_chat_members`, `invite_chat_members`, and `invite_to_group`.
  One immutable private approval binds the exact ordered user set, while each
  target has its own SQLite typestate, provider-call count, retry schedule,
  readback receipt, and terminal outcome. Telegram is invoked once per target;
  only explicitly no-effect responses may retry, stale `issuing` rows become
  readback-only `outcome_unknown`, and uncertain calls are never replayed.
  Added six read-only Stars and Star Gift tools through exact current
  `payments.getStarsStatus`, `payments.getStarsTransactions`,
  `payments.getStarGifts`, `payments.getSavedStarGifts`,
  `payments.getStarsTopupOptions`, and `payments.getStarsSubscriptions`
  contracts. The typed surface preserves current transaction, regular/unique
  gift, saved-gift, pricing, subscription, pagination, and related-peer state;
  the competitor's removed `excludeLimited` filter is emulated without cursor
  loss, while bot payloads, invite/prepaid hashes, addresses, and provider
  access hashes are never returned raw.
  Added approval-backed Star Gift show/hide, irreversible conversion, and Stars
  subscription cancel/restore through exact current TL methods. Each mutation
  binds the canonical wallet and exact prior object; show/hide and subscription
  changes recover uncertain receipts by readback without duplicate provider
  calls, while conversion binds its value and wallet balance and permanently
  disables automatic replay after any uncertain provider outcome.
  Added Telegram Business chat-link list/resolve/create/edit/delete through the
  exact current `account.*BusinessChatLink*` contracts. Link payloads bind the
  source text, parse mode, rendered message, exact serialized entity-vector
  SHA-256, title and provider slug; mutations use private approval and account
  readback, create permanently disables replay after an uncertain receipt, and
  edit/delete recover without duplicate provider calls. Returned links remain
  untrusted data and are never opened automatically.
  Added Telegram Business work hours, location, greeting, away-message, and
  intro settings through the exact current `account.updateBusiness*` contracts.
  Account-profile readback now includes all five settings; work intervals are
  canonical minute-of-week ranges, `open_now` remains server-derived, addresses
  obey Telegram's 96-character limit, greeting inactivity uses the supported
  7/14/21/28-day values, and intro stickers accept only verified set names plus
  opaque MCP handles. All writes use immutable private approval and exact
  before/target/readback recovery without duplicate provider calls.
  Added exact stateless update-cursor parity through `updates.getState`,
  `updates.getDifference`, and `updates.getChannelDifference`. Typed responses
  distinguish empty, complete, sliced, and too-long states; bind the request and
  next/resync cursor; preserve grouped global/channel deletion metadata, safe
  update kinds, related peers, encrypted-message omission counts, and channel
  snapshots; and enforce current user-account channel limits without exposing
  raw provider payloads or access hashes.
  Added exact chat-moderation policy parity through current
  `messages.editChatDefaultBannedRights`,
  `messages.setChatAvailableReactions`, `channels.toggleAntiSpam`, and
  `channels.toggleForum` contracts. Reads preserve default banned rights,
  available-reaction policy and limits, anti-spam, forum mode, and forum tabs;
  writes merge explicit allowed-capability patches, validate selected reactions,
  require confirmation before forum removal, and use private approval, exact
  stale-state checks, provider readback, and idempotent uncertain-receipt
  recovery without duplicate calls.
  Quota-bearing translation disables automatic replay when its provider receipt
  is lost.
- Added a shared durable mutation approval ledger with immutable payload hashes,
  exact prior-state snapshots, provider readback, and fail-closed recovery for
  non-idempotent actions.
- Added durable private Telegram button approval for prepared text sends through
  `request_send_approval` and `get_send_approval`, with exact operator/chat/card
  binding, expiry, cancellation, persistent Bot API offsets, and terminal
  receipts.
- Added a single callback worker lease, bounded delivery-uncertain recovery, and
  restart tests proving that a Telegram-accepted send reuses the same
  `random_id` instead of creating a duplicate.
- Added approval-backed add/remove mutation for ordinary and attached recent
  sticker catalogues. The mutation accepts only opaque set-bound handles,
  rejects custom emoji and incomplete hash-zero observations, and proves an
  uncertain receipt through terminal catalogue readback without replay.
- Extended invite-link enumeration and opaque-handle lifecycle operations with
  an optional issuing-administrator username or canonical user id; omission
  continues to mean the authenticated account, and raw invite tokens remain
  excluded from MCP and approval surfaces.
- Added an account-scoped persistent SQLite WAL message cache with passive
  write-through, bounded reconciliation, atomic global update cursors, typed
  edit/delete ingestion, restart and `too_long` recovery, explicit coverage,
  regex search, analytics, timelines, today views, bounded JSON/CSV export, and
  confirmed account-only clearing with a fresh successor cursor.
- Added `wait_for_new_message` and `wait_for_settled_message` on the daemon-owned
  update stream. Eligible private-human incoming messages enter a bounded,
  restart-safe journal; opaque immutable cursors support independent at-least-once
  replay, per-chat debounce preserves interleaved chats, lost wake-ups are closed
  by a post-registration SQLite recheck, and expired retention cursors fail
  closed instead of skipping events. Mixed per-chat/global advancement preserves
  every prior frontier; advancement revalidates exact account/checkpoint identity;
  bounded invalidation tombstones distinguish stale cursors from fabricated
  input; cache clear and Telegram resync wake active waits; and a `too_long` gap
  invalidates old cursors instead of returning a misleading clean timeout.
- Hardened every asynchronous message-cache writer with account-generation and
  exact global-cursor or per-chat-revision compare-and-swap checks. Provider
  deletion tombstones outrank later history/search replies, and global delete
  updates remove every matching cached row. A completed `clear_cache` now
  rejects older in-flight update differences, reconciliation/history fetches,
  exports, and passive write-through; same-generation stale reads cannot
  overwrite a live edit or restore a deleted message. Resync invalidation uses
  its actual commit-time clock, so newly stale event cursors are not evicted
  ahead of older tombstones under retention pressure.
- Added optional `TELEGRAM_MCP_MESSAGE_CACHE_MAX_AGE_DAYS` /
  `--message-cache-max-age-days` retention (`1..=36500`). Existing rows are
  pruned for every locally bound account before Telegram/session startup and at
  most daily thereafter; concurrent first readers wait for the committed sweep.
  Every cache write transaction enforces the same strict UTC cutoff so expired
  history cannot be reintroduced. Dialog checkpoints become partial after
  pruning; deletion tombstones, update cursors, dialogs, and incoming-event
  state remain independent. Child-process daemon E2E proves all-account,
  concurrent, retained/expired, and offline-start behavior.
- Added a singleton per-user Unix daemon and byte-transparent stdio proxy. The
  first client starts the daemon, later clients reuse the same Telegram session
  and background workers only when a secret-free digest of the full effective
  configuration matches. Relative paths are bound to their actual starting
  working directory; mismatches fail closed. The singleton lease is acquired
  before backend/worker side effects, and owner-only socket/lock/log/identity
  paths are removed or retained according to inode-safe lifecycle checks.

### Changed

- Replaced the terminal-style public demo with a proof-bound safe-send
  animation. Its renderer now runs an MCP integration harness first and refuses
  to publish unless direct bypass is blocked, the exact private approval card is
  emitted once, the bound operator reaches `sent`, provider readback confirms
  the target message, and a duplicate callback still produces one delivery.
- Approval-enabled deployments reject direct compatibility writes and route
  supported message, file, story, chat, member, profile, privacy, folder, poll,
  schedule, and notification mutations through exact private approval cards.
- The MCP tool surface now contains 306 typed tools. The compact compiled stdio
  inventory still exposes 17 common first-class entries and reaches the full
  catalog through typed discovery/gateway tools.

## [0.2.0] - 2026-07-19

### Added

- Added a live Codex-driven MCP demo as an asciinema cast, GIF, and H.264 MP4,
  plus a reproducible renderer that isolates the deterministic `--demo` server
  from every other configured MCP server.

### Changed

- Expanded the MCP server instructions so host agents read context before file
  actions, preserve immutable downloads, hash exact final bytes, bind
  `approved_sha256`, and verify replies after delivery.
- Release publication now retries GitHub's bounded draft readback window before
  failing closed, covering observed eventual consistency after draft creation.
- Updated the locked `clap` and `tokio` patch releases while keeping the
  compatibility-sensitive Ferogram and MCP SDK versions pinned.

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

[Unreleased]: https://github.com/Stranmor/telegram-personal-mcp/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/Stranmor/telegram-personal-mcp/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Stranmor/telegram-personal-mcp/releases/tag/v0.1.0
