# Core Test Suite

Fine-grained tests against single DFDs and modules. All inline `#[cfg(test)] mod tests` blocks in `src/` files. No external I/O, no mocking — pure unit tests.

**Total: 408 tests across 26 files (3 crates)**

---

## rocketchat crate (40 tests, 3 files)

### `src/ddp.rs` — 18 tests
DDP protocol message construction (connect, login, subscribe, ping/pong, send, typing), SHA-256 digest generation, sequential ID, message dispatch classification, login result extraction, subscription list parsing.
- Helper: `is_pong_msg()`
- 1 test `#[ignore]` (flaky global `AtomicU64` race)

### `src/types.rs` — 17 tests
`MessageFilter` parsing of RocketChat changed events: room fname extraction, image/file attachment deserialization, alias parsing, emoji stripping, DM/mention detection (bot name + display name matching).

### `src/rest.rs` — 5 tests
REST API client construction (host, TLS, user_id, auth_token), API URL building with TLS, auth header generation (X-Auth-Token, X-User-Id), JSON deserialization of `RoomInfoResponse` and `RoomsGetResponse` including Unicode fname fields.
- Helper: `test_config()`

---

## rockbot crate (330 tests, 20 files)

### `src/memory.rs` — 35 tests
`ConversationHistory` lifecycle (append, char_count, needs_archive, oldest_messages, prune_first, archive_seq), `MemoryManager` room creation/deduplication, context building with message window trimming, `RoomState` construction (channel + DM with Chinese fname), orphaned tool call/message stripping (6 scenarios), identity name extraction from Soul Memory markdown (8 scenarios: standard, emoji, CJK, same-line, no-match, too-long, not-a-header, empty, plus 2 display-name self-detection regex tests).
- Helper: `make_msg()`

### `src/harness.rs` — 49 tests
Full `AgentHarness`: simple response, DM handling, provider error fallback, max_iterations loop-limit, construction, image ID tracking (take/consume), `ImageCache` store/retrieve/consume, model resolution, archive without WebDAV, `check_and_archive` sequencing, summarization, `inject_room_context` (generic + image_gen variants), attachment reference URL injection with prompt matching/merging, `compute_webdav_dir` (8 variants: r-/d- prefix, hyphens, dots, Unicode, empty, fname preference), vision image caching from markdown (5 scenarios), vision image injection (4 scenarios: multipart, pool drain, empty noop, numbered labels), truncate-and-summarize (4 scenarios: below/above limit, system-prompt preservation, last-message preservation), process_message end-to-end effects.
- Helpers: `MockProvider` (implements `AiProvider` with `Mutex<Vec<CompletionResult>>` queue + `AtomicUsize` counter), `make_test_config()`

### `src/tools/web_search.rs` — 14 tests
`WebSearchTool` definition, `to_def()`, error paths (missing query, invalid JSON), argument parsing with optionals/defaults/num_results bounds (1-20), Exa API request body construction (highlights, non-neural type, no deprecated params), highlight result parsing, text fallback truncation.

### `src/tools/web_fetch.rs` — 26 tests
`WebFetchTool` definition (HTTP methods: GET/POST/PUT/PATCH/DELETE/HEAD/OPTIONS), HTML-to-Markdown conversion (headings, bold, links, script removal, Chinese content), `remove_tag_content`, `OutputFormat`/`HttpMethod` parsing, header parsing (JSON dict → reqwest HeaderMap, empty/none), page title extraction, domain extraction, text truncation, error paths (missing URL, invalid method), format parameter parsing, constructor variants (`default()`, `with_exa_key()`, `with_client_and_key()`), verified/related_sources fields, request building (GET, POST with body, custom headers).

### `src/tools/webdav.rs` — 19 tests
`WebDavTool` definition (7 actions: read/write/edit/list/mkdir/delete/exists), file size formatting (B/KB/MB), room path/dir construction (channel r- and DM d- prefixes), error paths (missing action, unknown action, missing room_id, missing path, missing write content, invalid JSON, missing oldString/newString in edit), image extension detection (png/jpg/jpeg/gif/svg/webp), MIME type mapping.

### `src/tools/calendar.rs` — 12 tests (includes mini_calendar)
`DateTimeTool` definition, `civil_from_days`/`days_from_civil` date algorithms (epoch, known dates, roundtrip), weekday names/indexes, ISO week number, calendar and weekday output generation, all 7 execution output formats (full, iso, human, unix, calendar, weekdays, week_number), empty args default, week_offset variants.

### `src/tools/image_gen.rs` — 23 tests
`ImageGenTool` definition (hidden `image_size` from LLM), execute error paths (missing prompt, invalid JSON), UUID format generation, webdav_dir extraction with room_id fallback, output format extension mapping, `ImageGenParams` construction/parsing (presets, custom sizes, image_urls for img2img, edge cases).
- Helpers: `make_fal_config()`, `make_fal_provider()`, `make_webdav()`, `make_image_cache()`

### `src/tools/vision.rs` — 9 tests
`VisionTool` definition, image name extraction from URLs (with/without query params), MIME detection by extension (png/jpg/jpeg/gif/webp/svg) with content-type preference, markdown tag format, `with_max_bytes` constructor, optional prompt parameter, missing URL error.

