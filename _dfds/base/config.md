# Configuration Management

## 1. Purpose

Loads two TOML files at startup — a bundled `default.config.toml` (shipped
with the repo, no secrets) and a user `config.toml` (gitignored, holds
passwords and API keys). The two are deep-merged via Serde's merge strategy
(user values override defaults). The validated `AppConfig` struct is shared
read-only across all subsystems.

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
    DEF_TOML[(default.config.toml\nbundled defaults\nno secrets)]
    USER_TOML[(config.toml\nuser overrides\npasswords + API keys)]
    LOAD_DEF(LoadDefaults)
    LOAD_USR(LoadUserConfig)
    MERGE(MergeConfig\nuser wins)
    VALIDATE(ValidateConfig)
    SHARE(DistributeAppConfig)
    SUBSYS[Subsystems]

    INIT -->|"built-in path"| LOAD_DEF
    DEF_TOML -->|"toml text"| LOAD_DEF
    INIT -->|"CONFIG_FILE env / 'config.toml'"| LOAD_USR
    USER_TOML -->|"toml text"| LOAD_USR
    LOAD_DEF -->|"default appconfig"| MERGE
    LOAD_USR -->|"user appconfig"| MERGE
    MERGE -->|"merged appconfig"| VALIDATE
    VALIDATE -->|"validated appconfig"| SHARE
    SHARE -->|"arc appconfig"| SUBSYS
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    LOAD_DEF(LoadDefaults)
    LOAD_USR(LoadUserConfig)
    ERR_MISSING_DEF[Error: default.config.toml\nnot found – corrupt install]
    ERR_PARSE[Error: TOML Parse]
    ERR_VALID[Error: Validation]
    VALIDATE(ValidateConfig)

    LOAD_DEF -->|"error: file not found"| ERR_MISSING_DEF
    LOAD_DEF -->|"error: parse failure"| ERR_PARSE
    LOAD_USR -->|"error: parse failure"| ERR_PARSE
    LOAD_USR -->|"error: file not found\n(defaults used)"| VALIDATE
    VALIDATE -->|"error: provider not found"| ERR_VALID
