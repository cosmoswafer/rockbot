# RocketChat Connection

## 1. Purpose

Standalone reusable crate (`rocketchat`) that manages the full lifecycle of a
RocketChat connection: REST authentication, WebSocket event streaming, message
parsing/filtering, and reply delivery. Only DMs and @mentions are forwarded to
the agent.

- Upstream: [Configuration Management](config.md) provides `ServerConfig`
- Downstream: [Agent Loop](agent-harness.md) receives filtered
  `IncomingMessage` events; consumes `BotReply` for delivery to RocketChat

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    CFG[ServerConfig]
    AUTH(Authenticate)
    SESSION[(SessionStore)]
    CONNECT(ConnectWebSocket)
    STREAM(StreamEvents)
    PARSE(ParseEvent)
    FILTER(FilterMentionOrDM)
    DISPATCH(DispatchMessage)
    SEND(SendReply)
    BOT_USER[BotUserId]
    HARNESS[Agent Loop]
    RC_API[RocketChat REST API]
    RC_WS[RocketChat WebSocket]

    CFG -->|"credentials"| AUTH
    AUTH -->|"login request"| RC_API
    RC_API -->|"auth token + userId"| AUTH
    AUTH -->|"session"| SESSION
    AUTH -->|"userId"| BOT_USER
    SESSION -->|"auth token"| CONNECT
    CONNECT -->|"ws:// upgrade"| RC_WS
    RC_WS -->|"connected"| STREAM
    RC_WS -->|"raw JSON frame"| STREAM
    STREAM -->|"JSON event"| PARSE
    BOT_USER -->|"bot user id"| FILTER
    PARSE -->|"RawEvent"| FILTER
    FILTER -->|"IncomingMessage"| DISPATCH
    DISPATCH -->|"message"| HARNESS
    HARNESS -->|"BotReply"| SEND
    SEND -->|"POST /chat.sendMessage"| RC_API
    RC_API -->|"delivery status"| SEND
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    AUTH(Authenticate)
    CONNECT(ConnectWebSocket)
    STREAM(StreamEvents)
    REAUTH(ReAuthenticate)
    RECONNECT(ReconnectWs)
    BACKOFF(ExponentialBackoff)
    RC_API[RocketChat REST API]
    RC_WS[RocketChat WebSocket]

    AUTH -->|"401 / invalid credentials"| BACKOFF
    CONNECT -->|"connection refused"| BACKOFF
    STREAM -->|"ws closed / error"| RECONNECT
    BACKOFF -->|"retry delay"| REAUTH
    REAUTH -->|"new token"| RC_API
    RECONNECT -->|"ws:// upgrade"| RC_WS
    STREAM -->|"ping"| RC_WS
    RC_WS -->|"pong"| STREAM
```

### 2c. Message Filter Deep Dive

```mermaid
flowchart TD
    RAW[RawEvent]
    CHECK_TYPE(CheckEventType)
    CHECK_DM(CheckIsDirect)
    CHECK_AT(CheckAtMention)
    IGNORE(Ignore)
    MSG[IncomingMessage]
    BOT_ID[BotUserId]

    RAW -->|"event"| CHECK_TYPE
    CHECK_TYPE -->|"type != message"| IGNORE
    CHECK_TYPE -->|"type == message"| CHECK_DM
    CHECK_DM -->|"room is direct"| MSG
    CHECK_DM -->|"room is channel"| CHECK_AT
    CHECK_AT -->|"@bot mentioned"| MSG
    CHECK_AT -->|"not mentioned"| IGNORE
    BOT_ID -->|"id"| CHECK_AT
```

## 3. Data Structures

#### `IncomingMessage`

| Field       | Type     | Notes                                       |
| ----------- | -------- | ------------------------------------------- |
| `msg_id`    | `String` | RocketChat message ID                       |
| `room_id`   | `String` | Room/Channel ID                             |
| `sender_id` | `String` | User who sent the message                   |
| `text`      | `String` | Message text (mentions stripped)            |
| `is_dm`     | `bool`   | True if direct message                      |
| `timestamp` | `i64`    | Unix timestamp                              |

#### `BotReply`

| Field       | Type     | Notes                                  |
| ----------- | -------- | -------------------------------------- |
| `room_id`   | `String` | Target room                            |
| `text`      | `String` | Reply content (Markdown supported)     |
| `thread_id` | `Option<String>` | Reply in thread if set         |

#### `SessionStore`

| Field        | Type     | Notes                               |
| ------------ | -------- | ----------------------------------- |
| `auth_token` | `String` | X-Auth-Token from login             |
| `user_id`    | `String` | Bot user ID                         |
| `ws_url`     | `String` | Resolved WebSocket URL              |

#### `RawEvent`

| Field    | Type     | Notes                                       |
| -------- | -------- | ------------------------------------------- |
| `msg`    | `String` | WS frame type (`"changed"`, `"ping"`, etc.) |
| `fields` | `Value`  | Event payload from RocketChat stream        |
