# Rocket.Chat Room Name Fields: `name` vs `fname`

## Discovery

Tested against real server at `rc.tokyofy.top` (2026-06-10). Rocket.Chat rooms have
**two** name fields:

| Field | Location | Content |
|-------|----------|---------|
| `name` | REST, DDP `args[1].roomName` | URL slug — ASCII only, lowercase |
| `fname` | REST, DDP `args[1].fname` | Display name — can contain Chinese, emoji, any Unicode |

## Server evidence (real rooms)

```
name: shit          fname: 💩💩💩SHIT屎
name: pigbar        fname: 🐵🐷🦁🐶🐸豬欄PIGBAR
name: sen1-lin2-sheng1-tai4  fname: 🐵🌴🐷森林生態
name: general       fname: (empty)
name: atomkb        fname: atomkb
```

## Official source

From Rocket.Chat's [`IRoom.ts`](https://github.com/RocketChat/Rocket.Chat/blob/develop/packages/core-typings/src/IRoom.ts#L13-L14):

```ts
export interface IRoom extends IRocketChatRecord {
    t: RoomType;
    name?: string;   // URL slug (ASCII)
    fname?: string;  // friendly/display name (Unicode)
    ...
}
```

`RoomAdminFieldsType` also lists `'fname'` as an admin-visible field.

## Current code

`crate-rocketchat/src/types.rs:157-174` extracts both `roomName` and `fname`:

```rust
let (room_name, is_dm) = if args.len() > 1 {
    let name = args[1].get("roomName").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let room_type = args[1].get("t").and_then(|v| v.as_str());
    let dm = match room_type {
        Some("d") => true,
        Some(_) => false,
        None => name.is_empty() || name == "DIRECT_MESSAGES",
    };
    (name, dm)
} else {
    (String::new(), true)
};

let room_fname = if args.len() > 1 {
    args[1].get("fname").and_then(|v| v.as_str()).unwrap_or("").to_string()
} else {
    String::new()
};
```

### Precedence

When both are present, prefer `fname` for display/log messages and
`name` (or `roomName`) for matching/registration lookup.

### DDP `__my_messages__` subscription caveat

The bot uses the DDP subscription `["__my_messages__", false]` (`crate-rocketchat/src/ddp.rs:52-59`)
to receive all room messages. This subscription is **non-persistent** and only sends "changed"
events — it never sends "added" events with full room metadata.

In practice, the `args[1]` room-metadata object in "changed" events **does not always include
`fname`**. RocketChat only includes it conditionally (e.g. when the room actually has a non-empty
`fname`). When `fname` is absent or `""`, the bot has no way to know the display name from this
subscription alone.

### WebDAV directory — no fallback to pinyin slug

`crate-rockbot/src/harness.rs:1536-1541` uses `room_fname` for the WebDAV directory name.
When `room_fname` is empty, the function **panics** — no fallback to the URL slug:

```rust
fn compute_webdav_dir(room_name: &str, room_fname: &str, is_dm: bool) -> String {
    assert!(!room_fname.is_empty(), "compute_webdav_dir: room_fname must not be empty");
    format!("{}-{}", if is_dm { "d" } else { "r" }, room_fname)
}
```

Displaying the pinyin/internal codename as a room name is unacceptable — crash fast so
the missing `fname` is immediately noticed and fixed at the RocketChat server level.

### RoomCache removed (2026-06-10)

The `"rooms"` DDP subscription on `rc.tokyofy.top` never sends a `"ready"` response,
so the **RoomCache** (populated from `"rooms"` subscription to fill missing `fname`
values) was **removed entirely** — both from code and DFDs:

- `RoomCache` / `CachedRoom` structs removed from `types.rs`
- `subscribe_rooms_message` / `is_added` removed from `ddp.rs`
- `wait_for_rooms_ready` and rooms subscription logic removed from `client.rs`
- `MessageFilter::filter()` no longer takes a `room_cache` parameter
- DFD sections 2g (Room Name Cache) and 2h (Subscription Ordering) deleted

Without the cache, `room_fname` is sourced **only** from the per-event
`args[1].fname` field. When absent, `room_fname` stays empty and the bot
**crashes** — no fallback to pinyin slug.

### REST fallback attempted — breaks the application (2026-06-11)

A REST fallback (`resolve_room_fname` via `rooms.info` / `rooms.get`) was
added in commit `6d4526e` as the replacement for RoomCache. However, it
**crashes the bot** because:

1. `RestApiClient::new()` uses `assert!` to check that `user_id` and
   `auth_token` are non-empty (`crate-rocketchat/src/rest.rs:59-61`)
2. `auth_token` comes from `MessageSender.auth_token` which is populated
   with `self.auth_token.as_deref().unwrap_or("")` (`client.rs:313`)
3. If the DDP auth token is missing (reconnection edge case, or login
   response lacks a token), an empty string is passed, the `assert!` fires,
   and the bot panics

This REST fallback must **not** be used. Room name resolution relies
solely on `args[1].fname` from DDP. When `fname` is absent, the bot
crashes — no fallback to `roomName` (pinyin slug) and no REST API call.

Any future improvement to room name handling must avoid REST API calls
from the message handler and must not use `assert!` for runtime data.