```

## 3. Data Structures

#### `AppConfig`

| Field        | Type                         | Notes                                          |
| ------------ | ---------------------------- | ---------------------------------------------- |
| `rocketchat` | `RocketChatSection`          | Server connection + chat model settings        |
| `chat_providers` | `Vec<ProviderConfig>`    | Chat AI provider definitions (array-of-tables) |
| `image_providers`| `Vec<ProviderConfig>`    | Image generation provider definitions          |
| `image_model`    | `ImageModelConfig` (always present via default)| Default image provider + model alias           |
| `webdav`     | `Option<WebDavConfig>`       | NextCloud WebDAV endpoint and credentials      |
| `tools`      | `HashMap<String, ToolServiceConfig>`| Tool-specific API keys (generic map)     |

#### `RocketChatSection`

| Field    | Type           | Notes                                         |
| -------- | -------------- | --------------------------------------------- |
| `server` | `ServerConfig` | RocketChat connection details                 |
| `model`  | `ModelConfig`  | Default provider, model alias, history limits |

#### `ServerConfig`

| Field      | Type     | Notes                                                               |
| ---------- | -------- | ------------------------------------------------------------------- |
| `url`      | `String` | RocketChat server host (no scheme)                                  |
| `username` | `String` | Bot login username (`""` in defaults, filled in user config)        |
| `password` | `String` | Bot login password (`""` in defaults, filled in user config)        |
| `debug`    | `bool`   | Enable verbose DDP frame logging (default `false`)                  |

> The rocketchat crate has its own `ServerConfig` in `crate-rocketchat/src/config.rs`
> with a `use_tls: bool` field (default `true`) instead of `debug`. The rockbot crate's
> `ServerConfig` is for bot-level connections; the rocketchat crate's is per-client TLS
> configuration.

#### `ModelConfig`

| Field                  | Type           | Notes                                                         |
| ---------------------- | -------------- | ------------------------------------------------------------- |
| `default_provider`     | `ProviderName` | Must match a `[[chat_providers]].name`; non-empty validated newtype |
| `default_model`        | `String`       | Model alias key in provider's models map                      |
| `max_history_size`     | `BoundedUsize` | Max conversation turns (default 18); validated 1..=100_000_000 |
| `max_text_length`      | `BoundedUsize` | Layer 1 overflow threshold chars (default 50000); validated 1..=100_000_000 |
| `max_iterations`       | `u32`          | Max agent loop iterations (default 28)                         |
| `max_soul_chars`       | `BoundedUsize` | Layer 3 max chars for soul.md content (default 2000); validated 1..=100_000_000 |
| `memory_ttl_secs`      | `u64`          | Room idle timeout — snapshot to WebDAV then evict (default 300)|
| `persist_interval_secs`| `u64`          | Snapshot persist timer interval (default 60)                  |
| `max_context_bytes`    | `BoundedUsize` | Max byte size for context (default 4MB ≈ 1M tokens). Triggers inline summarization and image-stripping when exceeded. Validated 1..=100_000_000 |
| `max_attachment_bytes` | `u64`          | Max size of a single attachment in bytes (default 25_000_000) |
| `model_context_length` | `u32`          | Model's max context window in tokens (default 131072). 90% threshold triggers background compression after LLM calls when usage nears limit. |

#### `ProviderConfig`

| Field        | Type                     | Notes                                                             |
| ------------ | ------------------------ | ----------------------------------------------------------------- |
| `name`       | `ProviderName`           | Provider identifier ("openrouter", etc.); non-empty validated newtype |
| `api_key`    | `String`                 | Provider API key (`""` in defaults, filled in user config)        |
| `base_url`   | `ConfigUrl`              | API endpoint base URL; non-empty validated newtype                |
| `basecf_url` | `Option<String>`         | Cloudflare worker proxy override; used by Fal as storage/CDN upload URL |
| `chat_path`  | `Option<String>`         | Chat completions path (Default: `/chat/completions`)             |
| `draw_path`  | `Option<String>`         | Image generation path (opt.)                                      |
| `models`     | `HashMap<String, String>`| Alias → model-id map                                              |

> **Note:** `basecf_url` is used by `FalAiProvider` as the `storage_url` for CDN uploads. Chat providers use `base_url` + `chat_path` via `ProviderConfig::chat_url()`.

#### `ToolServiceConfig`

| Field     | Type     | Notes                  |
| --------- | -------- | ---------------------- |
| `api_key` | `String` | Service-specific key   |

#### `ImageModelConfig`

| Field                   | Type     | Notes                                                     |
| ----------------------- | -------- | --------------------------------------------------------- |
| `default_provider`      | `ProviderName` | Must match an `[[image_providers]].name`; non-empty validated newtype |
| `default_text_model`    | `String` | Model alias for text-to-image generation                  |
| `default_edit_model`    | `String` | Model alias for image editing                             |
| `default_quality`       | `String` | Image quality level (default `"medium"`)                  |
| `default_output_format` | `String` | Output image format (default `"png"`)                      |
| `default_num_images`    | `u32`    | Number of images per generation (default 1)                |
| `default_image_size`    | `String` | Target image dimensions (default `"portrait_2_3"`)         |
| `default_image_size_tier` | `String` | Resolution tier `"2K"` or `"4K"` (default `"4K"`)       |

#### `WebDavConfig`

| Field      | Type     | Notes                                   |
| ---------- | -------- | --------------------------------------- |
| `url`      | `DavUrl`  | NextCloud WebDAV endpoint URL; non-empty validated newtype |
| `username` | `String`  | NextCloud username                      |
| `password` | `String`  | NextCloud app password                  |
| `root`     | `DavRoot` | Base directory for bot data; non-empty validated newtype |
| `calendar_name` | `Option<String>` | CalDAV calendar name (enables calendar tool if set) |
| `dav_path`      | `String`         | WebDAV/NextCloud API path prefix (default `"/remote.php/dav"`) |

> **Validated newtypes.** `ProviderName`, `ConfigUrl`, `DavUrl`, `DavRoot`, `NonEmptyString`, and `BoundedUsize`
> are hand-written validated wrappers that enforce invariants at deserialization time
> (config boundary) via custom `Serialize`/`Deserialize` implementations. `ProviderName`, `ConfigUrl`,
> `DavUrl`, `DavRoot`, and `NonEmptyString` require
> non-empty strings. `BoundedUsize` enforces the range `1..=100_000_000`. Holding
> an instance of any of these types guarantees the invariant — no downstream runtime
> checks needed.
>
> **Two-layer input protection** follows the pattern in AGENTS.md:
> - [`serde_valid`](https://crates.io/crates/serde_valid) — format/shape constraints at deserialization
>   boundaries (`min_length`, `max_length`, `pattern`, etc.). Used on `ToolServiceConfig`,
>   `KnowledgeIndex`, and `IndexEntry`.
> - [`validator`](https://crates.io/crates/validator) — business-logic cross-field validation.
>   Used on `AppConfig` via a `#[validate(schema)]` function that verifies `default_provider`
>   references exist in `[[chat_providers]]` and `[[image_providers]]`.
>
> Defined in `crate-rockbot/src/validated.rs` (rockbot types) and
> `crate-webdav/src/validated.rs` + `crate-webdav/src/types.rs` (WebDAV types).

## 4. Config Files

| File                  | Git   | Secrets | Purpose                                    |
| --------------------- | ----- | ------- | ------------------------------------------ |
| `default.config.toml` | Tracked | No   | Bundled defaults (model limits, URLs, empty secrets) |
| `config.toml`         | Ignored | Yes  | User overrides (passwords, API keys)       |

- `default.config.toml` is loaded first from the workspace root (shipped with the repo).
- `config.toml` is loaded second; its path comes from the `CONFIG_FILE` env var (default `"config.toml"`).
- User-provided values deep-merge over defaults. Empty strings in user config override defaults.
- If `config.toml` is missing, the bot runs with only default values (all secrets will be empty — startup may fail validation).
