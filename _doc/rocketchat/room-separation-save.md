# Room Separation & Room Save

## Room Separation (`registered_rooms`)

An allowlist mechanism that limits which rooms the bot responds in without an `@mention`.

- **Field:** `RocketChatClient::registered_rooms: HashMap<String, bool>` (`client.rs:79`)
- **Register:** `register_room(&mut self, room_name: &str)` inserts a room slug (`client.rs:104-107`)
- **Dispatch gate** (`client.rs:211-215`): a message is dispatched if it's a DM, starts with `@botname`, **or** originates from a registered room slug (even without mention).
- **Effect:** rooms not in the allowlist are ignored unless the bot is explicitly @-mentioned or messaged directly.

## Room Save (`RoomCache`) — Removed 2026-06-10

The `RoomCache` / `CachedRoom` mechanism was removed because the RocketChat
server at `rc.tokyofy.top` doesn't respond to the `"rooms"` DDP subscription.
See [`_docs/rocketchat/room-name-fields.md`](../../_docs/rocketchat/room-name-fields.md)
for details. `room_fname` is now sourced from the per-event `args[1].fname`
first, with a REST API fallback (`rooms.info`/`rooms.get`) when `fname` is
absent from the DDP message (wired in `crate-rockbot/src/main.rs:417-451`).

## File Locations

| Mechanism | Primary Source |
|---|---|
| Registered rooms (separation) | `src/client.rs:79, 104-107, 211-215` |
| Room cache (save) | `src/types.rs:197-259` |
| DDP `"added"` detection | `src/ddp.rs:131-133` |
| Rooms subscription message | `src/ddp.rs:118-128` |
