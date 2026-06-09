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
./target/release/rocketchat          # uses ./config.toml
./target/release/rocketchat -c /path/to.toml
```

### Prerequisites

- Rust 1.85+ (edition 2024)
- A RocketChat server with WebSocket support enabled
- A NextCloud instance with WebDAV access
- An API key for OpenRouter, DeepSeek, or Replicate

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
├── crate-rockbot/              # bot library (no binary yet)
│   ├── src/config.rs           # TOML config loading
│   ├── src/provider/           # AiProvider trait + implementations
│   │   ├── openrouter.rs       # OpenRouter API client
│   │   ├── deepseek.rs         # DeepSeek API client
│   │   └── mod.rs              # trait definition
│   └── src/types.rs            # ChatMessage, ChatRequest, CompletionResult, ToolDef
├── crate-webdav/               # WebDAV client for NextCloud storage
│   └── src/client.rs           # GET, PUT, PROPFIND, MKCOL, DELETE
└── _dfds/                      # data flow diagrams (planned architecture)
```

### Key design decisions

| Decision | Rationale |
| -------- | --------- |
| **No local disk** | All persistent state lives on NextCloud WebDAV — config backups, memory archives, images |
| **Standalone `rocketchat` crate** | Reusable library with zero application logic — auth, WS stream, message I/O |
| **`AiProvider` trait** | Single interface for OpenRouter, DeepSeek, and Replicate; add new providers by implementing one trait |
| **TOML array-of-tables config** | `[[providers]]` syntax supports multiple AI backends with per-provider model aliases |
| **Per-room memory** | Each channel and DM has isolated in-memory history and its own WebDAV archive directory |
| **Minimal dependencies** | `tokio`, `reqwest`, `serde`, `toml`, `async-trait` — no heavy frameworks |

## Configuration

Copy `example.config.toml` to `config.toml` and fill in your credentials. The
config has four sections:

| Section | Purpose |
| ------- | ------- |
| `[rocketchat]` | Server URL, credentials, default AI provider/model, history limits |
| `[[providers]]` | AI backend definitions (OpenRouter, DeepSeek, Replicate) with API keys and model aliases |
| `[webdav]` | NextCloud WebDAV endpoint, credentials, storage root path |

See `example.config.toml` for the full annotated template.

## Build & Run

```bash
cargo build --release
./target/release/rocketchat                     # uses ./config.toml
./target/release/rocketchat -c /path/to.toml    # custom config path
```

Run all tests:
```bash
cargo test                          # unit + integration (no server needed)
cargo test -- --ignored             # real integration tests (needs credentials)
cargo test -p rocketchat            # single crate
```

The `rocketchat` debug binary connects to a RocketChat server and logs incoming
events. It also responds to built-in commands (`!ping`, `!echo`, `!help`). The
full agentic bot binary (`rockbot`) is not yet implemented in Rust.

## Agentic Flow (planned)

The bot will run a loop: receive message → build context → query AI → execute
tool calls → feed results back → repeat until final reply.

**Planned tools:**

| Tool | Description |
| ---- | ----------- |
| `web_search` | Search the web via Exa API and return ranked results |
| `web_fetch` | Fetch a URL and optionally convert HTML to clean Markdown |
| `vision` | Download an image from WebDAV and send it to the AI provider for analysis |
| `infograph` | Generate an infographic image from a text prompt, stored on WebDAV |
| `anime` | Generate a Japanese anime-style image from a text prompt, stored on WebDAV |

See [agent loop DFD](_dfds/agent-harness.md) for the full loop design.

## Memory Management (planned)

Each room accumulates conversation history in local memory. When the character
count exceeds a threshold, the oldest messages are summarized by the AI
provider into a `.md` file and archived to the room's WebDAV directory:

```
{webdav.root}/{room_id}/memory/000001_summary.md
```

Archives are loaded back on startup to seed context. If WebDAV is unreachable,
archiving is deferred and the oldest messages are truncated as a fallback.

See [memory management DFD](_dfds/memory.md) for the full pipeline.

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

## Crate: `rocketchat`

Standalone, reusable RocketChat client library. Handles authentication, WebSocket
reconnection with exponential backoff, ping/pong keepalive, and message
parsing/filtering.

```rust
use rocketchat::{RocketChatClient, MessageSender, IncomingMessage, MessageFilter};

let mut client = RocketChatClient::connect(config).await?;
let user_id = client.user_id().to_string();

while let Some(msg) = client.next_message().await? {
    if msg.is_dm_or_mention(&user_id) {
        client.send_message(&msg.room_id, "Hello!").await?;
    }
}
```

## Crate: `webdav`

Async WebDAV client for NextCloud storage. Supports directory listing, file
upload/download, folder creation, and deletion.

```rust
use webdav::WebDavClient;

let client = WebDavClient::new(config)?;
client.put("/rockbot/notes/hello.txt", b"Hello, WebDAV!").await?;
let entries = client.list("/rockbot/notes/").await?;
```

## Crate: `rockbot`

Library crate providing the bot's core types and AI provider abstraction. Two
providers are implemented (`OpenRouterProvider`, `DeepSeekProvider`), with a
third (`ReplicateProvider`) for image generation planned. The `AiProvider` trait
makes it straightforward to add new backends.

```rust
use rockbot::provider::{AiProvider, DeepSeekProvider};
use rockbot::types::ChatRequest;

let provider = DeepSeekProvider::from_config(&config)?;
let result = provider.complete(ChatRequest {
    messages: vec![/* ... */],
    ..Default::default()
}).await?;
```

## License

MIT
