# Matrix DMs Without Encryption — Investigation Notes

**Date**: 2026-06-17

## Problem

Bot cannot see any messages in Matrix direct messages. A user sends a DM, the bot
stays silent.

## Root Causes (3 issues, all independent)

### 1. No E2EE support compiled in

`crate-rockbot/Cargo.toml` builds `matrix-sdk` with:

```toml
matrix-sdk = { version = "0.18", default-features = false, features = ["markdown"] }
```

The `e2e-encryption` feature is **not** enabled. This means:
- The SDK has no Olm/Megolm support, no crypto store.
- `m.room.encrypted` events cannot be decrypted.
- No event handler is registered for `m.room.encrypted` — these events are
  silently dropped at the SDK level.

Since clients like Element create **encrypted DMs by default**, every DM the
bot receives is an encrypted room. The bot sees nothing.

### 2. No auto-join on room invites (intentional design)

The bot deliberately never auto-joins rooms. `matrix.rs:134` only processes
`RoomState::Joined` rooms; `RoomState::Invited` is silently ignored. There is
no invite event handler, no `client.join_room_by_id()` call.

When a user starts a DM with the bot, the bot receives an invite but never
accepts it — even an unencrypted DM would fail at this step. This is a
conservative design choice: the bot only enters rooms an admin has explicitly
placed it in.

> Workaround: manually accept invites via Element or homeserver admin.

### 3. No mention filter (minor, unrelated to encryption)

The DFD spec (`_dfd/infra/matrix.md` section 2c) says the bot should only
dispatch DMs and @mentions. The code currently dispatches **all** text messages
from **all** joined rooms. This means the bot responds to every message in
every room (not just DMs). This is a spec/code divergence, not a blocker.

## The Full Flow (why DMs are invisible)

```
User (Element) → "Hi bot" (DM)
    → Element creates encrypted room, sets m.room.encryption state
    → Element sends m.room.encrypted event with encrypted body
    → Matrix homeserver forwards to bot's /sync
    → Bot's matrix-sdk receives m.room.encrypted event
    → No handler registered → event silently dropped
    → Bot never sees the message
```

## Workaround for Unencrypted DMs

1. User creates an **unencrypted** DM (in Element: disable encryption toggle
   before sending the first message).
2. On the homeserver side, manually accept the invite (`RoomState::Invited →
   RoomState::Joined`), because the bot does not auto-join.
3. The bot will now see `m.room.message` events (plain text) and can respond.

## Long-term Fix Options

Scored by completeness vs effort:

| # | Approach | Effort | Result |
|---|----------|--------|--------|
| A | Enable `e2e-encryption` feature + add invite auto-join + crypto store setup | ~50-100 LoC | Proper fix; all DMs work regardless of client encryption settings |
| B | Add invite auto-join + encrypted-event fallback reply ("I don't support encryption") | ~30 LoC | Requires users to create unencrypted DMs |
| C | Deploy Pantalaimon proxy between bot and homeserver | Zero code | Extra service to maintain |

## Related DFD Updates

`_dfd/infra/matrix.md` updated to reflect current reality:
- Section 1: E2EE status note (not compiled in)
- Section 2c: mention filter and invite handling marked as spec-not-implemented
- Section 2e: E2EE claim corrected with feature-gate
- Section 4: NFR updated
- Section 5: `matrix-sdk` version corrected to `0.18` with actual feature flags
