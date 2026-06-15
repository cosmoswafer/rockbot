# Top 10 User Stories — RockBot

> **Priority**: P0 = critical, P1 = core, P2 = enhancement

1. **DM Conversation** (P0) — Team member DMs bot, gets AI replies with per-room conversation history persisted across sessions via WebDAV `snapshot.json`.

2. **@Mention & Display Name in Channels** (P0) — Channel member `@rockbot` or uses bot's display name (extracted from soul memory via `- My name is ...` pattern); gets reply in-thread with per-room isolation. The display name is used for the incoming message filter and the outgoing REST API `alias` field (not the RocketChat user profile).

3. **Web Search via Exa** (P1) — User asks bot to search web; returns summarized results with URLs, dates, and snippets (auto/fast/deep modes, highlights or full text). 3 retries with exponential backoff on 429/5xx.

4. **HTTP Fetch & Web Requests** (P1) — User shares URL or asks bot to make HTTP requests; bot fetches via full HTTP client (GET/POST/PUT/PATCH/DELETE/HEAD/OPTIONS with custom headers, JSON/raw body, file upload from WebDAV). Returns raw, markdown, or JSON format. Optional Exa verification and WebDAV save. Supports `secret:<UUID>` references for API authentication (resolved at call time from per-room `secrets.toml`).

5. **Image Generation** (P2) — User prompts image; bot generates via configured image provider (fal.ai or OpenRouter), saves to WebDAV, creates NextCloud share link, returns inline display. Supports text-to-image and image-to-image editing with aspect ratio presets and configurable size tier (2K/4K). Previously generated images can be referenced by `image_key` for editing.

6. **Calendar Events & Todos (CalDAV)** (P2) — User creates/lists/updates/deletes calendar events via chat. Lists todos (read-only). Per-room auto-created calendars with ICS generation, recurrence (rrule), and reminders.

7. **Knowledge Persistence** (P2) — User tells bot to save/recall/forget facts with tags, priorities (P0-P3), and `when_useful` context hints. Persisted to WebDAV knowledge index; automatically recalled based on conversation relevance via keyword matching. Priority promotion on use, decay on review.

8. **Permanent Soul Memory** (P1) — User sets identity, preferences, and facts via `edit_soul` (full-replace semantics); bot persists to WebDAV `memory/soul.md` and adapts permanently across restarts. Format is a flat bullet list (`- My name is ...`, `- preference ...`). Identity name extracted from first bullet for message filtering.

9. **WebDAV File Operations** (P1) — User reads/writes/lists/edits/deletes files and creates/checks directories in bot's WebDAV workspace with per-room scoped paths. Actions: read, write, edit (find-and-replace), list, mkdir, delete, exists, rename (move). Images returned as base64 inline markdown. Secrets sanitization preserves host/key but replaces values.

10. **Vision / Image Retrieval** (P2) — User requests an image from WebDAV or a public URL; bot downloads and returns it as inline base64 markdown for vision-capable LLM processing. Attached images in RocketChat messages are downloaded, encoded as data URIs, and injected into LLM context automatically (retained on the latest user message; older messages have images stripped for byte budget).
