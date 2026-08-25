# Matrix — Shared Structures

## 1. Overview

Rust module (`crate-rockbot/src/platform/matrix.rs`) wrapping
[`matrix-rust-sdk`](https://github.com/matrix-org/matrix-rust-sdk) to provide
a Matrix messaging client that implements the `MessagingClient` trait. The
Matrix platform uses the SDK's high-level `Client` API to authenticate with a
homeserver, sync room events via long-polling `/sync`, filter incoming messages,
and send replies.

**Feature status (2026-07-10):** `matrix-sdk` is compiled with `default-features = false`
and `features = ["markdown", "bundled-sqlite"]`. The `bundled-sqlite` feature enables
persistent state storage (access token, device ID, sync token, room state) in a
SQLite database at `state_dir` from config, with a vendored libsqlite3 (no system
dependency required). The `e2e-encryption` feature is **not** enabled. The bot
cannot decrypt `m.room.encrypted` events — they are silently dropped because no
handler is registered. When a client (e.g. Element) creates an encrypted DM, the
bot will not see any messages. Two paths to resolve:
1. Enable `e2e-encryption` feature + crypto store setup (see `level-2/authentication.md` notes).
2. Users must create **unencrypted** DMs (disable encryption toggle in their client
   before sending the first message) and manually accept the room invite (bot does
   not auto-join — see `level-2/message-filter.md` notes).

With E2EE enabled, the SDK's built-in crypto store would handle Olm/Megolm key
exchange and message decryption transparently.

Messages from joined rooms are parsed into the shared `IncomingMessage` type
(defined in `crate-rocketchat/src/types.rs` — reused as the cross-platform
message contract). The agent harness and tools are unaware of the underlying
platform.

- Upstream: [Configuration Management](../config/main-path.md) provides `MatrixServerConfig`
  (homeserver URL, user_id, password, device_id, state_dir)
- Upstream: [Agent Loop](../../agent/agent-loop/main-path.md) calls `connect_and_run()` with a
  message handler callback
- Downstream: [Agent Harness](../../agent/agent-harness/agent-loop.md) receives filtered
  `IncomingMessage` structs; sends replies through `MessagingClient::send_reply()`

## 3. Data Structures

#### `MatrixPlatform`

| Field            | Type                    | Purpose                                     |
| ---------------- | ----------------------- | ------------------------------------------- |
| `homeserver`     | `String`                | Homeserver URL (e.g. `"https://matrix.org"`)|
| `user_id`        | `String`                | Bot's Matrix user ID for login              |
| `password`       | `String`                | Account password                            |
| `device_id`      | `Option<String>`        | Device ID for session management            |
| `state_dir`      | `String`                | SQLite state store path (default `"./tmp/matrix-sdk"`) |
| `force_relogin`  | `AtomicBool`            | Set to `true` on `M_UNKNOWN_TOKEN`; forces fresh login on next `connect_and_run()` regardless of stored session |

The `matrix_sdk::Client` is created inside `connect_and_run()`, not stored in
the struct. The authenticated user ID is extracted from `client.user_id()`
after login and captured by the event handler closure. If `client.user_id()`
returns `None`, the connection returns `AuthFailed`.

`MatrixPlatform` implements `MessagingClient::bot_id()` by returning
`&self.user_id` (the configured `@bot:server` MXID). `main.rs` calls this at
boot to obtain the `bot_id` value passed to `AgentHarness::new()` (issue #58).
Non-emptiness is guaranteed by `MatrixServerConfig.user_id` validation
(`#[validate(min_length = 1)]`).

#### `MatrixSender` (implements `PlatformSender`)

Per-message platform handle created in the event handler closure. Stores the
`matrix_sdk::Room` for reply sending and the bot's `user_id` for mention
prefix stripping.

| Field      | Type              | Purpose                                              |
| ---------- | ----------------- | ---------------------------------------------------- |
| `room`     | `matrix_sdk::Room`| Room object for `send()`, `typing_notice()`          |
| `room_id`  | `String`          | Room ID string (e.g. `!abc:example.org`)             |
| `user_id`  | `String`          | Bot's full MXID (e.g. `@bot:example.org`) — used by `strip_mention_prefix` to strip `@bot:server` or `@bot` localpart from non-DM message text |

#### Matrix → `IncomingMessage` Field Mapping

| `IncomingMessage` field | Matrix source                                          |
| ----------------------- | ------------------------------------------------------ |
| `msg_id`                | `event.event_id` (e.g. `$abc123`)                      |
| `room_id`               | `room.room_id` (e.g. `!abc:example.org`)               |
| `room_name`             | Canonical alias localpart or room ID localpart          |
| `room_fname`            | Room display name (`m.room.name`)                      |
| `sender_name`           | `event.sender` localpart (e.g. `@alice` from `@alice:example.org`) |
| `text`                  | `event.content.body` (raw plain text body — may contain `@bot` mention prefix; stripped by `MatrixSender::strip_mention_prefix` in the agent loop) |
| `is_dm`                 | Room joined member count ≤ 2                           |
| `timestamp`             | `event.origin_server_ts` (milliseconds → seconds)      |
| `sender_id`             | `event.sender` (full MXID, e.g. `@alice:example.org`)  |
| `alias`                 | `None` (Matrix has no per-message alias)               |
| `file`                  | `None` (image data travels via `attachments`)           |
| `files`                 | Empty (Matrix has no file list metadata)                |
| `attachments`           | Populated from `m.image` events with `data:` URI in `title_link` |
| `urls`                  | Extracted from message body URLs (no server-side preview headers) |

#### `MatrixServerConfig`

| Field          | Type             | Notes                                           |
| -------------- | ---------------- | ----------------------------------------------- |
| `homeserver`   | `String`         | Homeserver URL (e.g. `"https://matrix.org"`)    |
| `user_id`      | `String`         | Bot user ID (`@bot:example.org`)                |
| `password`     | `String`         | Account password                                |
| `device_id`    | `Option<String>` | Device ID for session management                |
| `state_dir`    | `String`         | SDK state store path (default `"./tmp/matrix-sdk"`) |

## 4. Non-Functional Requirements

- **SDK state on local disk**: Unlike the "no local files" rule for tools and
  memory, the matrix-rust-sdk requires a local state directory for its SQLite
  stores (sync token, room state, access token). This is configured via
  `state_dir` (default `./tmp/matrix-sdk`) and is considered infrastructure
  state, not bot data. The `sqlite` feature must be enabled in `Cargo.toml`.
- **E2EE transparency** (spec target): When the `e2e-encryption` feature is enabled,
  end-to-end encryption is handled entirely by the SDK. The bot sees decrypted plain
  text in event handlers. No manual key management is required. Currently the feature
  is **not** enabled — see Section 1 note.
- **Sync state recovery**: On reconnect, the SDK resumes sync from the last stored
  `since` token in the SQLite store, avoiding re-processing old messages. If
  `force_relogin` is set (due to `M_UNKNOWN_TOKEN`), a fresh login creates a new
  session and the first sync is a full initial sync.
- **No alias support**: Matrix does not support per-message sender name
  override. The `alias` parameter is accepted by `send_reply()` but silently
  ignored.

## 5. Dependencies

| Crate            | Version | Purpose                                         |
| ---------------- | ------- | ----------------------------------------------- |
| `matrix-sdk`     | `0.18`  | High-level Matrix client (sync, rooms, media); built with `default-features = false`, `features = ["markdown", "bundled-sqlite"]` |
| `matrix-sdk-base`| (transitive) | Core types (`OwnedUserId`, `OwnedRoomId`) |
| `ruma` (re-exported via SDK) | (transitive) | Matrix event types (`SyncRoomEvent`, `RoomMessageEventContent`) |

**Note**: `e2e-encryption` and `native-tls` features are not enabled. The
`bundled-sqlite` feature is enabled, providing persistent state storage (access token,
device ID, sync token, room state) in a SQLite database at `state_dir` with a
vendored libsqlite3.
