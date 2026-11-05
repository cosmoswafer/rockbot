# RockBot

AI-powered RocketChat + Matrix bot written in Rust. Responds to DMs and @mentions
with agentic capabilities — web search, URL fetching, image vision, image generation,
calendar/todo management, knowledge storage, and file operations — backed
by a NextCloud WebDAV server for persistent state.

## User Stories

> **Priority**: P0 = critical, P1 = core, P2 = enhancement
> Full details: [`docs/scenarios/`](docs/scenarios/)

1. **DM Conversation** (P0) — DMs bot, gets AI replies with per-room conversation history across sessions.
2. **@Mention & Display Name in Channels** (P0) — `@rockbot` or display name triggers reply in-thread; per-room isolation.
3. **Web Search via Exa** (P1) — Returns summarized results with URLs, dates, snippets (auto/fast/deep modes).
4. **HTTP Fetch & Web Requests** (P1) — Full HTTP client (all methods, custom headers, JSON body); returns raw/markdown/JSON.
5. **Image Generation & Editing** (P2) — Text-to-image and image-to-image via fal.ai or OpenRouter; saves to WebDAV, returns share link.
6. **Image Attachment → Conversation Memory** (P2) — Pasted/dropped RocketChat images intercepted, encoded as `data:` URIs, stored in per-room conversation history; visible to vision LLMs, available to `image_gen` for editing.
7. **Vision LLM Sees Images** (P2) — Vision-capable providers (OpenRouter) receive multipart image content; LLM describes, analyzes, and edits images it sees in memory.
8. **Vision Tool Image Fetch** (P2) — Downloads image from public URL into memory; vision LLMs see it, `image_gen` can edit it.
9. **Calendar Events & Todos (CalDAV)** (P2) — Creates/lists/updates/deletes events and todos; per-room calendars, ICS generation, recurrence (rrule), reminders.
10. **Knowledge Persistence** (P2) — Save/recall/forget facts with categories (skill/secret/note), tags, priorities (P0-P3), `when_useful` hints; automatic contextual recall.
11. **Permanent Soul Memory** (P1) — Sets identity, preferences, facts via `edit_soul`; persists to WebDAV, syncs display name, survives restarts.
12. **WebDAV File Operations** (P1) — Reads/writes/lists/edits/deletes files; per-room scoped paths; images returned as base64 markdown.

## Quick Start

```bash
cp example.config.toml config.toml
# edit config.toml with your RocketChat, provider, and WebDAV credentials
cargo build --release
./target/release/rockbot
```

### Install to user home

Installs the `rockbot` binary to `~/.cargo/bin/` (add to `PATH` if not already):

```bash
cargo install --path crate-rockbot
```

## Architecture

```mermaid
flowchart TB
    subgraph Workspace["rockbot workspace"]
        direction TB
        subgraph rb["crate-rockbot"]
            App["main.rs<br/>binary entry"]
            AH["harness.rs<br/>agent loop"]
            Mem["memory.rs<br/>history + archive"]
            AP["provider/<br/>AiProvider"]
            TS["tool.rs + tools/<br/>Tool registry"]
            Cfg["config.rs<br/>TOML config"]
        end
        RC["crate-rocketchat<br/>DDP WebSocket client"]
        WD["crate-webdav<br/>WebDAV client"]

        RC --> rb
        WD --> rb
    end

    RCS["RocketChat Server"] <--> RC
    MX["Matrix Homeserver"] <-->|HTTP| rb
    NC["NextCloud WebDAV"] <--> WD
    LLM["LLM APIs<br/>DeepSeek / OpenRouter / Fal"] <-->|HTTP| AP
```

Three crates: `rocketchat` (DDP WebSocket client), `rockbot` (bot logic + Matrix HTTP client), `webdav` (NextCloud storage).

### Key design decisions

- **No local disk** — all persistent state on NextCloud WebDAV
- **`AiProvider` trait** — single OpenAI-compatible interface; separate `[[chat_providers]]` and `[[image_providers]]` tables for text and image backends (DeepSeek, OpenRouter, Fal)
- **`Tool` trait with `ToolRegistry`** — tools registered dynamically; agent loop dispatches and feeds results back
- **Two-layer memory** — chat history (short-term) and soul archive (long-term) on WebDAV per room; hard reset on overflow (no LLM summarization)
- **Knowledge store** — persistent skill/secret/note entries per room with WebDAV-backed `index.json`

## Prerequisites

- Rust 1.93+ (edition 2024)
- RocketChat server with WebSocket and/or Matrix homeserver
- NextCloud WebDAV (optional — bot runs without it)
- API key for DeepSeek, OpenRouter, or Fal
- Exa API key (optional, for web search/fetch)

## Configuration

Copy `example.config.toml` to `config.toml`. Config path is set via `CONFIG_FILE`
env var (defaults to `config.toml`, not a CLI argument).

See [`example.config.toml`](example.config.toml) for the annotated template.

## Build & test

