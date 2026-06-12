# Real Test Suite

Tests connecting to live servers and APIs with real credentials. All `#[ignore]`-annotated — not run by default. Use for data collection during development and final integration verification.

**Total: 21 tests across 6 files (3 crates) — 20 true real tests + 1 flaky unit test**

*Excludes 1 test in `rocketchat/src/ddp.rs` ignored for flaky `AtomicU64` global state race (not a real integration test).*

---

## rocketchat crate — 10 tests

### `tests/integration_real.rs` — 7 tests
**Services:** RocketChat DDP, RocketChat REST, NextCloud WebDAV
**Config:** `config.toml` (`[rocketchat.server]` + `[webdav]`)
**Run:** `cargo test --test integration_real -- --ignored`

| # | Test | What it does | Services |
|---|------|-------------|----------|
| 1 | `test_config_toml_exists_and_parses` | Loads `config.toml`, validates server url/username/password/TLS fields populated. Smoke test — no connection. | Config only |
| 2 | `test_connect_and_receive_events` | Connects via `RocketChatClient`, registers "general" room, runs 30s event loop. Auto-replies "Echo: {text}" to each message. Asserts ≥1 message received. | DDP |
| 3 | `test_send_message_and_verify` | Validates `ws_uri()` starts with `wss://` and ends with `/websocket`, checks `host()` returns clean hostname. URI validation only. | Config only |
| 4 | `test_send_message_with_alias_two_clients` | Two DDP sessions connect. A creates DM, B subscribes. A sends via REST `chat.sendMessage` with `alias: "CoolAliasBot"`. Both clients verify alias on changed event. Also unit-tests `IncomingMessage` alias parsing. Cleans up. | DDP, REST |
| 5 | `test_set_real_name_via_ddp` | Calls `setRealName` DDP method to set name to `TestBot_{pid%10000}`, verifies result, reverts to original "香菜" (2s rate limit). | DDP |
| 6 | `test_soul_to_rest_alias_end_to_end` | Fetches `soul.md` from WebDAV (tries `d-saru`, `d-🐵 猴一隻`), extracts `## Identity` name via regex, opens two DDP sessions, sends via REST with extracted name as alias, both verify alias. **Cross-service: WebDAV → DDP → REST.** | DDP, REST, WebDAV |
| 7 | `test_send_image_attachment_via_ddp` | Creates DM, sends `sendMessage` with hardcoded 1x1 transparent PNG data URI attachment, verifies delivery. Tests image attachment path used by rockbot. | DDP |

**Helpers:** `config_path()`, `sha256_digest()`, `init_crypto()`, `ddp_handshake()`, `expect_msg()`, `read_until_alias()`, `expect_msg_json()`, `read_until_text()`, `load_webdav_config()`, `fetch_soul_display_name()`

### `tests/integration_dual.rs` — 3 tests
**Services:** RocketChat DDP
**Config:** `config.toml` (`[rocketchat.server]`)
**Run:** `cargo test --test integration_dual -- --ignored --nocapture`

| # | Test | What it does | Services |
|---|------|-------------|----------|
| 1 | `test_message_roundtrip_dual` | Two independent DDP sessions A and B. Both create DM to self, subscribe. A sends message; B polls changed event. End-to-end delivery validation. | DDP |
| 2 | `test_typing_indicator_roundtrip` | A sends typing ON/OFF via `ddp::typing_payload()`. B subscribes to `stream-notify-room/{rid}/user-activity` and verifies "user-typing" event then empty activities. | DDP |
| 3 | `test_message_alias_roundtrip` | A sends message with `alias: "TotallyRealHuman"` via DDP. Checks if accepted (requires `message-impersonate`). If accepted, B verifies alias. Falls back to plain roundtrip if rejected. | DDP |

**Helpers:** `config_path()`, `next_local_id()`, `send_json()`, `expect_msg()`, `raw_connect()`, `subscribe_my_messages()`, `subscribe_notify()`, `create_dm()`, `send_message()`, `uuid_v4_simple()`

---

## rockbot crate — 3 tests

### `tests/fal_real.rs` — 1 test
**Services:** fal.ai API
**Config:** `config.toml` (`[[image_providers]]` with `name = "fal"`)
**Run:** `cargo test --test fal_real -- --ignored`

| # | Test | What it does | Services |
|---|------|-------------|----------|
| 1 | `test_fal_image_edit_with_p1` | Loads fal config, reads `_docs/ref_img/p1.png`, uploads to fal CDN, calls `generate_image_url()` with edit prompt ("Change red sweater to Rei Ayanami's blue plugsuit") and `landscape_4_3` preset. Exercises full pipeline: upload_file → submit_request → poll_status → fetch_result. Asserts result is HTTPS URL. | fal.ai |

