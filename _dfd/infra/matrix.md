# Matrix Connection

## 1. Purpose

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
1. Enable `e2e-encryption` feature + crypto store setup (Section 2e notes).
2. Users must create **unencrypted** DMs (disable encryption toggle in their client
   before sending the first message) and manually accept the room invite (bot does
   not auto-join — see Section 2c notes).

With E2EE enabled, the SDK's built-in crypto store would handle Olm/Megolm key
exchange and message decryption transparently.

Messages from joined rooms are parsed into the shared `IncomingMessage` type
(defined in `crate-rocketchat/src/types.rs` — reused as the cross-platform
message contract). The agent harness and tools are unaware of the underlying
platform.

- Upstream: [Configuration Management](config.md) provides `MatrixServerConfig`
  (homeserver URL, user_id, password, device_id, state_dir)
- Upstream: [Agent Loop](../agent/agent-loop.md) calls `connect_and_run()` with a
  message handler callback
- Downstream: [Agent Harness](../agent/agent-harness.md) receives filtered
  `IncomingMessage` structs; sends replies through `MessagingClient::send_reply()`

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    CFG[MatrixServerConfig]
    CLIENT(CreateMatrixClient)
    LOGIN(LoginToHomeserver)
    SYNC(StartSyncLoop)
    DISPATCH(DispatchRoomMessage)
    FILTER(FilterMessage)
    HARNESS[Agent Loop]
    REPLY(SendReply)
    MATRIX[Matrix Homeserver]

    CFG -->|"homeserver + credentials"| CLIENT
    CLIENT -->|"Client::new(homeserver_url)"| LOGIN
    LOGIN -->|"login(user_id, password)"| MATRIX
    MATRIX -->|"session token"| LOGIN
    LOGIN -->|"authenticated client"| SYNC
    SYNC -->|"sync loop started"| MATRIX
    MATRIX -->|"m.room.message event"| SYNC
    SYNC -->|"SyncEvent::Room timeline"| DISPATCH
    DISPATCH -->|"SyncRoomEvent"| FILTER
    FILTER -->|"IncomingMessage"| HARNESS
    HARNESS -->|"BotReply text"| REPLY
    REPLY -->|"RoomMessageEventContent"| MATRIX
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    LOGIN(LoginToHomeserver)
    SYNC(StartSyncLoop)
    ERR_TOKEN[Error: M_UNKNOWN_TOKEN<br/>force_relogin = true]
    ERR_AUTH[Error: AuthFailed]
    ERR_SYNC[Error: Provider<br/>transient sync error]
    RECONNECT(ReconnectWithBackoff)
    RESTORE[Session restored from store<br/>skip login]
    AGENT[Agent Loop exits<br/>after 5 consecutive auth failures]

    LOGIN -->|"401 / 403"| ERR_AUTH
    LOGIN -->|"network error"| RECONNECT
    SYNC -->|"M_UNKNOWN_TOKEN"| ERR_TOKEN
    SYNC -->|"other sync error"| ERR_SYNC
    ERR_SYNC -->|"transient"| RECONNECT
    RECONNECT -->|"backoff + retry"| RESTORE
    RESTORE -->|"logged_in() = true"| SYNC
    RESTORE -->|"force_relogin or<br/>logged_in() = false"| LOGIN
    ERR_TOKEN -->|"AuthFailed"| RECONNECT
    ERR_AUTH -->|"5 consecutive"| AGENT
