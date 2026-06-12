# Configuration Management

## 1. Purpose

Configuration is split into two conceptual groups that map to two TOML files:

- **Group 1 — Model Config** (`default.config.toml`, tracked in git): Provider
  definitions (URLs, paths, model alias maps), behavioral limits, and infrastructure
  defaults. Contains no secrets.
- **Group 2 — Secrets + Model Preferences** (`config.toml`, gitignored): API keys,
  passwords, WebDAV credentials, and optional model-preference overrides (which
  provider/model the user wants to use).

The two files are deep-merged at startup (user values override defaults).
The validated `AppConfig` struct is shared read-only across all subsystems.

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
    MDL_CFG[(default.config.toml\nGroup 1 — Model Config\nproviders + limits + defaults)]
    SEC_CFG[(config.toml\nGroup 2 — Secrets +\nModel Preferences)]
    LOAD_DEF(LoadModelConfig)
    LOAD_USR(LoadUserConfig)
    MERGE(MergeConfig\nuser wins)
    VALIDATE(ValidateConfig)
    SHARE(DistributeAppConfig)
    SUBSYS[Subsystems]

    INIT -->|"built-in path"| LOAD_DEF
    MDL_CFG -->|"provider defs + limits"| LOAD_DEF
    INIT -->|"CONFIG_FILE env / 'config.toml'"| LOAD_USR
    SEC_CFG -->|"api keys + creds + overrides"| LOAD_USR
    LOAD_DEF -->|"model config"| MERGE
    LOAD_USR -->|"secrets + prefs"| MERGE
    MERGE -->|"merged appconfig"| VALIDATE
    VALIDATE -->|"validated appconfig"| SHARE
    SHARE -->|"arc appconfig"| SUBSYS
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    LOAD_DEF(LoadModelConfig)
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

### 2c. Two-Group Config Layout

```mermaid
flowchart LR
    subgraph G1["Group 1 — Model Config (default.config.toml)"]
        PVDEF[[[chat_providers]\nprovider defs\nURLs + model maps\n(api_key = \"\")]]
        IMGDEF[[[image_providers]\nprovider defs\nURLs + model maps\n(api_key = \"\")]]
        MDL_DEF[[rocketchat.model]\nbehavioral limits\nmax_history, max_text,\nmemory_ttl, etc.]]
        IMG_DEF[[image_model]\nimage defaults\nprovider, quality,\nsize, format, etc.]]
        WD_DEF[["[webdav]\ndav_path only"]]
    end

    subgraph G2["Group 2 — Secrets + Model Preferences (config.toml)"]
        PVKEY["[[chat_providers]]\napi_key = \"sk-...\""]
        IMGKEY["[[image_providers]]\napi_key = \"...\""]
        RCSRV["[rocketchat.server]\nurl + username + password"]
        RCMDLS["[rocketchat.model]\n(default_provider +\ndefault_model overrides)"]
        IMGMDLS["[image_model]\n(default_provider +\ndefault_text_model overrides)"]
        WDAV["[webdav]\nurl + username + password\n+ root + calendar_name"]
        EXA["[tools.exa]\napi_key"]
    end

    G1 -->|"deep-merge\n(user wins)"| MERGED[Merged AppConfig]
    G2 -->|"deep-merge\n(user wins)"| MERGED
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
| `debug`    | `bool`   | Enable verbose DDP logging                                          |

#### `ModelConfig`

| Field                  | Type    | Notes                                                         |
| ---------------------- | ------- | ------------------------------------------------------------- |
| `default_provider`     | `String`| Must match a `[[chat_providers]].name`                        |
| `default_model`        | `String`| Model alias key in provider's models map                      |
| `max_history_size`     | `usize` | Max conversation turns (default 18)                           |
| `max_text_length`      | `usize` | Layer 1 overflow threshold chars (default 50000)              |
| `max_iterations`       | `u32`   | Max agent loop iterations (default 28)                        |
| `max_summary_chars`    | `usize` | Layer 2 max chars across loaded summaries (default 8000)      |
| `max_soul_chars`       | `usize` | Layer 3 max chars for soul.md content (default 2000)          |
| `summary_days`         | `u32`   | Layer 2 retention window in days (default 7)                  |
| `memory_ttl_secs`      | `u64`   | Room idle timeout — snapshot to WebDAV then evict (default 300)|
| `persist_interval_secs`| `u64`   | Snapshot persist timer interval (default 60)                  |
| `max_context_bytes`    | `usize` | Max byte size for image-stripping trigger (default 30MB)      |
| `max_attachment_bytes` | `u64`   | Max size of a single attachment in bytes (default 25_000_000) |

#### `ProviderConfig`

| Field        | Type                     | Notes                                                             |
| ------------ | ------------------------ | ----------------------------------------------------------------- |
| `name`       | `String`                 | Provider identifier ("openrouter", etc.)                          |
| `api_key`    | `String`                 | Provider API key (`""` in defaults, filled in user config)        |
| `base_url`   | `String`                 | API endpoint base URL                                             |
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
| `default_provider`      | `String` | Must match an `[[image_providers]].name`                   |
| `default_text_model`    | `String` | Model alias for text-to-image generation                  |
| `default_edit_model`    | `String` | Model alias for image editing                             |
| `default_quality`       | `String` | Image quality level (default `"medium"`)                   |
| `default_output_format` | `String` | Output image format (default `"png"`)                      |
| `default_num_images`    | `u32`    | Number of images per generation (default 1)                |
| `default_image_size`    | `String` | Image aspect ratio / size preset (default `"portrait_2_3"`)|
| `default_image_size_tier`|`String`  | Output resolution tier (default `"4K"`)                    |

#### `WebDavConfig`

| Field      | Type     | Notes                                   |
| ---------- | -------- | --------------------------------------- |
| `url`      | `String` | NextCloud WebDAV endpoint URL           |
| `username` | `String` | NextCloud username                      |
| `password` | `String` | NextCloud app password                  |
| `root`     | `String` | Base directory for bot data             |
| `calendar_name` | `Option<String>` | CalDAV calendar name (enables calendar tool if set) |
| `dav_path`      | `String`         | WebDAV/NextCloud API path prefix (default `"/remote.php/dav"`) |

## 4. Config Files

Two files, two groups:

| File                  | Git   | Group                           | Content                                            |
| --------------------- | ----- | ------------------------------- | -------------------------------------------------- |
| `default.config.toml` | Tracked | Group 1 — Model Config         | Provider defs (URLs, models, empty api_keys), behavioral limits, infrastructure defaults |
| `config.toml`         | Ignored | Group 2 — Secrets + Preferences| API keys, server/WenDAV credentials, optional model-preference overrides |

- `default.config.toml` is loaded first from the workspace root (shipped with the repo). It defines *what* providers and models exist, and *default* behavioral limits.
- `config.toml` is loaded second; its path comes from the `CONFIG_FILE` env var (default `"config.toml"`). It supplies *secrets* (api_key, password) and *preferences* (which provider/model to actually use).
- Both files can contain any TOML key; deep-merge resolves conflicts (user wins). Array-of-table entries (`[[chat_providers]]`, `[[image_providers]]`) are merged by matching `name`.
- If `config.toml` is missing, the bot runs with only default values (all secrets will be empty — startup may fail validation).
- The `example.config.toml` in the repo serves as a template for user `config.toml` — it shows all secret fields a user must fill in, plus commented-out model-preference overrides.
