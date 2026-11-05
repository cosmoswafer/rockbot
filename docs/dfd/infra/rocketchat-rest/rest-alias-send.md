# Happy Flow — Main Send (REST + Alias)

## 1. Purpose

Messages are sent **REST-first with alias**: the agent loop produces a reply
text, the bot's self-display name is extracted from soul memory, and the REST
`chat.sendMessage` endpoint is called with `alias`. If the REST call fails for
any reason, the system falls back to DDP `sendMessage` **without** alias.

- Upstream: [Configuration Management](../config/main-path.md) provides server
  hostname and TLS settings
- Upstream: [RocketChat Connection](../rocketchat/main-path.md) provides
  `user_id` and `auth_token` from the DDP `login` response
- Upstream: [Memory Management](../../memory/memory/soul-editing.md) Layer 3
  (soul) stores the bot's per-room self-display name, extracted via
  `self_display_name()`
- Upstream: [Shared Structures](structures.md) — endpoints, `RestApiClient`,
  `RoomInfo`
- Downstream: [Error Handling — REST → DDP Fallback](level-2/rest-ddp-fallback.md)

## 2. Diagram

```mermaid
flowchart TD
    HARNESS[Agent Harness]
    MEMORY[(Soul Memory<br/>soul.md)]
    EXTRACT(ExtractSelfDisplayName)
    REST_CLIENT(RestApiClient)
    RC_API[RocketChat REST API v1]
    DDP_CLIENT(MessageSender)
    DDP_WS[RocketChat DDP]

    HARNESS -->|"BotReply (text)"| EXTRACT
    MEMORY -->|"soul content"| EXTRACT
    EXTRACT -->|"alias (e.g. 零夢✨)"| REST_CLIENT
    REST_CLIENT -->|"POST /api/v1/chat.sendMessage {msg, alias}"| RC_API
    RC_API -->|"HTTP 200 {message: {_id, alias}}"| REST_CLIENT
    REST_CLIENT -. REST success .- HARNESS
    REST_CLIENT -. REST error .-> DDP_CLIENT
    DDP_CLIENT -->|"sendMessage (no alias)"| DDP_WS
```
