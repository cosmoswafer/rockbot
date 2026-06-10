# Top 10 User Stories — RockBot

> **Priority scale**: P0 = critical path (must ship), P1 = core value (should ship), P2 = enhancement (nice to have)

---

## 1. Direct Message Conversation

| Field | Value |
|-------|-------|
| **Actor** | Team member |
| **Story** | As a team member, I want to DM the bot and receive AI responses, so that I can get answers, research, and task help in a private 1:1 chat. |
| **Priority** | P0 |

**Acceptance Criteria**
- GIVEN I send a DM to the bot, WHEN the bot receives it, THEN it replies with a coherent AI-generated response within the same DM thread.
- GIVEN I send multiple messages in a DM, WHEN the bot responds, THEN it references the conversation history (not just the last message).
- GIVEN the bot has been idle in my DM for hours, WHEN I send a new message, THEN the bot remembers the ongoing conversation context.

**Tech notes**
- DM events: `_dfds/base/rocketchat.md` 2a (StreamEvents → FilterMentionOrDM → DispatchMessage)
- Room init: WebDAV directory `d-{sender_name}` (`rocketchat.md` 2f)
- Context inject: Layer 1 (chat history) + Layer 2 (summaries) + Layer 3 (soul) (`_dfds/base/memory.md` 2a)
- Reply delivery: DDP `sendMessage` method (`rocketchat.md` 3 — `BotReply`)

---

## 2. @Mention in Channels

| Field | Value |
|-------|-------|
| **Actor** | Channel member |
| **Story** | As a channel member, I want to `@rockbot` in a channel and get a response, so that the bot participates in group discussions without leaving the room. |
| **Priority** | P0 |

**Acceptance Criteria**
- GIVEN I post a message starting with `@rockbot` in a channel, WHEN the bot processes it, THEN it replies in the same channel with the `@rockbot` prefix stripped from the original message context.
- GIVEN multiple channels each have separate conversations with the bot, WHEN the bot responds in each, THEN each channel's history is isolated (no cross-channel context leakage).
- GIVEN I do not `@rockbot` in a channel, WHEN the bot sees my message, THEN it does not respond.

**Tech notes**
- Filter: `_dfds/base/rocketchat.md` 2c (Stage 2 — `msg.starts_with("@botname") AND room_name != ""`)
- Room key: `r-{room_fname}` or `r-{room_name}` (prefer friendly name over slug) (`rocketchat.md` 3)
- Isolation: per-room `HashMap<String, RoomState>` with independent history (`_dfds/agent-harness.md` 2f)

---

## 3. Web Search via Exa

| Field | Value |
|-------|-------|
| **Actor** | User |
| **Story** | As a user, I want to ask the bot to search the web and get summarized results, so that I can research topics without leaving RocketChat. |
| **Priority** | P1 |

**Acceptance Criteria**
- GIVEN I ask the bot to search for a topic, WHEN the bot executes the search, THEN it returns a concise summary with titles, URLs, and relevant snippets.
- GIVEN the Exa API key is not configured, WHEN the bot receives a search request, THEN it replies that the search tool is unavailable (not crash).
- GIVEN the Exa API returns an error, WHEN the search fails, THEN the bot reports the failure gracefully without crashing the agent loop.

**Tech notes**
- Tool: `web_search` → `POST https://api.exa.ai/search` (`_dfds/tools/exa-search.md` 2a)
- Mode: highlights by default (token-efficient) (`exa-search.md` 2d)
- Results: default 5, capped at 10,000 chars per URL highlight (`exa-search.md` 3)
- Error: 429/5xx retry with backoff, 401/403 immediate fail (`exa-search.md` 2b)

---

## 4. URL Fetch and Read

| Field | Value |
|-------|-------|
| **Actor** | User |
| **Story** | As a user, I want to share a URL and have the bot fetch, summarize, or convert it to markdown, so that I can consume web content without leaving chat. |
| **Priority** | P1 |

