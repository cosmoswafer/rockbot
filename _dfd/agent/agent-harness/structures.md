# Agent Harness — Shared Structures

## 1. Overview

The operational environment that wraps the agent loop — the invariant core
cycle of `LLM → tools → LLM → ...`. The harness layers Tools, Knowledge, and
Context around this loop without modifying it.

### 1a. Micro Harness Scope

rockbot implements a **micro harness**: a minimal harness with only the
mechanisms needed for a single-agent, single-channel chatbot. Three of the six
standard harness mechanisms are present:

| Mechanism   | Coverage | Details |
|-------------|----------|---------|
| **Tools**   | Full     | Abstract tool calling via `ToolRegistry` — individual tools each have their own DFD |
| **Context** | Full     | Per-room conversation history buffer, LLM summarization and hard reset (see [Memory Reset](../../memory/memory-reset/post-reply-decision.md)), archive loading — see [Memory Management](../../memory/memory/retrieve-two-layers.md); plus iteration limits, room state routing, system prompt assembly (with live UTC time injection via `now_utc_human()`). Soul (`soul.md`) is **re-read from WebDAV on every message** to ensure multi-instance consistency. |
| **Knowledge** | Full     | `save_knowledge`, `forget_knowledge`, `recall_knowledge`; index summary injected as context, AI uses `recall_knowledge` for on-demand full-entry retrieval — see [Knowledge Management](../../knowledge/knowledge/write.md) |

Intentionally absent — not needed for rockbot's scope:

| Mechanism       | Reason |
|-----------------|--------|
| **Permissions** | Single-user bot — no sandbox or approval flows |
| **Extensions**  | No plugin/hook system — tools are statically registered |
| **Coordination**| Single agent — no subagents, teams, or worktrees |

- Upstream: [Agent Loop](../agent-loop/main-path.md) feeds `IncomingMessage`
  into the loop and consumes `BotReply`
- Downstream: [AI Provider](../../ai/ai-provider/main-path.md) receives `ChatRequest` and returns
  `CompletionResult` with tool calls or final text
- Downstream: [Memory Management](../../memory/memory/retrieve-two-layers.md) provides `ConversationHistory` per
  room and receives new messages for archival
- Downstream: [Knowledge Management](../../knowledge/knowledge/write.md) extracts and persists
  domain facts, loads index summary into agent context on room init
 - Downstream: Individual tools (see `tools/` directory) are registered in
   `ToolRegistry` and invoked by the agent loop via `execute_by_name()`
 - Shared: `ImageCache` (`image_cache.rs`) stores `GeneratedImage` entries keyed by call_id for the image upload pipeline (see [image-sharing.md](./image-sharing.md))

## 3. Data Structures

- **AgentContext** — does not exist as a struct. The harness constructs these values on the fly: `system_prompt` is built by `build_system_prompt()` (which injects the current UTC time via `now_utc_human()` into `{current_utc_time}` in the `DEFAULT_SYSTEM_PROMPT` template), `history` by `build_context()`, `tools` by `ToolRegistry::definitions()`, `room_id` is a method parameter, `webdav_dir` is computed by `compute_webdav_dir()`.

#### `ToolResult`

| Field      | Type     | Notes                                      |
| ---------- | -------- | ------------------------------------------ |
| `call_id`  | `NonEmptyString` | Matches `ToolCall.id`; validated in factory methods |
| `name`     | `NonEmptyString` | Tool name                                  |
| `content`  | `String` | Result text (returned to LLM as tool msg)  |
| `is_error` | `bool`   | True if tool execution failed              |

#### `ToolRegistry`

| Field      | Type                    | Notes                          |
| ---------- | ----------------------- | ------------------------------ |
| `tools`    | `HashMap<String, Box<dyn Tool>>` | Name → implementation |

#### `ToolDef`

| Field           | Type            | Notes                                   |
| --------------- | --------------- | --------------------------------------- |
| `tool_type`     | `String`        | Always `"function"`                     |
| `function`      | `FunctionDef`   | Nested function definition object       |

#### `FunctionDef`

| Field        | Type            | Notes                                   |
| ------------ | --------------- | --------------------------------------- |
| `name`       | `String`        | Function name                           |
| `description`| `Option<String>`| Human-readable description for the LLM  |
| `parameters` | `Option<Value>` | JSON Schema for arguments               |
| `strict`     | `Option<bool>`  | Whether to enforce strict schema        |

#### `GeneratedImage` (ImageCache Entry)

Stored in `Arc<Mutex<HashMap<String, GeneratedImage>>>` keyed by tool call_id.

| Field          | Type           | Description                                   |
| -------------- | -------------- | --------------------------------------------- |
| `webdav_path`  | `NonEmptyString` | WebDAV path where the image was persisted; validated at construction |
| `image_bytes`  | `Vec<u8>`       | Raw image bytes for fallback data URI         |
| `mime_type`    | `NonEmptyString` | MIME type, e.g. `image/png`; validated at construction |
| `share_url`    | `Option<string>`| NextCloud public share link (7-day expiry)    |

#### Registered Tools

Tools are registered at startup via `ToolRegistry::register()`. Each tool
implements the `Tool` trait (`name`, `description`, `parameters`, `execute`).
The registry exposes `definitions()` for the LLM and dispatches calls via
`execute_by_name()`. See individual tool DFDs under `tools/` for each tool's
implementation.
