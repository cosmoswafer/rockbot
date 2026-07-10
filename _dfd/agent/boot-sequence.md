# Boot Sequence

## 1. Purpose

Covers the startup sequence from `main()` entry to entering the agent loop.
Includes version logging, config loading, AI provider selection, WebDAV
initialization, tool registry construction (with about-info logging at `INFO`
level), image provider setup, platform client creation, and maintenance timer
start.

- Downstream: [Agent Loop](agent-loop.md) — enters the `AgentLoop` event loop
  after boot completes
- References: [Configuration Management](../infra/config.md) — provides
  `AppConfig` used throughout boot
- References: [AI Provider](../ai/ai-provider.md) — `AiProvider` trait
  implemented by DeepSeek, OpenRouter, llama.cpp

## 2. Diagram

### 2a. Config & Provider Boot

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

After logging is initialized, the bot logs its version (`rockbot v{version}`)
at `INFO` level. The version is embedded at compile time via
`env!("CARGO_PKG_VERSION")`, reading from `Cargo.toml` so it stays in sync
automatically. Config is loaded from `config.toml` (or `CONFIG_FILE` env var)
with embedded Rust defaults (`#[serde(default)]`), deserialized, and validated
into an `AppConfig` instance. The active model config selects which AI provider
to instantiate (DeepSeek / OpenRouter / llama.cpp). WebDAV client creation is
conditional on `[webdav]` config presence.

### 2b. Tool Registration

```mermaid
flowchart TD
    CFG_STORE[(AppConfig)]
    DAV[(NextCloud WebDAV)]
    HARNESS[(AgentHarness)]
    REG_TOOLS(InitToolRegistry)
    RESOLVE_IMG(ResolveImageModels)
    IMG_PROV(InitImageProvider)
    TOOLS[(ToolRegistry)]
    ATTACH(AttachTools)
    RESET_REG(RegisterResetMemory)

    CFG_STORE -->|"tools + image model config"| REG_TOOLS
    DAV -->|"webdav client"| REG_TOOLS
    CFG_STORE -->|"image model config"| RESOLVE_IMG
    RESOLVE_IMG -->|"resolved model IDs"| IMG_PROV
    IMG_PROV -->|"image providers"| REG_TOOLS
    REG_TOOLS -->|"registered tools"| TOOLS
    TOOLS -->|"attach tools"| ATTACH
    HARNESS -->|"harness"| ATTACH
    ATTACH -->|"harness with tools"| HARNESS
    HARNESS -->|"harness lock"| RESET_REG
    RESET_REG -->|"register reset_memory"| HARNESS
```

Tool registration is the core of boot. Every tool is registered conditionally
based on available config (search provider config for web search, WebDAV for
persistent tools, image provider for image_gen). Each registration and
model-resolution step emits an **about-info** log line (see §4).

Tools registered unconditionally:
- `WebFetchTool` (variant depends on Exa + WebDAV availability)
- `VisionTool`

Tools registered conditionally on config:
- `WebSearchTool` (requires configured search provider in `[search]` section)

Tools registered when WebDAV is configured:
- `WebDavTool`, `EditSoulTool`, `SaveKnowledgeTool`,
  `ForgetKnowledgeTool`, `RecallKnowledgeTool`
- `CalendarTool` (if `[webdav]` calendar settings present)
- `ImageGenTool` (if an `image_provider` config entry exists — uses
  `FalAiProvider` or `OpenRouterImageProvider` internally, with model
  aliases resolved via `resolve_image_model()`)

After all tools are registered, they are attached to `AgentHarness`. The
`ResetMemoryTool` is registered last (requires harness lock access).

### 2c. Platform Connect & Enter Loop

```mermaid
flowchart TD
    CFG_STORE[(AppConfig)]
    SELECT_PLAT(SelectPlatform)
    BOT_ID[bot_id<br/>from platform.bot_id]
    HARNESS[(AgentHarness)]
    CONNECT(ConnectPlatform)
    TIMER(StartMaintenance)
    PLATFORM[Messaging Platform]
    LOOP[AgentLoop]

    CFG_STORE -->|"platform.name"| SELECT_PLAT
    SELECT_PLAT -->|"messaging client"| CONNECT
    SELECT_PLAT -->|"bot_id()"| BOT_ID
    CFG_STORE -->|"config + bot_id"| HARNESS
    BOT_ID -->|"bot_id param"| HARNESS
    HARNESS -->|"maintenance timer"| TIMER
    TIMER -->|"periodic flush every Ns"| HARNESS
    CONNECT -->|"auth + subscribe"| PLATFORM
    CONNECT -->|"connected"| LOOP
    PLATFORM -->|"incoming messages"| LOOP
    LOOP -->|"bot replies"| PLATFORM
```

Platform is selected by `[platform] name` (rocketchat or matrix). A
`MessagingClient` is instantiated **before** the `AgentHarness` so its
`bot_id()` trait method can supply the per-bot identifier the harness needs
for WebDAV snapshot path isolation (issue #58 — the harness no longer derives
`bot_id` from `config.platform.name` itself). The connect+run loop then
begins. The maintenance timer is started just before entering the connection
loop, running every `persist_interval_secs` to flush dirty room snapshots to
WebDAV and evict stale rooms.

## 3. Data Structures

All boot-time data structures are defined in their respective subsystem DFDs.
Boot is a wiring layer — it does not define new data types.

| Structure | Defined In |
| --- | --- |
| `AppConfig` | [Configuration Management](../infra/config.md) §3 |
| `AiProvider` trait | [AI Provider](../ai/ai-provider.md) §3 |
| `ImageProvider` trait | [Image Generation](../tools/image-gen.md) §3 |
| `ToolRegistry` | [Agent Loop](agent-loop.md) §3 (`ToolRegistry` data store) |
| `AgentHarness` | [Agent Loop](agent-loop.md) §3 |
| `MessagingClient` trait | [Agent Loop](agent-loop.md) §3 |

## 4. Non-Functional Requirements

**About-info at default log level**: All about-info messages emitted during
boot — version log, tool registration variant, image model resolution,
WebDAV/tool status, `WebFetchTool` support mode, calendar registration status —
are logged at `INFO` level. These are visible without `RUST_LOG=debug`. Only
WebSocket/DDP wire traffic, memory/secret debugging internals, and
per-invocation tool execution traces require `DEBUG`.

**Config-only startup**: The application only reads `config.toml` (with
embedded Rust defaults via `#[serde(default)]`) at startup. No other local
files are read or created.

**Fail-fast parse boundaries**: Config deserialization and validation
occur at the boundary. If config is malformed or a required provider is
missing, the boot terminates with an error log and exit code 1.