**Acceptance Criteria**
- GIVEN I share a URL with the bot, WHEN I request a fetch, THEN the bot returns the content in the requested format (raw, markdown, or structured JSON).
- GIVEN the URL returns HTML content and I request markdown format, WHEN the bot fetches it, THEN the HTML is converted to markdown for AI-friendly consumption.
- GIVEN the URL is unreachable or returns a 4xx/5xx, WHEN the fetch times out (30s) or fails, THEN the bot reports the HTTP status or timeout error.

**Tech notes**
- Tool: `web_fetch` with `url`, `format`, `verify` params (`_dfds/tools/web-fetch.md` 3)
- Format modes: `raw` (passthrough), `markdown` (HTML→MD), `json` (structured metadata) (`web-fetch.md` 2a)
- Verify: optional parallel Exa search on extracted title (`web-fetch.md` 2c)
- Timeout: 30s (`web-fetch.md` 2b)
- Truncation: 10,000 chars in `json` mode (`web-fetch.md` 3)

---

## 5. Image Generation with fal.ai

| Field | Value |
|-------|-------|
| **Actor** | User |
| **Story** | As a user, I want to ask the bot to generate an image from a text prompt, so that I can create visuals for presentations, concepts, or fun. |
| **Priority** | P2 |

**Acceptance Criteria**
- GIVEN I send a text prompt asking for an image, WHEN the bot generates it, THEN it returns the WebDAV URL of the generated image.
- GIVEN the image is generated successfully, WHEN the bot saves it, THEN the image is stored in the room's WebDAV `images/` directory and accessible via the returned URL.
- GIVEN the image provider is not configured in `config.toml`, WHEN the bot receives a generation request, THEN it replies that the tool is unavailable.

**Tech notes**
- Tool: `image_gen` (`_dfds/agent-harness.md` 3 — Registered Tools)
- Pipeline: submit to fal.ai queue → poll until COMPLETED → download → PUT to WebDAV (`agent-harness.md` 2e)
- Config: requires `[[image_providers]]` (fal or openrouter with `draw_path`) + `[image_model]` (`example.config.toml`)
- Conditional registration: `ImageGenTool` registered only if `image_provider` entry exists (`AGENTS.md`)

---

## 6. Calendar Event Management (CalDAV)

| Field | Value |
|-------|-------|
| **Actor** | User |
| **Story** | As a user, I want to create, list, update, and delete calendar events via chat, so that I can manage my schedule without leaving RocketChat. |
| **Priority** | P2 |

**Acceptance Criteria**
- GIVEN I ask the bot to list events for a date range, WHEN the bot queries CalDAV, THEN it returns event titles, times, and descriptions from the shared calendar.
- GIVEN I ask the bot to create an event with title, date, and optional description, WHEN the bot creates it, THEN the event is stored on the NextCloud calendar.
- GIVEN I ask the bot to update an existing event, WHEN the bot PUTs the changes, THEN it uses ETag-based optimistic concurrency and reports success.
- GIVEN `calendar_name` is not set in config, WHEN the bot receives a calendar request, THEN it replies that the calendar tool is unavailable.

**Tech notes**
- Tool: `calendar` with `action` (list/get/create/update/delete) + event params (`_dfds/tools/calendar.md` 3)
- Protocol: CalDAV (RFC 4791) over NextCloud at `/remote.php/dav/calendars/{user}/{calendar}/` (`calendar.md` 4)
- Format: iCalendar VEVENT (RFC 5545) with optional VALARM (`calendar.md` 3)
- Scope: global (shared across all rooms) (`calendar.md` 1)

---

## 7. Knowledge Persistence (Save / Recall / Forget)

| Field | Value |
|-------|-------|
| **Actor** | User |
| **Story** | As a user, I want to tell the bot to remember a fact, skill, or secret and recall it later, so that the bot builds persistent domain knowledge per room. |
| **Priority** | P2 |

