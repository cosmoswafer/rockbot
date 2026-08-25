# Authentication Deep Dive

## 1. Purpose

The login flow uses DDP method calls over the WebSocket (`ddp::login_message()`
in `crate-rocketchat/src/ddp.rs:36`). The Rocket.Chat `login` method requires
the password to be pre-hashed with **SHA-256**, sent as a lowercase hex digest
alongside the algorithm name. The Rust implementation uses `sha2::Digest` to
hash the password before constructing the payload.

All DDP method calls (`login`, `stream-notify-room`, `sendMessage`) use a shared
`AtomicU64` counter (`MSG_ID`) that generates sequential, unique IDs per call.
This ensures the server can match each `"result"` response to its originating
`"method"` call — required by the DDP protocol to prevent "Match failed" errors
on duplicate IDs.

References: [Happy Flow (Main Success Path)](../main-path.md)

## 2. Spec

**Implementation** (`ddp::login_message()`):

```json
{
    "msg": "method",
    "method": "login",
    "id": "<next_id()>",
    "params": [
        {
            "user": { "username": "rockbot" },
            "password": {
                "digest": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
                "algorithm": "sha-256"
            }
        }
    ]
}
```

**sendMessage implementation** (`ddp::send_message_payload()`):

The `sendMessage` method requires a client-generated `_id` string field inside
the `params` message object — RocketChat validates this on the server side and
rejects messages without it. message `_id` uses `unique_msg_id()` (timestamp-seq format), method `id` uses `next_id()` (sequential).

An optional `alias` field in `params[0]` overrides the displayed sender name for
this message — the server substitutes the alias in place of the bot's real username.
Requires the `message-impersonate` permission on the RocketChat user role.

```json
{
    "msg": "method",
    "method": "sendMessage",
    "id": "<next_id()>",
    "params": [{
        "_id": "<next_id()>",
        "rid": "room-uuid",
        "msg": "reply text",
        "alias": "TotallyRealHuman"
    }]
}
```

The `alias` field is optional. When omitted or when the user lacks
`message-impersonate`, the message is sent under the bot's own username
with no error.

**Server response** on success:

```json
{
    "msg": "result",
    "id": "<next_id()>",
    "result": {
        "id": "user-id",
        "token": "auth-token",
        "tokenExpires": { "$date": 1480377601 }
    }
}
```

The `tokenExpires` field is **not consumed** by the current implementation. If the
server has `Accounts_LoginExpiration` enabled, the token has a finite TTL. Once
expired, REST API calls using `X-Auth-Token`/`X-User-Id` return `401
Unauthorized` — the REST client has no refresh mechanism and the DDP WebSocket
session (kept alive independently via pings) does not automatically refresh the
token for REST. See [RocketChat REST API §2f](../../rocketchat-rest/rest-alias-send.md) for the
impact on REST calls.
