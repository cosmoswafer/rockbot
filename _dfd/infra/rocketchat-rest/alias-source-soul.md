# Alias Source — Soul Memory to REST Send

## 1. Purpose

The alias is extracted from per-room soul memory (Layer 3) at send time. The
`self_display_name()` function resolves the name from `SoulMemory.front.name`,
which is populated from the `name:` front-matter key, or — for unmigrated
legacy souls — derived once at load from the `- My name is ...` bullet. Legacy
souls are upgraded to front matter on first load, so `name:` becomes the normal
source. The agent loop in `main.rs` orchestrates this flow inline.

- Upstream: [Memory Management](../../memory/memory/soul-editing.md) Layer 3
  (soul) stores the bot's per-room self-display name, extracted via
  `self_display_name()`
- Upstream: [Shared Structures](structures.md) — `chat.sendMessage` request
  shape

## 2. Diagram

```mermaid
flowchart TD
    HAR_RESP[Harness returns BotReply]
    SOUL[(soul.md<br/>per-room)]
    PARSE(Parse self_display_name)
    ALIAS{Has name?}
    REST_SEND(REST chat.sendMessage<br/>with alias)
    DDP_SEND(DDP sendMessage<br/>no alias)
    RESULT_OK[Message sent ✓]

    HAR_RESP -->|"reply text"| PARSE
    SOUL -->|"SoulMemory.front.name"| PARSE
    PARSE --> ALIAS
    ALIAS -->|"yes: alias string"| REST_SEND
    ALIAS -->|"no"| DDP_SEND
    REST_SEND -->|"success"| RESULT_OK
    REST_SEND -.->|"error"| DDP_SEND
    DDP_SEND --> RESULT_OK
```

The REST send is fire-and-forget: the reply is sent and the result logged.
There is no DDP verification step — the server broadcasts the message to all
subscribers via DDP `changed` events, which is handled by the normal event
loop.