**Acceptance Criteria**
- GIVEN I tell the bot to remember something, WHEN the bot invokes `save_knowledge`, THEN the entry is persisted as a `.md` file on WebDAV and indexed in `index.json`.
- GIVEN I ask the bot to recall what it knows about a topic, WHEN the bot searches the knowledge index, THEN it returns matching entries ranked by relevance.
- GIVEN I tell the bot to forget a previously saved entry, WHEN the bot invokes `forget_knowledge`, THEN the `.md` file is deleted and the index entry is removed (idempotent — no error if already gone).
- GIVEN the same room interacts with the bot again after a restart, WHEN the room is initialized, THEN matching knowledge entries are loaded and injected into context.

**Tech notes**
- Tools: `save_knowledge` / `recall_knowledge` / `forget_knowledge` (`_dfds/base/knowledge.md` 1)
- Categories: `skill` (procedural), `secret` (credential), `note` (factual) with `when_useful` triggers (`knowledge.md` 1)
- Storage: `{webdav_dir}/knowledge/{category}_{slug}.md` + `index.json` (`knowledge.md` 3 — File Layout)
- Retrieval: keyword overlap scoring against recent conversation (`knowledge.md` 2e)
- Always-on: no separate config toggle — enabled when WebDAV is configured (`knowledge.md` 1)
- Status: code not yet implemented (`AGENTS.md` — DFD mapping, `*(planned)*`)

---

## 8. Permanent Soul Memory (Layer 3)

| Field | Value |
|-------|-------|
| **Actor** | User |
| **Story** | As a user, I want to tell the bot my preferences and have it remember them permanently, so that the bot's personality and behavior adapt to my needs over time. |
| **Priority** | P1 |

**Acceptance Criteria**
- GIVEN I tell the bot "remember I prefer short answers", WHEN the bot responds, THEN it invokes `edit_soul` to persist the preference and adapts future replies.
- GIVEN I tell the bot to update or remove a previously saved preference, WHEN the bot processes the request, THEN the `soul.md` file on WebDAV is updated accordingly.
- GIVEN the bot restarts, WHEN it loads a room's context, THEN the soul content is restored from WebDAV and injected into every interaction.

**Tech notes**
- Tool: `edit_soul` with actions `append` / `replace` / `delete_section` (`_dfds/base/memory.md` 2d)
- Storage: `{webdav_dir}/memory/soul.md` on WebDAV (`memory.md` 3)
- Layer: Layer 3 (long-term persistent), truncated to `max_soul_chars` (default 2000) (`memory.md` 1)
- Injection: loaded before Layer 2 and Layer 1 on every context build (`memory.md` 5)
- Snapshot: marked dirty on soul change, coalesced write on next timer (`memory.md` 2c)

---

## 9. WebDAV File Operations

| Field | Value |
|-------|-------|
| **Actor** | User |
| **Story** | As a user, I want to read, write, list, and manage files in the bot's WebDAV workspace, so that I can store and retrieve shared documents through the chat. |
| **Priority** | P1 |

**Acceptance Criteria**
- GIVEN I ask the bot to list files in my room's workspace, WHEN the bot does a PROPFIND, THEN it returns file names, sizes, and last-modified dates.
- GIVEN I ask the bot to read a specific file, WHEN the bot does a GET, THEN it returns the file contents.
- GIVEN I ask the bot to write a file to a deep path, WHEN the parent directories don't exist, THEN the bot creates them via mkcol fallback and writes the file.
- GIVEN I ask the bot to edit a file with oldString/newString replacement, WHEN the file exists, THEN the bot performs the replacement on WebDAV content.

**Tech notes**
- Tool: `webdav` with actions `read` / `write` / `edit` / `list` / `mkdir` / `delete` / `exists` (`_dfds/tools/webdav.md` 2a)
- Room isolation: `{root}/{webdav_dir}/` subdirectories: `memory/`, `images/`, `workspace/` (`webdav.md` 2d)
- Write fallback: AutoMkcol header → 404 → mkcol parents → plain PUT retry (`webdav.md` 2c)
- Conditional registration: `WebDavTool` registered only if `[webdav]` config section present (`AGENTS.md`)

