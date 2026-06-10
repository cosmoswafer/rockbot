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
