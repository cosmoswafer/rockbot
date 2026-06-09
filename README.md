# RockBot

AI-powered RocketChat bot written in Rust. Responds to DMs and @mentions with
agentic capabilities — web search, URL fetching, and image vision — backed
entirely by a NextCloud WebDAV server for persistent state.

## Architecture

```
rockbot/
├── Cargo.toml              # workspace root
├── config.toml             # runtime configuration
├── crates/
│   ├── rocketchat/         # standalone RocketChat client library
│   │   ├── auth            # REST login, session management
│   │   ├── websocket       # WS event stream, reconnection
│   │   └── messages        # parsing, filtering, sending
│   └── rockbot/            # application binary
│       ├── config          # TOML loader + JSON migration
│       ├── provider        # AiProvider trait (OpenRouter, DeepSeek)
│       ├── agent           # agentic loop, tool registry
│       ├── tools           # Exa search, web fetch, vision
│       ├── memory          # in-memory history + archive pipeline
│       └── webdav          # NextCloud WebDAV client
└── _dfds/                  # data flow diagrams
```

### Key design decisions

| Decision | Rationale |
| -------- | --------- |
| **No local disk** | All persistent state lives on NextCloud WebDAV — config backups, memory archives, images |
| **Standalone `rocketchat` crate** | Reusable library with zero application logic — auth, WS stream, message I/O |
| **`AiProvider` trait** | Single interface for OpenRouter and DeepSeek; add new providers by implementing one trait |
| **Per-room memory** | Each channel and DM has isolated in-memory history and its own WebDAV archive directory |
| **Minimal dependencies** | `tokio`, `reqwest`, `serde`, `toml`, `scraper`/`htmd` for HTML→MD — no heavy frameworks |

## Configuration

Configuration uses `config.toml`. On first run, the bot detects a legacy
`config.json` and auto-migrates it.

```toml
[server]
url = "https://chat.example.com"
username = "rockbot"
password = "secret"
use_tls = true

[ai]
provider = "openrouter"        # or "deepseek"
api_key = "sk-or-..."
model = "openai/gpt-4o"
base_url = ""                  # optional override

[webdav]
url = "https://cloud.example.com/remote.php/dav/files/rockbot"
username = "rockbot"
password = "app-password"
root = "/rockbot"

[memory]
max_chars = 50000              # archive trigger threshold
archive_interval = 3600        # seconds between archive checks

[tools]
exa_api_key = "..."
web_fetch = true
vision = true
```

See [config migration DFD](_dfds/config.md) for the JSON→TOML field mapping.

## Build & Run

```bash
cargo build --release
./target/release/rockbot                    # uses ./config.toml
./target/release/rockbot -c /path/to.toml   # custom config path
```

### Prerequisites

- Rust 1.75+ (edition 2021)
- A RocketChat server with WebSocket support enabled
- A NextCloud instance with WebDAV access
- An API key for OpenRouter or DeepSeek
- (Optional) An Exa API key for web search

## Agentic Flow

The bot runs a loop: receive message → build context → query AI → execute tool
calls → feed results back → repeat until final reply.

**Available tools:**

| Tool | Description |
| ---- | ----------- |
| `web_search` | Search the web via Exa API and return ranked results |
| `web_fetch` | Fetch a URL and optionally convert HTML to clean Markdown |
| `vision` | Download an image from WebDAV and send it to the AI provider for analysis |

See [agent orchestration DFD](_dfds/agent.md) for the full loop.

## Memory Management

Each room accumulates conversation history in local memory. When the character
count exceeds `memory.max_chars`, the oldest messages are summarized by the AI
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
| [Config](_dfds/config.md) | 1 | TOML loading and JSON migration |
| [RocketChat](_dfds/rocketchat.md) | 1 | Auth, WebSocket, message filtering |
| [AI Provider](_dfds/ai-provider.md) | 1 | Trait abstraction, vision payloads |
| [Agent](_dfds/agent.md) | 1 | Agentic loop, tool execution |
| [Memory](_dfds/memory.md) | 1 | History, summarization, archival |
| [WebDAV](_dfds/webdav.md) | 1 | NextCloud storage operations |

## Crate: `rocketchat`

Standalone, reusable RocketChat client library. No application logic.

```rust
use rocketchat::{Client, EventStream, MessageFilter};

let client = Client::connect("https://chat.example.com", "user", "pass").await?;
let mut stream = client.event_stream().await?;

while let Some(event) = stream.next().await {
    if MessageFilter::is_dm_or_mention(&event, client.user_id()) {
        client.send_message(room_id, "Hello!").await?;
    }
}
```

Handles authentication, WebSocket reconnection with exponential backoff, ping/pong
keepalive, and message parsing/filtering.

## License

MIT