```

The matrix-rust-sdk `sync()` returns on the **first** sync error — there is no
internal retry within the SDK. The `connect_and_run()` method inspects the
error: if it is `M_UNKNOWN_TOKEN` (detected via
`client_api_error_kind() → ErrorKind::UnknownToken`), the `force_relogin` flag
is set and the error is returned as `AuthFailed`; all other sync errors are
returned as `Provider` errors. On reconnect, `connect_and_run()` checks
`force_relogin` and `client.matrix_auth().logged_in()`: if the flag is false
and a session exists in the SQLite store, login is skipped (session restored);
otherwise, a fresh login is performed.

The agent loop applies exponential backoff on all errors. After 5 consecutive
`AuthFailed` errors, the bot exits rather than retrying indefinitely.
`retry_count` resets to 0 after a connection lasting >= 60 seconds, so
transient errors always start with a short backoff.

### 2c. Message Filter Deep Dive

Matrix rooms deliver all timeline events to the sync handler. The filter
identifies messages that should be forwarded to the agent: DMs (rooms with
≤2 members) always forward; group rooms require @mentions. Self-messages
(events from the bot's own user_id) are silently dropped.

```mermaid
flowchart TD
    EVT[SyncRoomEvent<br/>m.room.message]
    FILTER(FilterMessage)
    BOT_USER[BotUserId]
    ROOMS[(JoinedRooms)]
    DISPATCH[DispatchToAgent]
    DROP[Silently Drop]

    EVT -->|"event + sender"| FILTER
    BOT_USER -->|"bot user_id"| FILTER
    ROOMS -->|"room membership"| FILTER
    FILTER -->|"self-message"| DROP
    FILTER -->|"DM or @mention"| DISPATCH
    FILTER -->|"no @mention"| DROP
    DISPATCH -->|"IncomingMessage"| AGENT[Agent Loop]
```

**Filter rules** (evaluated in order):

1. **Skip non-joined rooms**: `room.state() != Joined` → drop
2. **Skip non-original events**: edits, reactions → drop
3. **Skip self**: `event.sender == bot_user_id` → drop (logged at `info!` level)
4. **Skip historical**: `origin_server_ts + 600s < startup_ts` → drop (10-min grace window
   allows messages sent shortly before restart to be processed)
5. **Skip non-text/non-image**: `msgtype != "m.text"` and `msgtype != "m.image"` → drop
   (encrypted `m.room.encrypted` events also dropped — no handler registered for them)
6. **DM check**: room member count ≤ 2 → forward unconditionally (`is_dm = true`)
7. **Mention check** (group rooms only): message body must contain
   `@bot_user_id` (full MXID or localpart `@username`), OR `m.mentions.user_ids`
   must include the bot's MXID. Logs `user_id`, `localpart`, `body`, `mentions`,
   and `member_count` at `info!` level on both match and mismatch.

**Room invite handling** *(by design)*: The bot never auto-joins rooms.
Only `RoomState::Joined` rooms are processed; `RoomState::Invited` is silently
ignored. Invites must be accepted manually (Element / homeserver admin).

### 2d. Sync Loop Deep Dive

The matrix-rust-sdk sync loop runs as a background task. Events are delivered
to registered event handlers. The `connect_and_run()` method registers a
room message handler before starting sync.

```mermaid
flowchart TD
    MATRIX[Matrix Homeserver]
    SDK[matrix-rust-sdk Client]
    HANDLER(RoomEventHandler)
    PARSE(ParseMessageEvent)
    FILTER(FilterMessage)
    DISPATCH(DispatchToAgent)

    MATRIX -->|"GET /_matrix/client/v3/sync<br/>(long poll)"| SDK
    SDK -->|"sync response"| SDK
    SDK -->|"room timeline event"| HANDLER
    HANDLER -->|"SyncRoomEvent"| PARSE
    PARSE -->|"text + sender + room"| FILTER
    FILTER -->|"IncomingMessage"| DISPATCH
```

**Sync parameters**: `SyncSettings::default()` includes a 30-second long-poll
timeout. Subsequent syncs resume from the stored `since` token, persisted in
the SQLite state store at `state_dir`. On reconnect (without re-login), the SDK
restores the `since` token and resumes incremental sync; on re-login (fresh
session), the first sync is a full initial sync.

### 2e. Authentication Deep Dive

Authentication uses the Matrix `m.login.password` flow via the SDK's
`Client::login_username()` builder. Sessions are persisted in a SQLite state
store at `state_dir`, enabling session restoration on reconnect.

```mermaid
flowchart TD
    CLIENT[matrix_sdk::Client<br/>sqlite_store state_dir]
    CHECK{logged_in?<br/>and not force_relogin}
    LOGIN[login_username]
    MATRIX[Matrix Homeserver]
    STORE[(SQLite StateStore)]
    SYNC[Start Sync Loop]

    CLIENT --> CHECK
    CHECK -->|"yes"| SYNC
    CHECK -->|"no"| LOGIN
    LOGIN -->|"POST /_matrix/client/v3/login<br/>{type: m.login.password}"| MATRIX
    MATRIX -->|"access_token + device_id"| LOGIN
    LOGIN -->|"session persisted"| STORE
    SYNC -->|"sync error"| ERRCHECK{M_UNKNOWN_TOKEN?}
    ERRCHECK -->|"yes"| FORCE["force_relogin = true<br/>return AuthFailed"]
    ERRCHECK -->|"no"| RETURN["return Provider error"]
