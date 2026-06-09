# RockBot

AI-powered RocketChat bot written in Rust. Responds to DMs and @mentions with
agentic capabilities — web search, URL fetching, and image vision — backed
entirely by a NextCloud WebDAV server for persistent state.

## Quick Start

```bash
git clone https://github.com/anomalyco/rockbot.git
cd rockbot
cp example.config.toml config.toml
# edit config.toml with your RocketChat, provider, and WebDAV credentials
cargo build --release
./target/release/rockbot               # uses ./config.toml (full bot)
./target/release/rocketchat            # debug binary (connect + log events)
```

### Prerequisites

- Rust 1.85+ (edition 2024)
- A RocketChat server with WebSocket support enabled
- A NextCloud instance with WebDAV access (optional — bot runs without it)
- An API key for OpenRouter, DeepSeek, or Replicate
- An Exa API key (optional, for web search)

## Architecture

```
rockbot/
├── Cargo.toml                  # workspace root
├── example.config.toml         # config template (copy to config.toml)
├── crate-rocketchat/           # standalone RocketChat DDP WebSocket client
│   ├── src/client.rs           # connection, auth, message send
│   ├── src/ddp.rs              # DDP message construction
│   ├── src/types.rs            # message parsing and filtering
│   └── src/main.rs             # debug binary (connect + log events)
├── crate-rockbot/              # bot library + application binary
│   ├── src/main.rs             # main bot binary
│   ├── src/harness.rs          # agent loop, context building, tool dispatch
│   ├── src/memory.rs           # per-room conversation history, archiving
│   ├── src/config.rs           # TOML config loading
│   ├── src/tool.rs             # Tool trait, ToolRegistry, ToolResult
│   ├── src/tools/              # tool implementations
│   │   ├── web_search.rs       # Exa search API
│   │   ├── web_fetch.rs        # URL fetcher with HTML→Markdown
│   │   └── vision.rs           # image download and analysis
│   ├── src/provider/           # AiProvider trait + implementations
│   │   ├── deepseek.rs         # DeepSeek API client
│   │   ├── openrouter.rs       # OpenRouter API client
│   │   ├── replicate.rs        # Replicate image generation
│   │   └── mod.rs              # trait definition
│   └── src/types.rs            # ChatMessage, ChatRequest, CompletionResult, ToolDef
├── crate-webdav/               # WebDAV client for NextCloud storage
│   └── src/client.rs           # GET, PUT, PROPFIND, MKCOL, DELETE
└── _dfds/                      # data flow diagrams
```

### Key design decisions

| Decision | Rationale |
| -------- | --------- |
| **No local disk** | All persistent state lives on NextCloud WebDAV — config backups, memory archives, images |
| **Standalone `rocketchat` crate** | Reusable library with zero application logic — auth, WS stream, message I/O |
| **`AiProvider` trait** | Single interface for OpenRouter, DeepSeek, and Replicate; add new providers by implementing one trait |
| **TOML array-of-tables config** | `[[providers]]` syntax supports multiple AI backends with per-provider model aliases |
| **Per-room memory** | Each channel and DM has isolated in-memory history and its own WebDAV archive directory |
| **`Tool` trait with `ToolRegistry`** | Tools are dynamically registered; the agent loop queries the provider, executes tool calls, and feeds results back |
| **Minimal dependencies** | `tokio`, `reqwest`, `serde`, `toml`, `async-trait` — no heavy frameworks |

## Configuration

Copy `example.config.toml` to `config.toml` and fill in your credentials. The
config has three sections:

| Section | Purpose |
| ------- | ------- |
| `[rocketchat]` | Server URL, credentials, default AI provider/model, history limits |
| `[[providers]]` | AI backend definitions (OpenRouter, DeepSeek, Replicate) with API keys and model aliases |
| `[webdav]` | NextCloud WebDAV endpoint, credentials, storage root path (optional) |

See `example.config.toml` for the full annotated template.

## Build & Run

```bash
cargo build --release
./target/release/rockbot                     # uses ./config.toml (full agent bot)
./target/release/rockbot -c /path/to.toml    # custom config path
./target/release/rocketchat                  # debug binary (connect + log events)
./target/release/rocketchat -c /path/to.toml
```

