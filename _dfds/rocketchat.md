# RocketChat Connection

## 1. Purpose

Python module (`RocketChatBot`) that manages the full lifecycle of a
RocketChat connection: WebSocket authentication, subscription to message
stream, event dispatch, message parsing/filtering, and reply delivery.
Only DMs and messages starting with `@botname` are forwarded to the agent.

- Upstream: [Configuration Management](config.md) provides configuration
  (loaded as `SimpleNamespace` from `config.json` in the Python implementation)
- Downstream: [Agent Loop](agent-harness.md) receives filtered messages via
  callback `(sender_name, room_name, room_id, text)`; sends replies through
  the `bot` helper class wrapping `sendMsg()`

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    CFG[ServerConfig]
    AUTH(Authenticate)
    CONNECT(ConnectWebSocket)
    SUB(SubscribeStream)
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
    AUTH -->|"subscribe"| SUB
    SUB -->|"sub method"| RC_WS
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

The `_cb_changed` callback (`bot/RocketChatBot.py:116`) implements a four-stage
decision chain. Messages from the bot itself are silently dropped. The bot
responds to three cases: (1) `@botname` at the start of a channel message,
(2) a specific registered room, or (3) a direct message with no room name.

```mermaid
flowchart TD
    RAW[RawEvent]
    SKIP_SELF{SkipSelf}
    MENTION{StartsWithAt}
    REG_RM{RegisteredRoom}
    IS_DM{IsDirectMessage}
    IGNORE(Ignore)
    MSG[Callback args]
    BOTNAME["@username"]
    ROOMS[(RegisteredRooms)]

    RAW --> SKIP_SELF
    SKIP_SELF -->|"sender_id == bot_uid"| IGNORE
    SKIP_SELF -->|"otherwise"| MENTION
    MENTION -->|"room_name && starts w/ @botname"| MSG
    MENTION -->|"otherwise"| REG_RM
    REG_RM -->|"room_name in rooms"| MSG
    REG_RM -->|"otherwise"| IS_DM
    IS_DM -->|"no room_name"| MSG
    IS_DM -->|"otherwise"| IGNORE
    BOTNAME -->|"@username"| MENTION
    ROOMS -->|"lookup"| REG_RM
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

### 2e. Subscription Deep Dive

After authentication succeeds (`_rt_dispatch` receives `result` with `id` and
`token`), `_gologin()` subscribes to the `stream-room-messages` endpoint with
the `__my_messages__` scope. Once subscribed, the server begins delivering
`"changed"` events for all messages visible to the bot user.

```mermaid
flowchart TD
    WS[RocketChat WebSocket]
    AUTH_RX(ReceiveAuthResult)
    SUB(SubscribeToStream)
    PARAMS[(SubscriptionParams)]
    STREAM_PROC[StreamEvents]

    WS -->|"result {id, token}"| AUTH_RX
    AUTH_RX -->|"login ok"| SUB
    PARAMS -->|"stream-room-messages, __my_messages__"| SUB
    SUB -->|"sub method {msg: sub, name: stream-room-messages}"| WS
    WS -->|"changed events"| STREAM_PROC
```

**Subscription payload** sent to the WebSocket:

```json
{
    "msg": "sub",
    "id": "ABCROCK",
    "name": "stream-room-messages",
    "params": ["__my_messages__", false]
}
```

The `params` array controls which messages are received: `"__my_messages__"`
scopes to the authenticated user, and `false` disables the `args` shorthand
(ensuring full message payloads are delivered).

## 3. Data Structures

The Python implementation does not define formal typed structures (dataclasses,
TypedDicts, etc.). Data flows through positional callback arguments and ad-hoc
dicts. The tables below describe both the conceptual types and how each field
maps to the current code.

#### `IncomingMessage`

| Field       | Type     | Python mapping                                       |
| ----------- | -------- | ---------------------------------------------------- |
| `msg_id`    | `String` | **Not parsed** — not available in callback           |
| `room_id`   | `String` | `rid` arg passed to callback                         |
| `sender_name` | `String` | `usr` arg passed to callback (username, not user ID) |
| `text`      | `String` | `txt` arg; mentions stripped via `.replace()` for @channel messages, **not stripped for DMs** |
| `is_dm`     | `bool`   | **Not a boolean** — inferred from `rom == "DIRECT_MESSAGES"` or `rom == ""` |
| `timestamp` | `i64`    | **Not parsed** — not available in callback           |

#### `BotReply`

| Field       | Type     | Python mapping                              |
| ----------- | -------- | ------------------------------------------- |
| `room_id`   | `String` | `bot.rid` on the `bot` helper class         |
| `text`      | `String` | `msg` arg to `bot.reply(msg)`               |
| `thread_id` | `Option<String>` | **Not supported** — `sendMsg()` has no threading capability |

The `bot` helper class (`bot/bot.py`) also provides `replyQ(msg)` (code-block
formatted reply) and `typing(state)` (typing indicator) which are not part of
the `BotReply` concept.

#### `DispatchTable`

| Field    | Type         | Python mapping                             |
| -------- | ------------ | ------------------------------------------ |
| `msg`    | `String`     | Key in `cbdist` dict (e.g. `"ping"`)       |
| `cb`     | `Callable`   | Value in `cbdist` dict (e.g. `_cb_ping`)   |

#### `RawEvent`

| Field    | Type     | Python mapping                              |
| -------- | -------- | ------------------------------------------- |
| `msg`    | `String` | `jds["msg"]` after JSON parse in `_dispatch_ds` |
| `fields` | `Value`  | The full parsed JSON object (`jds`) with `fields.args` for message payload |
