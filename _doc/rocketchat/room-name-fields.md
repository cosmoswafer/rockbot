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

## Current code impact

`crate-rocketchat/src/types.rs:111-113` extracts only `roomName` (the slugs):

```rust
let name = args[1]
    .get("roomName")       // ← this is `name`, the ASCII slug
    .and_then(|v| v.as_str())
    .unwrap_or("")
    .to_string();
```

For Chinese/display room names, also check `args[1].fname`:

```rust
let fname = args[1]
    .get("fname")          // ← friendly name (Unicode)
    .and_then(|v| v.as_str());
```

### Precedence

When both are present, prefer `fname` for display/log messages and
`name` (or `roomName`) for matching/registration lookup.

### DDP `__my_messages__` subscription caveat

The bot uses the DDP subscription `["__my_messages__", false]` (`crate-rocketchat/src/ddp.rs:38-45`)
to receive all room messages. This subscription is **non-persistent** and only sends "changed"
events — it never sends "added" events with full room metadata.

In practice, the `args[1]` room-metadata object in "changed" events **does not always include
`fname`**. RocketChat only includes it conditionally (e.g. when the room actually has a non-empty
`fname`). When `fname` is absent or `""`, the bot has no way to know the display name from this
subscription alone.

### WebDAV directory fallback

`crate-rockbot/src/harness.rs:476-487` resolves the WebDAV directory name with this priority:

1. **`room_fname`** — preferred, used when non-empty
2. **`room_name`** — fallback (the `roomName` URL slug from DDP, or `sender_name` for DMs)

```rust
fn compute_webdav_dir(room_name: &str, room_fname: &str, is_dm: bool) -> String {
    let name = if room_fname.is_empty() {
        room_name   // ← URL slug / internal name, can look like "sen1-lin2-sheng1-tai4"
    } else {
        room_fname  // ← display name, e.g. "森林生態"
    };
    format!("{}-{}", if is_dm { "d" } else { "r" }, name)
}
```

When `fname` is empty, the resulting WebDAV directory uses the URL slug (`room_name`), which
for rooms created without an explicit ASCII slug is indistinguishable from an internal codename.
This is the most common cause of unexpected WebDAV directory names.

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
`args[1].fname` field. When absent, `room_fname` stays empty and downstream
code falls back to `room_name` (the URL slug) or `sender_name` (for DMs) —
identical behavior to a cache miss in the old design.

### No fallback from REST API (yet)

The bot currently does **not** query the RocketChat REST API for room details. If `fname` is
missing from the DDP "changed" event, there is no other source of room display names. A future
improvement could call `GET /api/v1/rooms.info?roomId={id}` at startup or on first message to
retrieve the room's `fname` and cache it.