---

## 10. Vision / Image Analysis

| Field | Value |
|-------|-------|
| **Actor** | User |
| **Story** | As a user, I want to share an image URL and have the bot analyze it, so that I can get descriptions, metadata, or insights about images. |
| **Priority** | P2 |

**Acceptance Criteria**
- GIVEN I share an image URL with a descriptive prompt, WHEN the bot runs the vision tool, THEN it returns metadata (MIME type, dimensions if available).
- GIVEN the image URL is unreachable, WHEN the bot tries to download it, THEN it reports the error without crashing.
- GIVEN the image URL points to an unsupported format, WHEN the bot inspects it, THEN it reports the detected format and that analysis is limited.

**Tech notes**
- Tool: `vision` with `url` + `prompt` params (`_dfds/agent-harness.md` 3 — Registered Tools)
- MIME detection: png, jpg, jpeg, gif (tested in unit tests) (`_docs/test_suite/README.md` — vision.rs)
- Note: true vision (sending image data to AI provider) is planned, not yet implemented (`agent-harness.md` 3)

---

## Traceability Matrix

| # | Story | Primary DFD | Supporting DFDs | Config Required | Status |
|---|-------|-------------|-----------------|-----------------|--------|
| 1 | DM Conversation | `agent-loop.md` | `rocketchat.md`, `ai-provider.md` | `rocketchat.server`, `[[chat_providers]]` | ✅ Implemented |
| 2 | @Mention | `rocketchat.md` 2c | `agent-harness.md` 2f | `rocketchat.server` | ✅ Implemented |
| 3 | Web Search | `tools/exa-search.md` | `agent-harness.md`, `base/config.md` | `[tools.exa]` api_key | ✅ Implemented |
| 4 | URL Fetch | `tools/web-fetch.md` | `agent-harness.md` | — | ✅ Implemented |
| 5 | Image Gen | `agent-harness.md` 2e | `tools/webdav.md` | `[[image_providers]]`, `[image_model]` | ✅ Implemented |
| 6 | Calendar | `tools/calendar.md` | `agent-harness.md` | `[webdav]` calendar_name | ✅ Implemented |
| 7 | Knowledge | `base/knowledge.md` | `base/config.md`, `tools/webdav.md` | `[webdav]` (always on) | 🔄 Planned |
| 8 | Soul Memory | `base/memory.md` 2d | `base/memory.md` 4 | — | ✅ Implemented |
| 9 | WebDAV Files | `tools/webdav.md` | `agent-harness.md` 2d | `[webdav]` | ✅ Implemented |
| 10 | Vision | `agent-harness.md` 3 | — | — | ⚠️ Partial (download + MIME only) |

---

## Architectural Themes

| Theme | Description | DFD Cross-Cut |
|-------|-------------|---------------|
| **No local disk** | All persistent state on NextCloud WebDAV — never touches local filesystem | `context-diagram.md`, `tools/webdav.md` |
| **Per-room isolation** | Independent memory, knowledge, and file workspace per room (channel or DM) | `base/memory.md` 2g, `agent-harness.md` 2f |
| **Graceful degradation** | Every subsystem has error fallbacks — missing API keys skip the tool, snapshot missing reads individual files, AI errors produce a fallback reply | `agent-loop.md` 2b, `base/ai-provider.md` 2b, `base/memory.md` 2f |
| **Config-driven wiring** | Tools and providers are registered conditionally based on `config.toml` sections | `base/config.md`, `AGENTS.md` |
| **Single-agent micro harness** | 3 of 6 standard harness mechanisms (Tools, Context, Knowledge) — no permissions, extensions, or coordination needed | `_docs/agent-harness.md`, `_dfds/agent-harness.md` 1a |