```

**Session persistence**: The `Client::builder().sqlite_store(state_dir, None)`
call configures a SQLite state store at `state_dir` (default `./tmp/matrix-sdk`).
On reconnect, the SDK restores the access token, device ID, and sync token
from the store. If `client.matrix_auth().logged_in()` returns `true` and
`force_relogin` is `false`, login is skipped — the bot resumes sync with the
restored session, avoiding unnecessary re-authentication that would invalidate
the previous token.

**`force_relogin` flag**: When sync fails with `M_UNKNOWN_TOKEN`
(`ErrorKind::UnknownToken`), the `force_relogin` `AtomicBool` is set to `true`.
On the next `connect_and_run()` call, this forces a fresh login even if the
SQLite store has a cached session. The flag is cleared (swap to `false`) after
the login decision is made.

**User ID validation**: After login, `client.user_id()` is validated to ensure it
returns `Some` — if `None` (corrupted session), the connection returns
`AuthFailed` immediately rather than silently using an empty string for mention
matching and self-message filtering.

**E2EE**: The SDK automatically handles Olm/Megolm key exchange and message
decryption **when the `e2e-encryption` feature is enabled**. Currently this feature
is not compiled in (see Section 1 note). Encrypted messages arrive as
`m.room.encrypted` events and are dropped — no handler is registered for them.
To enable E2EE:
1. Add `"e2e-encryption"` to `features` in `crate-rockbot/Cargo.toml` for `matrix-sdk`.
2. The SDK's crypto store will use the `state_dir` path for Olm/Megolm session storage.
3. Device verification will need handling (e.g. auto-accept or a `/verify` command).
When enabled, decrypted messages arrive at the room event handler as plain text.

### 2f. Reply Sending

Replies are sent as plain text `m.room.message` events with `msgtype: "m.text"`.

```mermaid
flowchart TD
    HARNESS[Agent Loop]
    BUILD(BuildMessageContent)
    SEND(RoomSend)
    MATRIX[Matrix Homeserver]
    FORMATTED{Has markdown?}

    HARNESS -->|"room_id + text + alias"| BUILD
    BUILD --> FORMATTED
    FORMATTED -->|"yes"| MD["RoomMessageEventContent<br/>(text + formatted_body)"]
    FORMATTED -->|"no"| PLAIN["RoomMessageEventContent<br/>(text_plain)"]
    MD --> SEND
    PLAIN --> SEND
    SEND -->|"PUT /_matrix/client/v3/rooms/{roomId}/send/<txnId>"| MATRIX
```

**Markdown formatting**: If the bot reply contains markdown formatting
(headers, bold, code blocks), the message is sent with `formatted_body`
(org.matrix.custom.html) alongside the plain-text `body`. The Matrix SDK's
`RoomMessageEventContent::text_markdown()` handles this automatically.

**Alias**: Matrix does not support per-message sender alias like RocketChat.
The `alias` parameter in `send_reply()` is ignored for the Matrix platform.
The bot always sends under its own user identity.

### 2g. Typing Indicator

Matrix typing indicators are sent as ephemeral events to the room.

```mermaid
flowchart TD
    HARNESS[Agent Loop]
    TYPING(SendTypingState)
    MATRIX[Matrix Homeserver]

    HARNESS -->|"room_id + typing=true"| TYPING
    TYPING -->|"PUT /_matrix/client/v3/rooms/{roomId}/typing/{userId}<br/>{typing: true, timeout: 5000}"| MATRIX
    HARNESS -->|"room_id + typing=false"| TYPING
    TYPING -->|"PUT /_matrix/client/v3/rooms/{roomId}/typing/{userId}<br/>{typing: false}"| MATRIX
