# Top 10 User Stories — RockBot

> **Priority**: P0 = critical, P1 = core, P2 = enhancement

1. **DM Conversation** (P0) — Team member DMs bot, gets AI replies with per-room conversation history persisted across sessions.
2. **@Mention & Display Name in Channels** (P0) — Channel member `@rockbot` or uses bot's display name (configurable via Soul Memory `## Identity`); gets reply in-thread with per-room isolation. Registered rooms always dispatch.
3. **Web Search via Exa** (P1) — User asks bot to search web; returns summarized results with URLs, dates, and snippets (auto/fast/deep modes, highlights or full text).
4. **HTTP Fetch & Web Requests** (P1) — User shares URL or asks bot to make HTTP requests; bot fetches via full HTTP client (GET/POST/PUT/PATCH/DELETE/HEAD/OPTIONS with custom headers, JSON/raw body, file upload from WebDAV). Returns raw, markdown, or JSON format. Optional Exa verification and WebDAV save.
5. **Image Generation** (P2) — User prompts image; bot generates via configured image provider (fal.ai or OpenRouter), saves to WebDAV, creates NextCloud share link, returns inline display. Supports text-to-image and image-to-image editing with size/quality presets.
6. **Calendar Events & Todos (CalDAV)** (P2) — User creates/lists/updates/deletes calendar events and todos via chat. Per-room auto-created calendars with ICS generation, recurrence (rrule), and reminders.
7. **Knowledge Persistence** (P2) — User tells bot to save/recall/forget facts with categories (skill/secret/note), tags, priorities (P0-P3), and `when_useful` context hints. Persisted to WebDAV knowledge index; automatically recalled based on conversation relevance.
8. **Permanent Soul Memory** (P1) — User sets identity, preferences, and facts via `edit_soul`; bot persists to WebDAV and adapts permanently across restarts. Identity name syncs bot's RocketChat display name.
9. **WebDAV File Operations** (P1) — User reads/writes/lists/edits/deletes files in bot's WebDAV workspace with per-room scoped paths. Images returned as base64 inline markdown.
10. **Vision / Image Retrieval** (P2) — User requests an image from WebDAV or a public URL; bot downloads and returns it as inline base64 markdown for vision-capable LLM processing. Attached images in RocketChat messages are intercepted and injected into LLM context automatically.
