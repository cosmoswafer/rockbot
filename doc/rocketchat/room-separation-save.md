# Room Separation & Room Save

## Room Separation (`registered_rooms`)

An allowlist mechanism that limits which rooms the bot responds in without an `@mention`.

- **Field:** `RocketChatClient::registered_rooms: HashMap<String, bool>` (`client.rs:79`)
- **Register:** `register_room(&mut self, room_name: &str)` inserts a room slug (`client.rs:104-107`)
- **Dispatch gate** (`client.rs:211-215`): a message is dispatched if it's a DM, starts with `@botname`, **or** originates from a registered room slug (even without mention).
- **Effect:** rooms not in the allowlist are ignored unless the bot is explicitly @-mentioned or messaged directly.

## Room Save (`RoomCache`)

An in-memory cache of room metadata populated from the DDP `"rooms"` subscription.

- **Types:** `CachedRoom { room_id, name, fname, t }` (`types.rs:199-204`), stored in `RoomCache` (`types.rs:212-214`)
- **Insert:** `insert_from_added(&mut self, raw: &Value)` parses `"added"` DDP events (`types.rs:223-244`)
- **Lookup:** `get_fname(&self, room_id: &str) -> Option<&str>` resolves the friendly room name when the per-event `args[1].fname` is absent (`types.rs:246-254`)
- **Lifetime:** ephemeral — rebuilt on every connection from the `"rooms"` subscription stream.

## File Locations

| Mechanism | Primary Source |
|---|---|
| Registered rooms (separation) | `src/client.rs:79, 104-107, 211-215` |
| Room cache (save) | `src/types.rs:197-259` |
| DDP `"added"` detection | `src/ddp.rs:131-133` |
| Rooms subscription message | `src/ddp.rs:118-128` |