### `src/tools/calendar.rs` — 8 tests
`CalendarTool` definition (6 actions including list_todos), CalDAV URL construction (with/without dav prefix), room calendar cache, error paths (missing action, unknown action, add_event without summary).
- Helper: `make_test_tool()`

### `src/tools/edit_soul.rs` — 3 tests
`EditSoulTool` definition (required content field, webdav_dir parameter), missing content error, soul path construction.

### `src/tools/save_knowledge.rs` — 4 tests
`SaveKnowledgeTool` definition (category enum: skill/secret/note), error paths (missing category, invalid category), tag parsing with empty/missing edge cases.

### `src/tools/forget_knowledge.rs` — 3 tests
`ForgetKnowledgeTool` definition (name, description with "delete" and "index"), missing topic error.

### `src/tools/recall_knowledge.rs` — 2 tests
`RecallKnowledgeTool` definition (name, description with "query" and "Search").

### `src/tool.rs` — 6 tests
`ToolResult` success/error construction, `ToolRegistry` register/get/definitions, execute (known tool, unknown tool error).
- Helper: `MockTool` (implements `Tool` trait)

### `src/knowledge.rs` — 10 tests
`KnowledgeManager::slugify` (English, Chinese, empty/symbol-only), `KnowledgeCategory` Display impl, `match_relevant` by title/tag/when_useful/no-match, path construction helpers.

### `src/provider/deepseek.rs` — 25 tests
Request body building (minimal, tools, temperature/max_tokens, thinking enabled/disabled, reasoning_effort), response parsing (simple, tool_calls, reasoning_content, no choices error), error extraction (JSON/plain text), HTTP error mapping (401/429/500/400 context length), context length error detection (case-insensitive), constructor validation (missing/"EDITME"/empty key), chat URL (default / custom path), provider name/model, thinking disabled mode, `strip_message_images` (4 scenarios: text-only passthrough, single/multiple image replacement with [image]).

### `src/provider/openrouter.rs` — 33 tests
**Top-level:** Request body building (minimal, tools, temperature/max_tokens, thinking enabled/disabled, tool_choice), response parsing (simple, tool_calls, reasoning_content, finish_reason=length, no choices), HTTP error mapping (401/429/500/context_length), context length error detection, constructor validation, custom chat_path, `with_client` constructor, provider name/model.
**Image provider sub-module:** Constructor validation, provider name/model (both inherent and through `dyn ImageProvider` trait), `preset_to_aspect_ratio` mapping (landscape_16_9→16:9, square→1:1, passthrough), upload_file data URIs, response body parsing, `model_id` override.
- Helpers: `make_provider()`, `make_image_provider()`

### `src/provider/fal.rs` — 22 tests
`FalAiProvider::new` (success, missing/"EDITME"/empty key), provider name/model, `ImageGenParams` defaults and full-field construction, image size resolution presets (landscape_16_9=3840x2160, square_hd=2880x2880, landscape_4_3=3312x2480), custom size, None passthrough, unknown preset string passthrough, aspect-ratio strings (16:9, 2:3, 1:1, 9:16, 3:4, 3:2), named orientations (portrait_16_9, portrait_4_3, landscape_3_2, square).
- Helper: `make_config()`

### `src/utils.rs` — 9 tests
`civil_from_days` date conversion (epoch=1970-01-01, known date), `strip_emoji` (CJK+emoji, no-emoji, emoji-only), `strip_markdown_image_id` (markdown `![alt](call_id)` removal, no-match passthrough, inline, plain-text key removal).

### `src/types.rs` — 9 tests
`ImageGenParams::validate_dimensions`: preset bypass, custom valid dimensions (1920x1088), None passthrough, max edge exceeded (>3840), aspect ratio exceeded, pixel count too low (<1M), pixel count too high (>14.7M), not multiple of 16, zero edge.

---

## webdav crate (38 tests, 3 files)

### `src/path.rs` — 10 tests
`WebDavPath` construction: room_dir, root trimming, memory_dir, image_path/image_dir, workspace_dir, config_backup_path, room_path (leading-slash normalization), `parent_path`, empty root edge case.

### `src/client.rs` — 20 tests
WebDAV PROPFIND XML parsing using `quick-xml`: empty multistatus, namespace handling, href-only response, string-then-vec/struct-vec responses, minimal prop (getcontentlength, resourcetype, collection detection, getlastmodified, etag), quoted etag, propstat direct deserialization, sibling status element, full NextCloud-style multistatus (ownCloud/Nextcloud namespace extensions), empty/missing getcontentlength defaults.
- Helper: `make_test_client()`

### `src/calendar.rs` — 8 tests
iCalendar parsing: VEVENT (simple, with description+location, with VALARM/reminder), VTODO (summary, priority, status, due date), VEVENT ICS building (with/without reminders), iCal text escaping/unescaping.

---

## Summary

| Crate | Files | Tests |
|-------|-------|-------|
| rocketchat | 3 | 40 |
| rockbot | 20 | 330 |
| webdav | 3 | 38 |
| **Total** | **26** | **408** |

### Files with no core tests
`rocketchat`: config.rs, error.rs, main.rs, lib.rs
`rockbot`: config.rs, error.rs, image_cache.rs, lib.rs, main.rs, provider/mod.rs, tools/mod.rs
`webdav`: config.rs, error.rs, lib.rs, types.rs
