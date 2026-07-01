# Config files

Four bot instances, each driven by a separate `CONFIG_FILE`:

| File | Platform | User | Provider(s) | WebDAV root | Context |
|------|----------|------|-------------|-------------|---------|
| `config.toml` (default) | Matrix | zeroquokka | openrouter, deepseek, llamacpp | clawspaces | 32k |
| `config-localfalcon.toml` | Matrix | threefalcon | deepseek, llamacpp | CLAW | 64k |
| `config-localshark.toml` | Matrix | oneshark | llamacpp | CLAW | 28k |
| `config-atom.toml` | RocketChat | rockai | deepseek, openrouter | clawspaces | 64k |

All use the Matrix homeserver (`mtx.tokyofy.top`) except `config-atom.toml` which uses RocketChat (`rc.tokyofy.top`). The atom instance normally runs on a separate machine; it's only kept here for debugging.

## Shared WebDAV root is intentional (localfalcon + localshark)

localfalcon and localshark **intentionally** share WebDAV root `CLAW`. This is not a misconfiguration — it is the design for running two bots with **different LLMs** (a smarter one and a faster one) that present as **one shared identity** to the same DM user. Both DMs resolve to the same `d-{name}` WebDAV directory (e.g. `d-DTI` for DMs with `@dti:tokyofy.matrix`), so they read/write the same `soul.md` and `summary.md`. **Snapshot data is isolated per bot** — see below.

Consequences and constraints of this design:

- **One soul, two brains.** `soul.md` is shared; either bot's `edit_soul` updates the shared identity. Per-bot identity (e.g. a bot's own name) must not live in the shared `soul.md` unless both bots should present that same name.
- **Sync is pull-based, not real-time.** Each bot re-reads `soul.md` from WebDAV on every incoming message (`harness.rs:261`), so staleness is bounded by the inter-message gap in the other bot's room. There is no background polling, file watch, or cross-instance push. If one room is idle, that bot keeps the last-read soul until its next message. See issue #46.
- **No write coordination.** `edit_soul` does an unconditional PUT (last-write-wins, no ETag/If-Match). Concurrent edits from both bots can silently lose a write.
- **`state_dir` must differ per instance** even when the WebDAV root is shared. localfalcon uses `./tmp/matrix-sdk-falcon`, localshark uses `./tmp/matrix-sdk-shark` (Matrix SDK session store must not collide).
- **Snapshot isolation** (`snapshot.json`): each bot instance writes its own snapshot to `{root}/{snapshot_prefix}/{bot_id}/{wd}/snapshot.json` (default prefix `.snapshots`). Example:
  ```
  CLAW/.snapshots/threefalcon/d-DTI/snapshot.json    ← falcon's history only
  CLAW/.snapshots/oneshark/d-DTI/snapshot.json       ← shark's history only
  ```
  This eliminates the snapshot clobbering race documented in issue #46 / #49.

## Restart procedure

**"restart"** — all three regular instances (default + two locals):
```bash
pkill rockbot 2>/dev/null
CONFIG_FILE=config.toml             nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
CONFIG_FILE=config-localfalcon.toml nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
CONFIG_FILE=config-localshark.toml  nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
pgrep -ax rockbot
```

**"restart three bots"** — same as "restart" (config.toml + localfalcon + localshark):
```bash
pkill rockbot 2>/dev/null
CONFIG_FILE=config.toml             nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
CONFIG_FILE=config-localfalcon.toml nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
CONFIG_FILE=config-localshark.toml  nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
pgrep -ax rockbot
```

**"restart bot"** — only the default `config.toml` (zeroquokka/Matrix) instance:
```bash
pkill rockbot 2>/dev/null
CONFIG_FILE=config.toml nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
pgrep -ax rockbot
```

**"restart all"** — all four including atom (debug):
```bash
pkill rockbot 2>/dev/null
CONFIG_FILE=config.toml             nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
CONFIG_FILE=config-localfalcon.toml nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
CONFIG_FILE=config-localshark.toml  nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
CONFIG_FILE=config-atom.toml        nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
pgrep -ax rockbot
```
