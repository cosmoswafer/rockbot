# Configuration Management

## 1. Purpose

Loads `config.toml` at startup with typed deserialization via `serde`. The
validated `AppConfig` struct is shared read-only across all subsystems.
Supports `from_file` (with validation) and `from_str` (raw parse, no
validation) entry points.

- Downstream: [WebDAV Tool](../tools/webdav.md) consumes `WebDavConfig` for remote file
  access
- Downstream: [RocketChat Connection](rocketchat.md), [AI Provider](ai-provider.md),
  [Memory Management](memory.md) and [Tools](tools/) each consume their respective
  config slices

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    INIT(Initialize)
    TOML_FILE[(Config File)]
    LOAD(LoadToml)
    VALIDATE(ValidateConfig)
    SHARE(DistributeAppConfig)
    SUBSYS[Subsystems]

    INIT -->|"config path"| LOAD
    TOML_FILE -->|"toml text"| LOAD
    LOAD -->|"appconfig struct"| VALIDATE
    VALIDATE -->|"validated appconfig"| SHARE
    SHARE -->|"arc appconfig"| SUBSYS
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    LOAD(LoadToml)
    ERR_NO_CFG[Error: No Config Found]
    ERR_PARSE[Error: TOML Parse]
    ERR_VALID[Error: Validation]
    VALIDATE(ValidateConfig)

    LOAD -->|"error: file not found"| ERR_NO_CFG
    LOAD -->|"error: parse failure"| ERR_PARSE
    VALIDATE -->|"error: provider not found"| ERR_VALID
```

## 3. Data Structures

#### `AppConfig`

| Field        | Type                         | Notes                                          |
| ------------ | ---------------------------- | ---------------------------------------------- |
| `rocketchat` | `RocketChatSection`          | Server connection + chat model settings        |
| `chat_providers` | `Vec<ProviderConfig>`    | Chat AI provider definitions (array-of-tables) |
| `image_providers`| `Vec<ProviderConfig>`    | Image generation provider definitions          |
| `image_model`    | `Option<ImageModelConfig>`| Default image provider + model alias           |
| `webdav`     | `Option<WebDavConfig>`       | NextCloud WebDAV endpoint and credentials      |
| `tools`      | `HashMap<String, ToolSvcCfg>`| Tool-specific API keys (generic map)           |

#### `RocketChatSection`

| Field    | Type           | Notes                                         |
| -------- | -------------- | --------------------------------------------- |
| `server` | `ServerConfig` | RocketChat connection details                 |
| `model`  | `ModelConfig`  | Default provider, model alias, history limits |

#### `ServerConfig`

| Field      | Type     | Notes                              |
| ---------- | -------- | ---------------------------------- |
| `url`      | `String` | RocketChat server host (no scheme) |
| `username` | `String` | Bot login username                 |
| `password` | `String` | Bot login password                 |
| `debug`    | `bool`   | Enable verbose DDP logging         |

#### `ModelConfig`

| Field              | Type     | Notes                                    |
| ------------------ | -------- | ---------------------------------------- |
| `default_provider` | `String` | Must match a [[providers]].name          |
| `default_model`    | `String` | Model alias key in provider's models map |
| `max_history_size` | `usize`  | Max conversation turns (default 12)      |
| `max_text_length`  | `usize`  | Layer 1 overflow threshold chars (default 50000)|
| `max_iterations`   | `u32`    | Max agent loop iterations (default 8)    |
| `max_summary_chars` | `usize`  | Layer 2 max chars across loaded summaries (default 8000)|
| `max_soul_chars`   | `usize`  | Layer 3 max chars for soul.md content (default 2000)|
| `summary_days`     | `u32`    | Layer 2 retention window in days (default 7)|
| `memory_ttl_secs`  | `u64`    | Room idle timeout — snapshot to WebDAV then evict (default 300)|
| `persist_interval_secs` | `u64` | Snapshot persist timer interval (default 60) |

#### `ProviderConfig`

| Field        | Type                     | Notes                                      |
| ------------ | ------------------------ | ------------------------------------------ |
| `name`       | `String`                 | Provider identifier ("openrouter", etc.)   |
| `api_key`    | `String`                 | Provider API key                           |
| `base_url`   | `String`                 | API endpoint base URL                      |
| `basecf_url` | `Option<String>`         | Cloudflare worker proxy override (opt.)    |
| `chat_path`  | `Option<String>`         | Chat completions path (Default: `/chat/completions`)|
| `draw_path`  | `Option<String>`         | Image generation path (opt.)               |
| `models`     | `HashMap<String, String>`| Alias → model-id map                       |

> **Note:** `basecf_url` is deserialized but currently unused — both providers use `base_url` + `chat_path` via `ProviderConfig::chat_url()`.

#### `ToolServiceConfig`

| Field     | Type     | Notes                  |
| --------- | -------- | ---------------------- |
| `api_key` | `String` | Service-specific key   |

#### `WebDavConfig`

| Field      | Type     | Notes                                   |
| ---------- | -------- | --------------------------------------- |
| `url`      | `String` | NextCloud WebDAV endpoint URL           |
| `username` | `String` | NextCloud username                      |
| `password` | `String` | NextCloud app password                  |
| `root`     | `String` | Base directory for bot data             |
| `calendar_name` | `Option<String>` | CalDAV calendar name (enables calendar tool if set) |
