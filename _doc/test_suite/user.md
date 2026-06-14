# User Test Suite

Tests driven by user stories and end-to-end scenarios, verifying multiple DFDs work together. Tests in `tests/` directories using wiremock-mocked HTTP or in-memory data structures. No live servers needed.

**Total: 164 tests across 7 files (3 crates)**

| Crate | File | Tests | Mocked | Non-mock |
|-------|------|-------|--------|----------|
| rocketchat | `tests/integration.rs` | 30 | — | 30 |
| rocketchat | `tests/integration_rest.rs` | 9 | 9 | — |
| rockbot | `tests/integration_mock.rs` | 61 | 56 | 5 |
| rockbot | `tests/provider_tests.rs` | 46 | — | 46 |
| rockbot | `tests/knowledge_real.rs` | 1 | — | 1 |
| webdav | `tests/integration.rs` | 10 | — | 10 |
| webdav | `tests/config_tests.rs` | 7 | — | 7 |

---

## rocketchat crate

### `tests/integration.rs` — 30 tests (sync, in-memory)
**DFDs exercised:** `rocketchat.md`, `rocketchat-rest.md`

| Group | Tests | Coverage |
|-------|-------|----------|
| MessageFilter / IncomingMessage | 8 | Channel message parsing with/without fname, own-message skip, DM detection, registered room dispatch, non-registered channel no-mention, timestamp parsing, mention stripping |
| DDP message construction | 8 | connect, login (hashed), sendMessage (with/without alias), createDirectMessage, setRealName, subscribe, typing (on/off) |
| DDP parsing helpers | 3 | extract_login_result (valid/missing), msg_field helper |
| DDP dispatch checks | 1 | is_ping/is_connected/is_result/is_changed/is_ready/is_nosub |
| Config TOML | 3 | from_toml, use_tls, url_strips_protocol |
| BotReply / Client | 3 | BotReply::new, BotReply::with_alias, client_new |
| DM / room dispatch | 3 | DM detection, registered_room_dispatch |
| Crypto | 1 | SHA-256 digest known value |

**Helpers:** `make_changed_channel()`, `make_changed_dm()`

### `tests/integration_rest.rs` — 9 tests (async, wiremock)
**DFDs exercised:** `rocketchat-rest.md`

| Group | Tests | Coverage |
|-------|-------|----------|
| get_rooms() | 1 | Unicode fname survives roundtrip |
| get_room_info() | 2 | Valid response parsing, 404 returns None |
| send_message() | 3 | With alias, without alias, error response handling |
| resolve_room_fname() | 2 | Caches result, returns None for missing room |
| get_message() | 1 | Parses response including alias field |

**Helper:** `test_config()`

---

## rockbot crate

### `tests/integration_mock.rs` — 61 tests (56 wiremock + 5 in-memory)
**DFDs exercised:** `agent-harness.md`, `tools/webdav.md`, `base/ai-provider.md`, `base/memory.md`, `tools/image-gen.md`

#### DeepSeekProvider.complete() — 12 tests (wiremock)
Simple response, tool_calls, reasoning_content, 401/429/500/503/402/422 error paths, thinking+tools combined, custom chat_path, multi-turn conversation.

#### OpenRouterProvider.complete() — 7 tests (wiremock)
Simple response (auth header check), tool_calls, temperature+max_tokens, 401/429/500 errors, reasoning_content.

#### WebDavTool basic operations — 9 tests (wiremock)
Read file, write (AutoMkcol header), list empty, list with entries (size formatting), mkdir (new + existing), delete, exists (true/false), mkdir deep.

#### WebDAV write-with-fallback — 6 tests (wiremock)
Happy path (AutoMkcol succeeds), 404-then-mkdir-retry, nested dir creation, inner mkdir already exists, both-fail error propagation, non-404 error propagation.

#### WebDAV ensure_room_directory — 3 tests (wiremock)
Creates new dir, silently ignores already-exists, full first-time-in-room flow.

#### WebDavTool edit — 3 tests (wiremock)
Successful replace (1 occurrence), oldString not found error, multiple matches error.

