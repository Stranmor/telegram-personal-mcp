# Session ownership and recovery

An active MCP transport is not proof of Telegram authorization. Verify the
registered `health` tool, its positive account identity, and a bounded read.
`AUTH_KEY_UNREGISTERED` requires a fresh account login; repeated connections
cannot renew the authorization.

The session storage backend owns the exclusive lease for the complete lifetime
of every client and blocking writer. Cancelling a connection signals its workers
to stop; their storage references keep the lease until outstanding work ends.
All partial session updates serialize with full saves. Existing unreadable or
corrupt session files are errors, never implicit permission to replace them.

Failed or cancelled initialization cancels its background workers and closes
the transport. Startup key recovery updates only the home DC entry. Full
snapshot writers start after restoration, and request-only clients retain saved
update counters without running the periodic update-counter writer.

If a failed backend repeatedly reconnects, contain that exact backend before
repairing it. Preserve the registered stdio transport, session, approval ledgers
and active clients. Install a verified immutable backend generation, then switch
the future-launch reference and replace only the failed backend. Never delete
an active lock or restart the Codex-bound transport to change its backend.

Use `auth login` for the exact configured session in a local interactive
terminal. Load application credentials from the existing configured source;
never put login codes, passwords or API secrets in command arguments or logs.
Completion requires account identity and real reads through the original MCP.

Regression coverage lives in `tests/connection_cleanup.rs` and
`src/session_owner.rs`: rejected/cancelled connections, lease retention,
concurrent partial updates and preservation of corrupt session files.
