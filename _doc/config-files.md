# Config files

Four bot instances, each driven by a separate `CONFIG_FILE`:

| File | Platform | User | Provider(s) | WebDAV root | Context |
|------|----------|------|-------------|-------------|---------|
| `config.toml` | RocketChat | rockai | deepseek, openrouter | clawspaces | 64k |
| `config-local.toml` | Matrix | zeroquokka | openrouter, deepseek, llamacpp | clawspaces | 32k |
| `config-localfalcon.toml` | Matrix | threefalcon | deepseek, llamacpp | CLAW | 64k |
| `config-localshark.toml` | Matrix | oneshark | llamacpp | CLAW | 28k |

All use the same homeserver (`mtx.tokyofy.top`) except `config.toml` which uses RocketChat (`rc.tokyofy.top`).

## Restart procedure

**"restart"** — all four instances:
```bash
pkill rockbot 2>/dev/null
CONFIG_FILE=config.toml             nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
CONFIG_FILE=config-local.toml       nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
CONFIG_FILE=config-localfalcon.toml nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
CONFIG_FILE=config-localshark.toml  nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
pgrep -ax rockbot
```

**"restart three bots"** — only the three `config-local*` instances (excludes vanilla `config.toml`):
```bash
pkill rockbot 2>/dev/null
CONFIG_FILE=config-local.toml       nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
CONFIG_FILE=config-localfalcon.toml nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
CONFIG_FILE=config-localshark.toml  nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
pgrep -ax rockbot
```

**"restart local bot"** — only the `config-local.toml` instance:
```bash
pkill rockbot 2>/dev/null
CONFIG_FILE=config-local.toml nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
pgrep -ax rockbot
```