#### OpenRouterImageProvider — 7 tests (wiremock)
Successful generation, img2img with image_url, 401 unauthorized, missing images field, with aspect_ratio+quality+num_images, upload_file data URI.

#### FalAiProvider.generate_image_url() — 7 tests (wiremock)
Full submit→poll(COMPLETED)→fetch→image URL pipeline, 401 unauthorized, missing request_id, NSFW/FAILED, poll 503 error, missing status_url, missing response_url.

#### WebDavTool schema — 1 test (wiremock)
Verifies `webdav_dir` is NOT exposed in LLM-facing parameters.

#### MemoryManager — 4 tests (in-memory)
Rapid message no-loss, snapshot with soul+summaries no conflict, repeated snapshot builds preserve data, multi-room no cross-contamination.

**Helpers:** `make_test_client()`, `propfind_xml_response()`, `propfind_xml_body()`, `make_openrouter_image_config()`, `make_fal_config()`

### `tests/provider_tests.rs` — 46 tests (sync, in-memory)
**DFDs exercised:** `base/config.md`, `base/ai-provider.md`

| Group | Tests | Coverage |
|-------|-------|----------|
| Config TOML | 11 | Example TOML parsing, max_iterations default/custom, find_provider, resolve_model, tool_config_deserialize, find_tool, merge user-wins, validate missing provider, named-array overrides |
| chat_url() | 3 | Default, custom path, trailing slash |
| Type constructors | 10 | ChatMessage (system/user/assistant/tool/with_tool_calls), ToolCall, ToolDef, ThinkingConfig, CompletionResult defaults, UsageInfo |
| JSON serde | 8 | ChatRequest (stream present/omitted), FinishReason, Role, ToolCall, ToolDef, ToolDef with strict |
| Content multipart | 3 | MessageContent text/multipart, ContentPart::ImageUrl serde |
| Error handling | 3 | Display, From io, Result type alias |
| Provider construction | 5 | DeepSeek new/with_client, OpenRouter new/with_client, EDITME/empty key rejection |
| Trait object safety | 2 | AiProvider, ImageProvider |

### `tests/knowledge_real.rs` — 1 test (sync, in-memory)
**Note:** Only the non-ignored test is counted here; 2 `#[ignore]` tests are cataloged under Real tests.
- `test_knowledge_module_is_public` — compile-time check that `IndexEntry`, `KnowledgeCategory`, `KnowledgePriority`, `KnowledgeManager` are publicly accessible.

---

## webdav crate

### `tests/integration.rs` — 10 tests (sync, in-memory)
WebDavClient construction (with/without trailing slash), WebDavPath helpers (room_dir, memory_dir, image_path, root_trim, image_dir, workspace_dir, room_path, config_backup_path).

### `tests/config_tests.rs` — 7 tests (sync, in-memory)
WebDavConfig TOML deserialization (minimal, trailing slash URL, root with slashes), `into_client()`, `base_url_construction`, `dav_path_override`, missing field error.

---

## Mock vs Non-Mock Breakdown

| Type | Count | Files |
|------|-------|-------|
| Wiremock-mocked HTTP | 65 | `integration_rest.rs` (9), `integration_mock.rs` (56) |
| In-memory / no I/O | 99 | `integration.rs` rocketchat (30), `provider_tests.rs` (46), `integration_mock.rs` (5), `knowledge_real.rs` (1), `integration.rs` webdav (10), `config_tests.rs` (7) |

## DFD Coverage Map

| DFD | User test files covering it |
|-----|----------------------------|
| `base/rocketchat.md` | `integration.rs`, `integration_rest.rs` |
| `base/rocketchat-rest.md` | `integration_rest.rs` |
| `base/ai-provider.md` | `integration_mock.rs` (DeepSeek + OpenRouter sections), `provider_tests.rs` |
| `base/config.md` | `provider_tests.rs`, `config_tests.rs` |
| `base/memory.md` | `integration_mock.rs` (MemoryManager section) |
| `tools/webdav.md` | `integration_mock.rs` (WebDavTool + fallback sections) |
| `tools/image-gen.md` | `integration_mock.rs` (Fal + OpenRouterImage sections) |
| `base/knowledge.md` | `knowledge_real.rs` |
