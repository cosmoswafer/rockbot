# Config & Provider Boot

## 1. Purpose

After logging is initialized, the bot logs its version (`rockbot v{version}`)
at `INFO` level. The version is embedded at compile time via
`env!("CARGO_PKG_VERSION")`, reading from `Cargo.toml` so it stays in sync
automatically. Config is loaded from `config.toml` (or `CONFIG_FILE` env var)
with embedded Rust defaults (`#[serde(default)]`), deserialized, and validated
into an `AppConfig` instance. The active model config selects which AI provider
to instantiate (DeepSeek / OpenRouter / llama.cpp). WebDAV client creation is
conditional on `[webdav]` config presence.

## 2. Diagram

```mermaid
flowchart TD
    LOG_SETUP(SetupLogging)
    VER_LOG(LogVersion)
    TOML[(Config File)]
    CFG(LoadConfig)
    VALIDATE(ValidateConfig)
    CFG_STORE[(AppConfig)]
    SELECT_PROV(SelectAiProvider)
    DAV_INIT(InitWebDAV)
    DAV[NextCloud WebDAV]
    PROV(AiProvider)

    LOG_SETUP -->|"tracing subscriber"| VER_LOG
    VER_LOG -->|"info! rockbot v{version}"| CFG
    TOML -->|"raw toml"| CFG
    CFG -->|"deserialized config"| VALIDATE
    VALIDATE -->|"appconfig"| CFG_STORE
    CFG_STORE -->|"active model config"| SELECT_PROV
    SELECT_PROV -->|"ai provider instance"| PROV
    CFG_STORE -->|"webdav credentials"| DAV_INIT
    DAV_INIT -->|"webdav client"| DAV
```
