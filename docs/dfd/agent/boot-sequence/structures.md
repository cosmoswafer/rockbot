# Boot Sequence — Shared Structures

## 1. Overview

Covers the startup sequence from `main()` entry to entering the agent loop.
Includes version logging, config loading, AI provider selection, WebDAV
initialization, tool registry construction (with about-info logging at `INFO`
level), image provider setup, platform client creation, and maintenance timer
start.

- Downstream: [Agent Loop](../agent-loop/main-path.md) — enters the
  `AgentLoop` event loop after boot completes
- References: [Configuration Management](../../infra/config/main-path.md) —
  provides `AppConfig` used throughout boot
- References: [AI Provider](../../ai/ai-provider/main-path.md) — `AiProvider`
  trait implemented by DeepSeek, OpenRouter, llama.cpp

## 3. Data Structures

All boot-time data structures are defined in their respective subsystem DFDs.
Boot is a wiring layer — it does not define new data types.

| Structure | Defined In |
| --- | --- |
| `AppConfig` | [Configuration Management](../../infra/config/main-path.md) §3 |
| `AiProvider` trait | [AI Provider](../../ai/ai-provider/main-path.md) §3 |
| `ImageProvider` trait | [Image Generation](../../tools/image-gen/main-path.md) §3 |
| `ToolRegistry` | [Agent Loop](../agent-loop/main-path.md) §3 (`ToolRegistry` data store) |
| `AgentHarness` | [Agent Loop](../agent-loop/main-path.md) §3 |
| `MessagingClient` trait | [Agent Loop](../agent-loop/main-path.md) §3 |

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