```bash
cargo build --release                # workspace build (3 crates)
cargo test                           # unit + mock integration tests
cargo test -- --ignored              # real integration tests (needs credentials)
```

Test inventory and run instructions: [`docs/test_suite/`](docs/test_suite/).

## Reference docs

### Constraints & user stories
| Document | Description |
| -------- | ----------- |
| [`docs/scenarios/README.md`](docs/scenarios/README.md) | Directory index |
| [`docs/scenarios/top-10-user-stories.md`](docs/scenarios/top-10-user-stories.md) | Top-level user-facing features |
| [`docs/scenarios/image-generation-user-stories.md`](docs/scenarios/image-generation-user-stories.md) | Image pipeline: 5 detailed stories |
| [`docs/scenarios/non-functional-requirements.md`](docs/scenarios/non-functional-requirements.md) | Quality attributes |

### Design reference (archived DFDs)
| Component | DFD | Detailed notes |
| --------- | --- | -------------- |
| Context diagram | [`docs/dfd/context-diagram.md`](docs/dfd/context-diagram.md) | — |
| Agent loop | [`docs/dfd/agent/agent-loop/main-path.md`](docs/dfd/agent/agent-loop/main-path.md) | — |
| Agent harness | [`docs/dfd/agent/agent-harness/agent-loop.md`](docs/dfd/agent/agent-harness/agent-loop.md) | [`docs/agent-harness.md`](docs/agent-harness.md) |
| Boot sequence | [`docs/dfd/agent/boot-sequence/platform-connect.md`](docs/dfd/agent/boot-sequence/platform-connect.md) | — |
| Config | [`docs/dfd/infra/config/main-path.md`](docs/dfd/infra/config/main-path.md) | — |
| RocketChat client | [`docs/dfd/infra/rocketchat/main-path.md`](docs/dfd/infra/rocketchat/main-path.md) | [`docs/rocketchat/rocketchat-client.md`](docs/rocketchat/rocketchat-client.md) |
| RocketChat REST | [`docs/dfd/infra/rocketchat-rest/rest-alias-send.md`](docs/dfd/infra/rocketchat-rest/rest-alias-send.md) | — |
| Matrix client | [`docs/dfd/infra/matrix/main-path.md`](docs/dfd/infra/matrix/main-path.md) | — |
| AI Provider | [`docs/dfd/ai/ai-provider/main-path.md`](docs/dfd/ai/ai-provider/main-path.md) | — |
| Memory | [`docs/dfd/memory/memory/retrieve-two-layers.md`](docs/dfd/memory/memory/retrieve-two-layers.md) | — |
| Memory reset | [`docs/dfd/memory/memory-reset/post-reply-decision.md`](docs/dfd/memory/memory-reset/post-reply-decision.md) | — |
| Knowledge | [`docs/dfd/knowledge/knowledge/write.md`](docs/dfd/knowledge/knowledge/write.md) | — |
| Knowledge priority | [`docs/dfd/knowledge/knowledge-priority/priority-state.md`](docs/dfd/knowledge/knowledge-priority/priority-state.md) | — |
| Image interception | [`docs/dfd/interception/image-interception/complete-pipeline.md`](docs/dfd/interception/image-interception/complete-pipeline.md) | — |
| Secret interception | [`docs/dfd/interception/secret-interception/uuidv5-scoped-injection.md`](docs/dfd/interception/secret-interception/uuidv5-scoped-injection.md) | — |
| Web search | [`docs/dfd/tools/search-web/main-path.md`](docs/dfd/tools/search-web/main-path.md) | Exa / Brave multi-provider |
| Web fetch | [`docs/dfd/tools/web-fetch/main-path.md`](docs/dfd/tools/web-fetch/main-path.md) | — |
| Image gen tool | [`docs/dfd/tools/image-gen/main-path.md`](docs/dfd/tools/image-gen/main-path.md) | — |
| Vision tool | [`docs/dfd/tools/vision/main-path.md`](docs/dfd/tools/vision/main-path.md) | — |
| Calendar tool | [`docs/dfd/tools/calendar/main-path.md`](docs/dfd/tools/calendar/main-path.md) | — |
| WebDAV tool | [`docs/dfd/tools/webdav/main-path.md`](docs/dfd/tools/webdav/main-path.md) | — |
| Reset memory | [`docs/dfd/tools/reset-memory/flag-driven.md`](docs/dfd/tools/reset-memory/flag-driven.md) | — |
| Edit soul | [`docs/dfd/tools/edit-soul/main-path.md`](docs/dfd/tools/edit-soul/main-path.md) | — |
| Knowledge tools | [`docs/dfd/tools/knowledge/save.md`](docs/dfd/tools/knowledge/save.md) | — |
| Test suite | — | [`docs/test_suite/running.md`](docs/test_suite/running.md) |

## Environment variables

| Variable | Purpose |
| -------- | ------- |
| `CONFIG_FILE` | Config path (default: `config.toml`) |

## License

MIT
