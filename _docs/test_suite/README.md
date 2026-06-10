# Test Suite Summary

Total: **266 tests** across 3 crates. 138 unit tests, 121 integration tests, 7 ignored (real-integration).

## By Crate

| Crate | Unit | Integration | Real (`#[ignore]`) | Total |
|-------|------|------------|---------------------|-------|
| webdav | 27 | 18 | 4 | **49** |
| rocketchat | 12 | 25 | 3 | **40** |
| rockbot | 99 | 77 | 0 | **176** |

## crate-webdav (49 tests)

### `src/path.rs` — 11 unit tests
WebDavPath helpers: `room_dir`, `root_trim_slashes`, `memory_dir`, `image_path`, `image_dir`, `workspace_dir`, `config_backup_path`, `room_path`, `parent_path`, `room_path_empty_root`.

### `src/client.rs` — 16 unit tests + 1 helper
- **XML parsing (10)**: `parse_propfind_empty`, single-entry with/without namespace/whitespace, `<response>` deserialization (href-only, two-strings, string-then-vec, string-then-struct-vec), `<prop>` deserialization (minimal, with resourcetype, with collection, with opt-string, all fields, getetag with quote), direct prop/propstat/response deserialization.
- **Helper**: `make_test_client()`.

### `tests/integration.rs` — 12 tests
Client construction, WebDavPath integration (room_dir, memory_dir, image_path, room_path, config_backup_path, image_dir, workspace_dir, root_trim).

### `tests/config_tests.rs` — 6 tests
TOML config deserialization (minimal, trailing slash, root with slashes), `into_client()`, `base_url_construction`, missing field error.

### `tests/integration_real.rs` — 4 `#[ignore]` tests
Real WebDAV operations: `ensure_directory_and_list`, `write_and_read_file`, `write_file_auto_mkcol`, `exists`. Require `WEBDAV_URL`, `WEBDAV_USERNAME`, `WEBDAV_PASSWORD` env vars. Helper: `get_client()`.

---

## crate-rocketchat (40 tests)

### `src/ddp.rs` — 12 unit tests + 1 helper
SHA-256 digest (hello + known value), DDP messages (connect, login with digest, subscribe, pong, send_message, typing, msg_field extraction), login result extraction (valid, no-result), dispatch checks (is_ping/is_connected/is_result/is_changed/is_ready/is_nosub). Helper: `is_pong_msg()`.

### `tests/integration.rs` — 25 tests + 2 helpers
- **Message filtering (6)**: channel message with mention, skip own messages, direct message detection, registered room dispatch, non-registered channel no mention, strip_mention.
- **Message parsing (2)**: timestamp parsing, dm/mention detection.
- **DDP payloads (7)**: connect_message, login_message (hashed), send_message_payload, subscribe_payload, typing_payload (true/false), msg_field.
- **Login extraction (2)**: valid, missing fields.
- **Dispatch checks (2)**: dispatch_checks, sha256_digest_known_value.
- **BotReply/IncomingMessage (3)**: BotReply::new, is_dm_or_mention, registered_room_dispatch.
- **Client construction (1)**: client_new derives bot_name.
- **Config (3)**: config_from_toml, use_tls_false, url_strips_protocol.
- **Helpers**: `make_changed_channel()`, `make_changed_dm()`.

### `tests/integration_real.rs` — 3 `#[ignore]` tests
Real RocketChat: `config_toml_exists_and_parses`, `connect_and_receive_events` (30s timeout), `send_message_and_verify`. Requires `config.toml`. Helper: `config_path()`.

---

## crate-rockbot (177 tests)

### `src/harness.rs` — 19 unit tests
- **MockProvider-based (5)**: `simple_response`, `dm_message`, `provider_error`, `max_iterations_limit` (loop cutoff at 2), `construction`.
- **No-mock (14)**: `resolve_model`, `archive_room_if_needed_no_webdav`, `check_and_archive_returns_seq`, `summarize_for_archive`, `inject_room_context` (2), `compute_webdav_dir` (8 variants).
- **Helpers**: `MockProvider` struct, `make_test_config()`.

### `src/tools/webdav.rs` — 10 unit tests
Tool definition validation, `format_size` (0/bytes/KB/MB), `room_path_construction`, `room_dir_construction`, error handling (missing action, unknown action, missing room_id, missing path, write missing content, invalid JSON).

### `src/provider/replicate.rs` — 4 unit tests
Provider construction (success, EDITME key rejection, empty key rejection), provider/model names.

