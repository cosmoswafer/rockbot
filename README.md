# RockBot

AI-powered RocketChat + Matrix bot written in Rust. Responds to DMs and @mentions
with agentic capabilities — web search, URL fetching, image vision, image generation,
calendar/todo management, knowledge storage, and file operations — backed
by a NextCloud WebDAV server for persistent state.

## User Stories

> **Priority**: P0 = critical, P1 = core, P2 = enhancement
> Full details: [`_doc/scenarios/`](_doc/scenarios/)

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

Test inventory and run instructions: [`_doc/test_suite/`](_doc/test_suite/).

## Reference docs

### Constraints & user stories
| Document | Description |
| -------- | ----------- |
| [`_doc/scenarios/README.md`](_doc/scenarios/README.md) | Directory index |
| [`_doc/scenarios/top-10-user-stories.md`](_doc/scenarios/top-10-user-stories.md) | Top-level user-facing features |
| [`_doc/scenarios/image-generation-user-stories.md`](_doc/scenarios/image-generation-user-stories.md) | Image pipeline: 5 detailed stories |
| [`_doc/scenarios/non-functional-requirements.md`](_doc/scenarios/non-functional-requirements.md) | Quality attributes |

### DFDs
| Component | DFD | Detailed notes |
| --------- | --- | -------------- |
| Context diagram | [`_dfd/context-diagram.md`](_dfd/context-diagram.md) | — |
| Agent loop | [`_dfd/agent/agent-loop.md`](_dfd/agent/agent-loop.md) | — |
| Agent harness | [`_dfd/agent/agent-harness.md`](_dfd/agent/agent-harness.md) | [`_doc/agent-harness.md`](_doc/agent-harness.md) |
| Boot sequence | [`_dfd/agent/boot-sequence.md`](_dfd/agent/boot-sequence.md) | — |
| Config | [`_dfd/infra/config.md`](_dfd/infra/config.md) | — |
| RocketChat client | [`_dfd/infra/rocketchat.md`](_dfd/infra/rocketchat.md) | [`_doc/rocketchat/rocketchat-client.md`](_doc/rocketchat/rocketchat-client.md) |
| RocketChat REST | [`_dfd/infra/rocketchat-rest.md`](_dfd/infra/rocketchat-rest.md) | — |
| Matrix client | [`_dfd/infra/matrix.md`](_dfd/infra/matrix.md) | — |
| AI Provider | [`_dfd/ai/ai-provider.md`](_dfd/ai/ai-provider.md) | — |
| Memory | [`_dfd/memory/memory.md`](_dfd/memory/memory.md) | — |
| Memory reset | [`_dfd/memory/memory-reset.md`](_dfd/memory/memory-reset.md) | — |
| Knowledge | [`_dfd/knowledge/knowledge.md`](_dfd/knowledge/knowledge.md) | — |
| Knowledge priority | [`_dfd/knowledge/knowledge-priority.md`](_dfd/knowledge/knowledge-priority.md) | — |
| Image interception | [`_dfd/interception/image-interception.md`](_dfd/interception/image-interception.md) | — |
| Secret interception | [`_dfd/interception/secret-interception.md`](_dfd/interception/secret-interception.md) | — |
| Web search | [`_dfd/tools/search-web.md`](_dfd/tools/search-web.md) | Exa / Brave multi-provider |
| Web fetch | [`_dfd/tools/web-fetch.md`](_dfd/tools/web-fetch.md) | — |
| Image gen tool | [`_dfd/tools/image-gen.md`](_dfd/tools/image-gen.md) | — |
| Vision tool | [`_dfd/tools/vision.md`](_dfd/tools/vision.md) | — |
| Calendar tool | [`_dfd/tools/calendar.md`](_dfd/tools/calendar.md) | — |
| WebDAV tool | [`_dfd/tools/webdav.md`](_dfd/tools/webdav.md) | — |
| Reset memory | [`_dfd/tools/reset-memory.md`](_dfd/tools/reset-memory.md) | — |
| Edit soul | [`_dfd/tools/edit-soul.md`](_dfd/tools/edit-soul.md) | — |
| Knowledge tools | [`_dfd/tools/knowledge.md`](_dfd/tools/knowledge.md) | — |
| Test suite | — | [`_doc/test_suite/running.md`](_doc/test_suite/running.md) |

## Environment variables

| Variable | Purpose |
| -------- | ------- |
| `CONFIG_FILE` | Config path (default: `config.toml`) |

## License

MIT
