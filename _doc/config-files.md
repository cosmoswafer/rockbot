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

```bash
# 1. Kill all running instances
pkill rockbot 2>/dev/null

# 2. Start each instance in background (no local logs)
CONFIG_FILE=config.toml             nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
CONFIG_FILE=config-local.toml       nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
CONFIG_FILE=config-localfalcon.toml nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &
CONFIG_FILE=config-localshark.toml  nohup ./target/release/rockbot < /dev/null > /dev/null 2>&1 &

# 3. Verify
pgrep -ax rockbot
```