**Helpers:** `workspace_root()`, `load_fal_config()`

### `tests/knowledge_real.rs` — 2 tests
**Services:** NextCloud WebDAV
**Creds:** `WEBDAV_URL/WEBDAV_USERNAME/WEBDAV_PASSWORD/WEBDAV_ROOT` env vars, or `config.toml` `[webdav]`
**Run:** `cargo test --test knowledge_real -- --ignored`

| # | Test | What it does | Services |
|---|------|-------------|----------|
| 1 | `test_knowledge_save_and_read_index` | Clears previous index. Saves a `Note` entry with topic, content, `when_useful`, `tags: ["testing","real","integration"]`, `P1` priority. Reads `index.json` back. Asserts filename shape (`note_`+slug), `when_useful`+`tags` fields present in JSON. Tests `recall_entry()` by query matches. Cleans up. | WebDAV |
| 2 | `test_knowledge_match_relevant_with_new_fields` | Saves two entries (a `Skill` about building Cargo projects, a `Note` about a phone number) with distinct `when_useful`/`tags`. Tests `match_relevant()` with tag keyword ("phone"), when_useful keyword ("compile"), title keyword ("build"). Asserts correct entry returned per query. Cleans up. | WebDAV |

**Helper:** `get_webdav_client()`

---

## webdav crate — 7 tests

### `tests/integration_real.rs` — 7 tests
**Services:** NextCloud WebDAV
**Creds:** `WEBDAV_URL/WEBDAV_USERNAME/WEBDAV_PASSWORD/WEBDAV_ROOT` env vars (primary), or `config.toml` `[webdav]` (fallback)
**Run:** `cargo test -p webdav --test integration_real -- --ignored`

| # | Test | What it does | Services |
|---|------|-------------|----------|
| 1 | `test_real_ensure_directory_and_list` | Creates timestamped dir under `/test-run/`, lists it, asserts self-reference entry present. Cleans up. | WebDAV |
| 2 | `test_real_write_and_read_file` | Writes `"Hello WebDAV!"` to `hello.txt`, reads back, asserts content matches. Cleans up. | WebDAV |
| 3 | `test_real_write_file_auto_mkcol` | Writes to `deep/nested/file.txt` with AutoMkcol header, reads back. Skips gracefully if server returns 404 (not NextCloud 32+). | WebDAV |
| 4 | `test_real_exists` | Asserts non-existent dir returns false, creates it, re-checks true, cleans up. | WebDAV |
| 5 | `test_real_list_directory_empty` | Creates empty dir, lists it, asserts self-reference entry present even for empty dirs. | WebDAV |
| 6 | `test_real_list_directory_with_files` | Creates 3 files (`alpha.txt`, `beta.md`, `gamma.log`) + 1 subdir (`subdir/`). Lists and asserts every field: name, href, is_dir, size, modified timestamp. Cleans up. | WebDAV |
| 7 | `test_real_list_handles_special_chars` | Creates file named `"résumé (2026) v2.0.txt"` (Unicode + spaces + dots). Lists and asserts correct name and non-zero size. Cleans up. | WebDAV |

**Helpers:** `get_client()`, `unique_dir()`, `ensure_test_run_root()`, `test_list_entries()`, `format_size()`

---

## Credential Sources

| Source | Used by |
|--------|---------|
| `config.toml` `[rocketchat.server]` | `integration_real.rs`, `integration_dual.rs` |
| `config.toml` `[webdav]` | `integration_real.rs` (rocketchat test 6), `integration_real.rs` (webdav), `knowledge_real.rs` |
| `config.toml` `[[image_providers]]` with `name = "fal"` | `fal_real.rs` |
| `WEBDAV_URL/WEBDAV_USERNAME/WEBDAV_PASSWORD/WEBDAV_ROOT` env vars | `integration_real.rs` (webdav, primary), `knowledge_real.rs` (primary) |
| `CONFIG_FILE` env var | All (config path override) |

## How to Run

```bash
# All real integration tests across workspace
cargo test -- --ignored

# Per crate
cargo test -p rocketchat -- --ignored
cargo test -p rockbot -- --ignored
cargo test -p webdav -- --ignored

# Single test file
cargo test --test integration_real -- --ignored                      # rocketchat
cargo test --test integration_dual -- --ignored                      # rocketchat
cargo test --test fal_real -- --ignored                              # rockbot
cargo test --test knowledge_real -- --ignored                        # rockbot
cargo test -p webdav --test integration_real -- --ignored            # webdav

# Single test
cargo test -p webdav --test integration_real test_real_exists -- --ignored
```
