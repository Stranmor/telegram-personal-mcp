# Security Policy

## Supported Versions

Security fixes target the latest `main` branch until the first stable release is
cut. After that, the latest minor release receives fixes.

## Credential Handling

Treat all of these as secrets:

- Telegram API hash.
- Telegram Desktop `tdata`.
- Generated `.session` files.
- Proxy credentials embedded in a proxy URL.

Do not commit them, print them in logs, or paste them into issue reports.

## Reporting a Vulnerability

Open a private security advisory on GitHub if the repository is already public.
If advisories are unavailable, contact the maintainer privately and include:

- affected version or commit;
- exact tool/action involved;
- whether a message could be sent, deleted, or read beyond configured bounds;
- whether credentials, sessions, or proxy secrets may have been exposed.

Do not include live session files, `tdata`, API hashes, or message dumps.

## Design Boundaries

- No deletion MCP tool.
- No unbounded history export.
- No automatic Telegram Desktop `tdata` conversion.
- `send_message` requires explicit approval evidence in the tool arguments.
- `health` does not read dialogs or messages.
- Session files are locked so concurrent MCP processes do not open the same
  MTProto auth key.
