# Remove display name from message filter

## Files to edit

### 1. `crate-rocketchat/src/client.rs`

**Remove field** (line 117):
```rust
// Before:
    bot_name: String,
    display_name: Option<String>,
    registered_rooms: HashMap<String, bool>,
// After:
    bot_name: String,
    registered_rooms: HashMap<String, bool>,
```

**Remove from constructor** (line 131):
```rust
// Before:
            bot_name,
            display_name: None,
            registered_rooms: HashMap::new(),
// After:
            bot_name,
            registered_rooms: HashMap::new(),
```

**Remove set_display_name method** (lines 136-138):
Remove:
```rust
    pub fn set_display_name(&mut self, name: Option<String>) {
        self.display_name = name.map(|n| crate::types::strip_emoji(&n));
    }
```

**Remove display_match from event loop** (lines 243-245, 250):
```rust
// Before (lines 243-253):
                    let display_match = self.display_name.as_ref().is_some_and(|dn| {
                        !dn.is_empty() && msg.text.to_lowercase().contains(&dn.to_lowercase())
                    });
                    let should_dispatch = msg.is_dm
                        || (!msg.room_name.is_empty()
                            && (msg.text.starts_with(&bot_name)
                                || msg.text.contains(&bot_name)
                                || display_match))
                        || (!registered_rooms.is_empty()
                            && !msg.room_name.is_empty()
                            && registered_rooms.contains_key(&msg.room_name));

// After:
                    let should_dispatch = msg.is_dm
                        || (!msg.room_name.is_empty()
                            && (msg.text.starts_with(&bot_name)
                                || msg.text.contains(&bot_name)))
                        || (!registered_rooms.is_empty()
                            && !msg.room_name.is_empty()
                            && registered_rooms.contains_key(&msg.room_name));
```

### 2. `crate-rockbot/src/platform/rocketchat.rs`

**Remove field** (line 12):
```rust
// Before:
    pub config: rocketchat::RocketChatConfig,
    pub bot_name: String,
    pub display_name: Option<String>,
// After:
    pub config: rocketchat::RocketChatConfig,
    pub bot_name: String,
```

**Remove from new()** (lines 16-26):
```rust
// Before:
    pub fn new(
        config: rocketchat::RocketChatConfig,
        bot_name: String,
        display_name: Option<String>,
    ) -> Self {
        Self {
            config,
            bot_name,
            display_name,
        }
    }
// After:
    pub fn new(
        config: rocketchat::RocketChatConfig,
        bot_name: String,
    ) -> Self {
        Self {
            config,
            bot_name,
        }
    }
```

**Remove set_display_name call** (line 112):
```rust
// Before:
        let mut client = rocketchat::RocketChatClient::new(self.config.clone());
        client.set_display_name(self.display_name.clone());
// After:
        let mut client = rocketchat::RocketChatClient::new(self.config.clone());
```

### 3. `crate-rockbot/src/main.rs`

**Remove display_name fetch and update new() call** (lines 336-337):
```rust
// Before:
                let display_name = h.memory().any_display_name();
                Box::new(RocketChatPlatform::new(rc_config, bot_name.clone(), display_name))
// After:
                Box::new(RocketChatPlatform::new(rc_config, bot_name.clone()))
```

### 4. `crate-rocketchat/src/types.rs`

**Remove display_name from is_dm_or_mention** (lines 207-216):
```rust
// Before:
    pub fn is_dm_or_mention(
        msg: &IncomingMessage, bot_name: &str, registered_rooms: &HashMap<String, bool>,
        display_name: Option<&str>,
    ) -> bool {
        msg.is_dm || (!msg.room_name.is_empty()
            && (msg.text.starts_with(bot_name)
                || msg.text.contains(bot_name)
                || display_name.is_some_and(|dn| msg.text.contains(dn))))
            || (!registered_rooms.is_empty() && registered_rooms.contains_key(&msg.room_name))
    }
// After:
    pub fn is_dm_or_mention(
        msg: &IncomingMessage, bot_name: &str, registered_rooms: &HashMap<String, bool>,
    ) -> bool {
        msg.is_dm || (!msg.room_name.is_empty()
            && (msg.text.starts_with(bot_name)
                || msg.text.contains(bot_name)))
            || (!registered_rooms.is_empty() && registered_rooms.contains_key(&msg.room_name))
    }
```

**Remove 3 test functions** (lines 428-516):
Remove `test_is_dm_or_mention_with_display_name`, `test_is_dm_or_mention_display_name_no_match`, `test_is_dm_or_mention_display_name_none`.

### 5. `crate-rocketchat/tests/integration.rs`

Update all `is_dm_or_mention` calls to remove the `None` display_name argument. There are 6 calls:
- Line 185: `MessageFilter::is_dm_or_mention(&msg, bot_name, &rooms, None)` → `MessageFilter::is_dm_or_mention(&msg, bot_name, &rooms)`
- Line 202: same pattern
- Line 467: same pattern
- Line 485: same pattern
- Line 503: same pattern
- Line 528: same pattern

### 6. `_dfd/infra/rocketchat.md`

- Section 2c (Message Filter Deep Dive): Remove step 4 about self-display name matching, update the 5-stage list to 4 stages, remove display name from mermaid flowchart
- Section 3 `RocketChatClient` table: Remove `display_name` field entry (lines 543-549)

## Verification

```bash
cargo build --release && cargo test
```
