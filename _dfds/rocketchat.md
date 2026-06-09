# RocketChat Connection

## 1. Purpose

Python module (`RocketChatBot`) that manages the full lifecycle of a
RocketChat connection: WebSocket authentication, event streaming, message
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
    CONNECT(ConnectWebSocket)
    STREAM(StreamEvents)
    PARSE(ParseEvent)
    FILTER(FilterMentionOrDM)
    DISPATCH(DispatchMessage)
    SEND(SendReply)
    BOT_USER[BotUserId]
    HARNESS[Agent Loop]
    RC_WS[RocketChat WebSocket]

    CFG -->|"credentials"| AUTH
    CONNECT -->|"ws upgrade + connect msg"| RC_WS
    RC_WS -->|"connected event"| AUTH
    AUTH -->|"login method"| RC_WS
    RC_WS -->|"result {id, token}"| AUTH
    AUTH -->|"userId"| BOT_USER
    RC_WS -->|"raw JSON frame"| STREAM
    STREAM -->|"JSON event"| PARSE
    BOT_USER -->|"bot user id"| FILTER
    PARSE -->|"RawEvent"| FILTER
    FILTER -->|"IncomingMessage"| DISPATCH
    DISPATCH -->|"message"| HARNESS
    HARNESS -->|"BotReply"| SEND
    SEND -->|"sendMessage method"| RC_WS
```

### 2b. Error Handling & Fallbacks

The Python implementation has minimal internal error recovery — any WebSocket
exception propagates uncaught and terminates the process. External restart is
provided by the shell wrapper (`manual_start.sh`) with a fixed 5s delay and
retry counter.

```mermaid
flowchart TD
    AUTH(Authenticate)
    CONNECT(ConnectWebSocket)
    STREAM(StreamEvents)
    RESTART[Shell Restart Wrapper]

    AUTH -->|"exception"| RESTART
    CONNECT -->|"exception"| RESTART
    STREAM -->|"exception"| RESTART
    RESTART -.->|"fixed 5s delay"| CONNECT
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

### 2d. Ping/Pong Keepalive Deep Dive

The RocketChat server periodically sends `{"msg": "ping"}` to keep the
WebSocket alive. The bot responds immediately with `{"msg": "pong"}`. This
diagram decomposes the `StreamEvents` (STREAM) process from Level 1, showing
the internal dispatch that routes frames by `msg` field.

```mermaid
flowchart TD
    WS[RocketChat WebSocket]
    RECV(ReceiveFrame)
    PARSE(ParseJson)
    ROUTE(RouteByMsgField)
    CMD[(DispatchTable)]
    PONG(RespondPong)
    FORWARD(ForwardChanged)

    WS -->|"raw frame"| RECV
    RECV -->|"frame string"| PARSE
    PARSE -->|"json object"| ROUTE
    CMD -->|"msg → callback mapping"| ROUTE
    ROUTE -->|"msg == ping"| PONG
    PONG -->|"{msg: pong}"| WS
    ROUTE -->|"msg == changed"| FORWARD
    FORWARD -->|"RawEvent"| PARSE_PROC[ParseEvent]
```

**Dispatch table** — the `cbdist` dict maps each `msg` value to a callback:

| `msg` value    | Callback         | Action                            |
| -------------- | ---------------- | --------------------------------- |
| `"ping"`       | `_cb_ping`       | Send `{"msg": "pong"}`            |
| `"connected"`  | `_cb_connected`  | Send login method                 |
| `"result"`     | `_rt_dispatch`   | Extract userId, subscribe to room |
| `"changed"`    | `_cb_changed`    | Forward to ParseEvent             |

Note: the bot does **not** proactively send pings or monitor ping intervals —
it only responds to server-initiated pings. A missing server ping will not be
detected; a WebSocket error will propagate uncaught (see 2b).

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

#### `DispatchTable`

| Field    | Type         | Notes                             |
| -------- | ------------ | --------------------------------- |
| `msg`    | `String`     | WS frame type (key)               |
| `cb`     | `Callable`   | Async callback (value)            |

#### `RawEvent`

| Field    | Type     | Notes                                       |
| -------- | -------- | ------------------------------------------- |
| `msg`    | `String` | WS frame type (`"changed"`, `"ping"`, etc.) |
| `fields` | `Value`  | Event payload from RocketChat stream        |