Run all tests:
```bash
cargo test                          # unit + integration (no server needed)
cargo test -- --ignored             # real integration tests (needs credentials)
cargo test -p rockbot               # single crate
```

### Environment variables

| Variable | Purpose |
| -------- | ------- |
| `EXA_API_KEY` | API key for the web_search tool (Exa search API) |

## Agentic Flow

The bot runs a loop: receive message → build context → query AI → execute
tool calls → feed results back → repeat until final reply.

### Available tools

| Tool | Description |
| ---- | ----------- |
| `web_search` | Search the web via Exa API and return ranked results |
| `web_fetch` | Fetch a URL and optionally convert HTML to clean Markdown |
| `vision` | Download an image from a URL and report metadata |

### Image generation (via Replicate)

The `ReplicateProvider` supports creating predictions and polling for results.
Image generation tools (`infograph`, `anime`) are planned.

## Memory Management

Each room accumulates conversation history in local memory. When the character
count exceeds a threshold, the oldest messages are summarized by the AI
provider into a `.md` file and archived to the room's WebDAV directory:

```
{webdav.root}/{room_id}/memory/000001_summary.md
```

If WebDAV is unreachable, archiving is deferred and the oldest messages are
truncated as a fallback.

## Crate: `rocketchat`

Standalone, reusable RocketChat client library. Handles authentication, WebSocket
connection, ping/pong keepalive, and message parsing/filtering.

```rust
use rocketchat::{RocketChatClient, MessageSender, IncomingMessage, MessageFilter};

let config = RocketChatConfig::from_file("config.toml")?;
let mut client = RocketChatClient::new(config);
client.connect_and_run(|msg, sender| async move {
    if msg.is_dm {
        sender.reply("Hello!").await.ok();
    }
}).await?;
```

## Crate: `webdav`

Async WebDAV client for NextCloud storage. Supports directory listing, file
upload/download, folder creation, and deletion.

```rust
use webdav::{WebDavConfig, WebDavClient};

let config = WebDavConfig::from_file("config.toml")?;
let client = config.create_client()?;
client.write_file("/rockbot/test.txt", b"Hello!").await?;
let content = client.read_file_to_string("/rockbot/test.txt").await?;
```

## Crate: `rockbot`

Library crate providing the bot's core types, AI provider abstraction, tool
system, memory management, and agent harness. Three providers are implemented
(`DeepSeekProvider`, `OpenRouterProvider`, `ReplicateProvider`). The `AiProvider`
trait makes it straightforward to add new backends.

```rust
use rockbot::provider::{AiProvider, DeepSeekProvider};
use rockbot::harness::AgentHarness;
use rockbot::tool::ToolRegistry;

let config = AppConfig::from_file("config.toml")?;
let provider = DeepSeekProvider::new(&config.find_provider("deepseek").unwrap(), "deepseek-chat")?;
let mut harness = AgentHarness::new(config, Box::new(provider), None);
let reply = harness.process_message("room1", "general", false, "user", "Hello").await?;
```

## Data Flow Diagrams

| Diagram | Level | Description |
| ------- | ----- | ----------- |
| [Context](_dfds/context-diagram.md) | 0 | System boundary and external entities |
| [Agent Loop](_dfds/agent-harness.md) | 1 | Agent event loop, LLM interaction, tool execution, per-room routing |
| [Agent Orchestrator](_dfds/agent-orchestrator.md) | 1 | Top-level wiring: startup sequence, error handling, data structures |
| [Config](_dfds/base/config.md) | 1 | TOML loading and JSON migration |
| [RocketChat](_dfds/rocketchat.md) | 1 | Auth, WebSocket, message filtering |
| [AI Provider](_dfds/base/ai-provider.md) | 1 | Trait abstraction, ChatRequest/CompletionResult data structures |
| [Memory](_dfds/memory.md) | 1 | History, summarization, archival |
| [Knowledge](_dfds/tools/knowledge.md) | 1 | Fact extraction from memory, indexed `.md` storage, context injection |
| [WebDAV](_dfds/tools/webdav.md) | 1 | NextCloud storage operations |

## License

MIT
