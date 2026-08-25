# Error Handling — REST → DDP Fallback

## 1. Purpose

If `chat.sendMessage` fails with `401`, a connection error, or a timeout, the
system falls back to DDP `sendMessage` **without** alias, so the reply is
delivered even when the REST path is down.

- Parent: [Happy Flow — Main Send (REST + Alias)](../rest-alias-send.md)

## 2. Diagram

```mermaid
flowchart TD
    REST_CLIENT(RestApiClient)
    RC_API[RocketChat REST API v1]
    DDP_SEND(DDP sendMessage<br/>without alias)

    REST_CLIENT -->|"chat.sendMessage"| RC_API
    RC_API -->|"200 OK"| OK["message sent with alias ✓"]
    RC_API -.->|"401 / connection error / timeout"| DDP_SEND
    DDP_SEND -.->|"plain sendMessage"| OK2["message sent (no alias) ✓"]
```

The alias is optional from the server's perspective — if the bot user lacks
`message-impersonate` permission, the server silently ignores the alias and
uses the bot's own username. The REST client does not check for this; it
blindly sends regardless of permission state.