```

The typing timeout is set to 5000ms per the Matrix spec recommendation. The
heartbeat task in the agent loop refreshes it every 2 seconds, matching the
RocketChat behavior.

### 2h. Image Attachment Reception (Approach A)

When a user sends an image in a Matrix room, the event has `msgtype: "m.image"`
with an `mxc://` URI pointing to the media on the homeserver. The SDK provides
`Client::media()` for downloading media content directly.

Unlike the RocketChat path (which downloads in the harness layer via HTTP),
Matrix images are **downloaded and base64‑encoded in the platform event handler**
(Approach A). The encoded `data:` URI is placed in `attachments[0].title_link`,
and the harness `download_attachment_refs()` detects the `data:` scheme and
passes it through without a redundant HTTP fetch.

```mermaid
flowchart TD
    MATRIX[Matrix Homeserver]
    EVT[m.room.message<br/>msgtype: m.image]
    PARSE(ParseImageEvent)
    DOWNLOAD(DownloadViaSdk)
    ENCODE(Base64Encode)
    BUILD(BuildIncomingMessage)
    DISPATCH(DispatchToAgent)
    HARNESS[Agent Harness]
    BYPASS[download_attachment_refs<br/>detects data: → passthrough]

    MATRIX -->|"sync event"| EVT
    EVT -->|"ImageMessageEventContent"| PARSE
    PARSE -->|"mxc:// URI via source field"| DOWNLOAD
    DOWNLOAD -->|"client.media().get_file()"| MATRIX
    MATRIX -->|"image bytes"| DOWNLOAD
    DOWNLOAD -->|"raw bytes"| ENCODE
    ENCODE -->|"data:image/png;base64,..."| BUILD
    BUILD -->|"IncomingMessage<br/>attachments[0].title_link = data: URI"| DISPATCH
    DISPATCH -->|"process_message()"| HARNESS
    HARNESS -->|"download_attachment_refs()<br/>checks title_link starts with data: → passthrough"| BYPASS
```

**Limitations**:
- Encrypted images (`m.room.encrypted` + `file` field) are not supported — the
  `e2e-encryption` feature is not enabled (see Section 1). Only `MediaSource::Plain`
  (unencrypted `mxc://` URIs) can be downloaded.
- E2EE room images arrive as opaque `m.room.encrypted` events and are dropped
  by the event handler — no handler is registered for them.

**Mapping to `IncomingMessage`**:
- `text` → `body` field from the event content (filename or media caption)
- `attachments[0].title` → `body` field (filename)
- `attachments[0].title_link` → `data:image/{type};base64,...` (pre‑encoded data URI)
- `attachments[0].image_type` → `mimetype` from `info` (if present)
- `attachments[0].image_dimensions` → `{width, height}` from `info` (if present)
- `attachments[0].image_size` → `size` from `info` (if present)
- `file` → `None` (image data travels via `attachments`, not via `file`)

### 2i. Room Name Resolution

Matrix rooms have canonical aliases (e.g. `#room:server`), display names, and
room IDs. The mapping to `IncomingMessage` fields:

```mermaid
flowchart TD
    ROOM[Matrix Room]
    ALIAS(CanonicalAlias)
    DNAME(DisplayName)
    BUILD(BuildIncomingMessage)

    ROOM -->|"room.canonical_alias()"| ALIAS
    ROOM -->|"room.display_name()"| DNAME
    ALIAS -->|"room_name (slug)"| BUILD
    DNAME -->|"room_fname (Unicode)"| BUILD
```

- `room_name` → canonical alias localpart without `#` and `:server` suffix
  (e.g. `#general:example.org` → `"general"`). Falls back to room ID localpart
  if no canonical alias.
- `room_fname` → room display name from `m.room.name` state event. Falls back
  to `room_name` if unset.
- `is_dm` → `true` if room has exactly 2 joined members (bot + one other). DMs bypass the mention check — all messages are forwarded.

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
