# Reply Sending

## 1. Purpose

Replies are sent as plain text `m.room.message` events with `msgtype: "m.text"`.

## 2. Diagram

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
