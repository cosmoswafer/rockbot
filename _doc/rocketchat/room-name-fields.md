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

### REST fallback via `rooms.info` (2026-07-08)

A REST fallback (`resolve_room_fname` via `rooms.info` / `rooms.get`) was
added in commit `6d4526e` and is now **safely wired into the message handler**
in `crate-rockbot/src/main.rs:417-451`.

**How it works:**

1. When `room_fname` is empty in a DDP message, the handler checks whether
   the sender is an `RcPlatformSender` (RocketChat platform).
2. It retrieves the `auth_token` via `MessageSender::auth_token_for_rest()`
   — this is always non-empty after a successful login.
3. It creates a `RestApiClient` via `MessageSender::rest_client()` and calls
   `resolve_room_fname(room_id)` which:
   - First checks an in-memory cache (stale per-client instance, so cache
     hits only within the same message handler call).
   - Falls back to `GET /api/v1/rooms.info?roomId=...`.
   - Falls back further to `GET /api/v1/rooms.get` for a full room listing.
4. If resolved, the `fname` is used as the display name. If the REST call
   fails or returns no `fname`, the bot **panics** with a clear message
   (identical to the old panic, preserving the crash-fast policy).

**Safety invariant:** The auth token check (`!auth_token.is_empty()`)
prevents calling `RestApiClient::new()` with empty credentials, avoiding
the `assert!` panic that plagued the earlier attempt. The `assert!`s remain
in `RestApiClient::new()` as a safety net for other callers.

**Cache note:** Each call creates a fresh `RestApiClient`, so the
`room_name_cache` within it starts empty. Future optimization could store a
shared `Arc<Mutex<RestApiClient>>` on `RcPlatformSender` to share the cache
across messages.

**Panic still possible:** If the REST API is unreachable, the room has no
`fname` in RocketChat's database either, or the auth token is missing (edge
case), the bot still panics. This is intentional — channels without display
names should be fixed at the RocketChat server level.
