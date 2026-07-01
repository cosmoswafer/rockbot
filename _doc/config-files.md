# Config files

Four bot instances, each driven by a separate `CONFIG_FILE`:

| File | Platform | User | Provider(s) | WebDAV root | Context |
|------|----------|------|-------------|-------------|---------|
| `config.toml` (default) | Matrix | zeroquokka | openrouter, deepseek, llamacpp | clawspaces | 32k |
| `config-localfalcon.toml` | Matrix | threefalcon | deepseek, llamacpp | CLAW | 64k |
| `config-localshark.toml` | Matrix | oneshark | llamacpp | CLAW | 28k |
| `config-atom.toml` | RocketChat | rockai | deepseek, openrouter | clawspaces | 64k |

All use the Matrix homeserver (`mtx.tokyofy.top`) except `config-atom.toml` which uses RocketChat (`rc.tokyofy.top`). The atom instance normally runs on a separate machine; it's only kept here for debugging.

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