### `src/provider/openrouter.rs` — 19 unit tests + 1 helper
- **Request building (6)**: minimal body, with tools, with temperature+max_tokens, thinking enabled, thinking disabled, with tool_choice.
- **Response parsing (4)**: simple, with tool calls, with reasoning, length finish, no choices.
- **Error mapping (3)**: 401, 429, 500.
- **Construction (4)**: missing key, empty key, provider/model names, with_client.
- **URL (1)**: chat_url custom path.
- **Helper**: `make_provider()`.

### `src/provider/deepseek.rs` — 17 unit tests + 1 helper
- **Request building (3)**: minimal, with options, thinking disabled.
- **Response parsing (4)**: simple, with tool calls, with reasoning, no choices.
- **Error (3)**: extract_error_message (json/plain), http errors (401/429/500).
- **Construction (4)**: missing key, empty key, provider/model names, with_client.
- **URL (2)**: default chat_url, custom chat_path.
- **Helper**: `make_provider()`.

### `src/tools/web_search.rs` — 4 unit tests
Tool definition, to_def, missing query error, invalid JSON error.

### `src/tools/web_fetch.rs` — 15 unit tests
Tool definition, HTML-to-markdown (basic, link, strips script), output format parsing, extract_page_title (found/empty/none), extract_domain, truncate, missing url error, parse_format_parameter, default constructor, with_exa_key, with_client_and_key.

### `src/tools/vision.rs` — 3 unit tests
Tool definition, detect_mime_type (png/jpg/jpeg/gif), missing url error.

### `src/tool.rs` — 6 unit tests + 1 helper
ToolResult (success/error), Registry (register/get, definitions), execute (unknown tool, known tool). Helper: `MockTool` struct.

### `src/memory.rs` — 20 unit tests + 1 helper
ConversationHistory (new, append, needs_archive, needs_archive_too_few_messages, oldest_messages, prune_first, prune_first_all), MemoryManager (get_or_create, build_context, build_context_nonexistent_room), RoomState::new. Helper: `make_msg()`.

### `tests/provider_tests.rs` — 41 tests (no mocks)
Config parsing (example TOML with 2 providers, max_iterations default/custom, find_provider, resolve_model, tool_config_deserialize, find_tool), provider chat_url (default, custom, trailing slash), ChatMessage (system/user/assistant/tool/assistant_with_tool_calls), ToolCall::new, ToolDef::new, ThinkingConfig, ChatRequest JSON serialization (stream present/omitted), CompletionResult defaults, FinishReason/Role serde, MessageContent (text/multipart), ContentPart::ImageUrl serde, UsageInfo, ToolCall/ToolDef serde, strict mode, Error Display (Provider/AuthFailed/RateLimited/MissingApiKey), Error From io, Result type alias, provider construction (deepseek/openrouter new/with_client, EDITME/empty key rejection), AiProvider trait object safety, ChatMessage multipart.

### `tests/integration_mock.rs` — 37 tests (all wiremock)
- **DeepSeek HTTP mock (14)**: simple_response, with_tool_calls, with_reasoning, 401/auth, 429/rate_limit, 500/server_error, 503/overloaded, 402/insufficient_balance, with_thinking_and_tools, custom_chat_path, multi_turn_conversation, 422/invalid_params.
- **OpenRouter HTTP mock (9)**: simple_response (auth header check), with_tools, with_temperature (0.9 + 2048 tokens), 401, 429, 500, with_reasoning.
- **WebDAV HTTP mock (14)**: read, write (AutoMkcol header), list_empty, list_with_entries (formatted sizes), mkdir, delete, exists_true, exists_false, mkdir_deep, write_fallback (happy path, 404-then-mkdir-retry, nested_dir_creation, inner_mkdir_already_exists, both_fail, non_404_error_propagates), ensure_room_directory (creates, already_exists), write_first_time_in_room (end-to-end).
- **Helpers**: `make_test_client()`, `propfind_xml_response()`, `propfind_xml_body()`.

---

## Test Characteristics

| Feature | Count |
|---------|-------|
| Async (`#[tokio::test]`) | 75 |
| Ignored (`#[ignore]`) | 7 |
| Mock-based (wiremock) | 37 |
| Mock-based (inline MockProvider/MockTool) | 11 |
| Test helpers (factories/comparisons) | 14 |

## Running

```bash
# All unit + mock integration tests
cargo test

# All tests including real-integration (needs credentials)
cargo test -- --ignored

# Single crate
cargo test -p webdav
cargo test -p rocketchat
cargo test -p rockbot

# Specific test files
cargo test -p rocketchat --test integration_real -- --ignored
cargo test -p webdav --test integration_real -- --ignored
```
