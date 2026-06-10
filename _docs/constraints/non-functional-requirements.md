# Non-Functional Requirements — RockBot

Following ISO 25010 quality model categories, adapted for a single-agent
AI chatbot with WebDAV-backed persistence.

---

## 1. Performance Efficiency

| ID | Requirement | Measure / Threshold | Source | Related Story |
|----|------------|---------------------|--------|---------------|
| NFR-PE1 | Agent loop must terminate within a bounded number of iterations | `max_iterations` default **8**, configurable via `[rocketchat.model]` | `example.config.toml:55` | #1, #2 |
| NFR-PE2 | Web fetch must time out before exhausting agent loop patience | HTTP request timeout **30s** | `_dfds/tools/web-fetch.md` 2b | #4 |
| NFR-PE3 | Layer 1 (chat history) must stay within a fixed memory budget per room | `max_text_length` default **50,000** chars, `max_history_size` default **12** messages | `_dfds/base/memory.md` 4 | #1, #2 |
| NFR-PE4 | Layer 2 (summaries) must fit within the AI context injection budget | `max_summary_chars` default **8,000** chars across all loaded summaries | `_dfds/base/memory.md` 4 | #1, #2 |
| NFR-PE5 | Layer 3 (soul) must be bounded | `max_soul_chars` default **2,000** chars per room | `_dfds/base/memory.md` 4 | #8 |
| NFR-PE6 | Snapshot writes must be coalesced to avoid thrashing WebDAV | Timer every `persist_interval_secs` (default **60s**) batches dirty snapshots; not written on every mutation | `_dfds/base/memory.md` 2c | #8 |
| NFR-PE7 | Exa search results must be bounded | `numResults` default **5**, max highlight chars **10,000** per URL | `_dfds/tools/exa-search.md` 2c, 3 | #3 |
| NFR-PE8 | Web fetch response body must be truncated for agent consumption | **10,000** chars for `json` format `content` field | `_dfds/tools/web-fetch.md` 3 | #4 |

## 2. Reliability

| ID | Requirement | Measure / Threshold | Source | Related Story |
|----|------------|---------------------|--------|---------------|
| NFR-RL1 | WebSocket disconnect must trigger automatic reconnection | Exponential backoff `2^attempt` seconds, max **5** retries, then graceful shutdown | `_dfds/agent-loop.md` 2b | #1, #2 |
| NFR-RL2 | AI provider errors must be handled without crashing the agent loop | 5xx → retry with backoff, 429 → rate-limit backoff, 401 → immediate fail with clear message | `_dfds/base/ai-provider.md` 2b | #1, #2 |
| NFR-RL3 | WebDAV write to a non-existent directory must automatically create parent dirs | AutoMkcol header → 404 → mkcol all parents → plain PUT retry | `_dfds/tools/webdav.md` 2c | #9 |
| NFR-RL4 | Snapshot write failures must not block the agent loop | Keep dirty flag → retry on next timer tick; system continues | `_dfds/base/memory.md` 2f | #8 |
| NFR-RL5 | Soul write failures must not crash the bot | Warn + continue; next soul edit retries | `_dfds/base/memory.md` 2f | #8 |
| NFR-RL6 | Room state must survive process restart | Cache-first: read `snapshot.json` from WebDAV; fall back to individual files (`soul.md`, `summaries/*.md`) if missing | `_dfds/base/memory.md` 2e | #1, #2 |
| NFR-RL7 | DDP subscription loss must trigger automatic re-subscription | On `"nosub"` message → re-subscribe `stream-room-messages` inline | `_dfds/base/rocketchat.md` 2d | #1, #2 |
| NFR-RL8 | Tool execution errors must produce a fallback reply, not crash the loop | `SendFallbackReply` on API or tool execution error | `_dfds/agent-harness.md` 2b | #1–#10 |
| NFR-RL9 | AI summary generation failure during archiving must not block the loop | API error → truncate oldest messages without summarizing (lossy fallback) | `_dfds/base/memory.md` 2f | #1, #2 |
| NFR-RL10 | Knowledge load failure on room init must proceed without knowledge | WebDAV read / index parse failure → warn + continue with empty context | `_dfds/base/knowledge.md` 2c | #7 |
| NFR-RL11 | Max iterations exceeded must force a final reply, not loop forever | `CheckIterationLimit` → `TruncateAndSummarize` → return final reply | `_dfds/agent-harness.md` 2c | #1, #2 |
| NFR-RL12 | Knowledge forget must be idempotent | File already deleted → still remove from index; return success | `_dfds/base/knowledge.md` 4 | #7 |
| NFR-RL13 | CalDAV concurrent update conflicts must be recoverable | 409 ETag mismatch → refetch current event → merge → retry PUT | `_dfds/tools/calendar.md` 2c | #6 |

## 3. Security

| ID | Requirement | Measure / Threshold | Source | Related Story |
|----|------------|---------------------|--------|---------------|
| NFR-SC1 | Credentials must never be hardcoded in source | All secrets read from `config.toml` (gitignored) via `CONFIG_FILE` env var | `_dfds/base/config.md` | #1, #3, #5, #6, #9 |
| NFR-SC2 | RocketChat password must be SHA-256 hashed before transmission over DDP | `sha2::Digest` in `ddp.rs`; digest sent as lowercase hex with `"algorithm": "sha-256"` | `_dfds/base/rocketchat.md` 2f | #1, #2 |
| NFR-SC3 | WebDAV credentials must be transmitted via HTTP Basic auth over TLS | Base64-encoded `username:password` in `Authorization` header | `_dfds/tools/webdav.md` 3 | #9 |
| NFR-SC4 | WebSocket transport must use TLS (wss://) — no plain WS | `tokio-tungstenite` + `rustls-tls-native-roots` | `AGENTS.md` | #1, #2 |
| NFR-SC5 | Provider API keys must be sent in HTTP headers, never in query parameters or body | `x-api-key` header for Exa; `Authorization: Bearer` for AI providers | `_dfds/tools/exa-search.md` 4 | #3 |
| NFR-SC6 | `config.toml` and `Cargo.lock` must be gitignored | `.gitignore` entries prevent credential and lockfile leakage | `AGENTS.md` | all |
| NFR-SC7 | Exa API key must support env var fallback for CI/CD scenarios | `EXA_API_KEY` env var used if `[tools.exa]` section is absent | `AGENTS.md` | #3 |
| NFR-SC8 | Bot must never echo or respond to its own messages | `MessageFilter::filter()` drops messages where `sender_id == bot_user_id` | `_dfds/base/rocketchat.md` 2c | #1, #2 |

## 4. Maintainability

| ID | Requirement | Measure / Threshold | Source | Related Story |
|----|------------|---------------------|--------|---------------|
| NFR-MN1 | Codebase must be organized as a workspace of independent crates | 3 crates: `crate-rocketchat`, `crate-rockbot`, `crate-webdav` with workspace root `Cargo.toml` | `Cargo.toml` | all |
| NFR-MN2 | AI providers must conform to a shared `AiProvider` trait | `async-trait` with single `complete(&self, ChatRequest) -> Result<CompletionResult>` interface | `AGENTS.md` | #1, #2 |
| NFR-MN3 | Tools must conform to a shared `Tool` trait and be registered in a `ToolRegistry` | `ToolRegistry` holds `HashMap<String, Box<dyn Tool>>`; agent loop dispatches by name | `_dfds/agent-harness.md` 3 | #3–#10 |
| NFR-MN4 | Tool and provider registration must be config-driven, conditionally compiled at runtime | `WebDavTool` / `ImageGenTool` registered only if corresponding config section is present | `AGENTS.md` | #5, #6, #9 |
| NFR-MN5 | Configuration must use typed `serde` deserialization with nested sub-structs | `AppConfig` → `RocketChatSection`, `ProviderConfig`, `WebDavConfig`, `ToolServiceConfig` | `_dfds/base/config.md` | all |
| NFR-MN6 | All I/O must be async using tokio runtime | No `std::thread::spawn`, no synchronous `reqwest::blocking`, no sync file I/O | `AGENTS.md` | all |
| NFR-MN7 | Architecture documentation (DFDs) must be the source of truth, not reverse-engineered from code | DFD-to-code mapping table in `AGENTS.md`; implementation follows DFDs, not the inverse | `AGENTS.md` — Phase 2 | all |
| NFR-MN8 | Mermaid diagram syntax must be validated before commit | `mermaid-cli` skill runs `mermaid.parse()` via jsdom | `AGENTS.md` — skills | all |
| NFR-MN9 | Minimum supported Rust version: 1.85, Edition 2024 | `rust-version = "1.85"` in workspace, `edition = "2024"` | `Cargo.toml` | all |
| NFR-MN10 | Optional tooling must be absent by design (no CI, no rustfmt/clippy config files, no rust-toolchain) | Zero CI config files, zero linter configuration files | `AGENTS.md` | all |

## 5. Compatibility

| ID | Requirement | Measure / Threshold | Source | Related Story |
|----|------------|---------------------|--------|---------------|
| NFR-CM1 | AI provider APIs must be OpenAI-compatible | Shared `ChatRequest` / `CompletionResult` types; providers format provider-specific headers internally | `_dfds/base/ai-provider.md` 1 | #1, #2 |
| NFR-CM2 | RocketChat communication must use DDP over WebSocket | Protocol: `connect → login → sub stream-room-messages → stream changed events` | `_dfds/base/rocketchat.md` 1 | #1, #2 |
| NFR-CM3 | WebDAV client must target NextCloud server API | NextCloud-specific header `X-NC-WebDAV-AutoMkcol: 1` for auto-directory creation | `_dfds/tools/webdav.md` 4 | #9 |
| NFR-CM4 | Calendar must follow RFC 4791 (CalDAV) and RFC 5545 (iCalendar) | `VEVENT` payloads, `calendar-query` REPORT XML, time-range filters | `_dfds/tools/calendar.md` 2b | #6 |
| NFR-CM5 | Config must use TOML format (not legacy Python `config.json`) | `serde` deserialization; `[[chat_providers]]` and `[[image_providers]]` array-of-tables syntax | `AGENTS.md` | all |
| NFR-CM6 | DDP protocol deprecation risk is accepted; no migration to Rocket.Chat Apps-Engine | Bot continues using legacy realtime API; documented risk | `_dfds/base/rocketchat.md` 1 | #1, #2 |

## 6. Scalability & Capacity

| ID | Requirement | Measure / Threshold | Source | Related Story |
|----|------------|---------------------|--------|---------------|
| NFR-SC1 | Per-room memory must be fully isolated | Independent WebDAV directories (`r-{name}` channels, `d-{name}` DMs) each with `memory/`, `images/`, `workspace/` | `_dfds/base/memory.md` 2g | #1, #2 |
| NFR-SC2 | Idle rooms must be evicted from memory to free resources | `memory_ttl_secs` default **300s** (5 min); snapshot persisted before eviction | `_dfds/base/memory.md` 4 | #1, #2 |
| NFR-SC3 | Evicted rooms must restore on next interaction with a single WebDAV GET | Cache-first: `GET snapshot.json` restores all three layers in one round trip | `_dfds/base/memory.md` 2e | #1, #2 |
| NFR-SC4 | Snapshot must be a performance cache; individual files remain the source of truth | `soul.md` + `summaries/*.md` always authoritative; snapshot rebuilds incrementally | `_dfds/base/memory.md` 1 | #1, #2, #8 |
| NFR-SC5 | Old daily summaries must be auto-deleted to cap storage growth | Rolling `summary_days` window (default **7** days); DELETE old `.md` on each archive tick | `_dfds/base/memory.md` 4 | #1, #2 |
| NFR-SC6 | HTTP connections to WebDAV must use a shared connection pool | Single `reqwest::Client` instance per `WebDavClient` | `_dfds/tools/webdav.md` 3 | #9 |
| NFR-SC7 | Calendar events must be globally scoped (shared across rooms) — not per-room | `/calendars/{calendar_name}/` at WebDAV root; design decision: meetings span rooms | `_dfds/tools/calendar.md` 1 | #6 |

## 7. Portability & Availability

| ID | Requirement | Measure / Threshold | Source | Related Story |
|----|------------|---------------------|--------|---------------|
| NFR-PA1 | Bot must run on Linux as the sole production target | Process management via `pkill` (process name), logs to `./tmp/` | `AGENTS.md` | all |
| NFR-PA2 | Zero local disk writes for persistent state | All state on NextCloud WebDAV; only transient logs go to `./tmp/` | `AGENTS.md`, `README.md` | all |
| NFR-PA3 | Config path must be set via environment variable only (no CLI argument) | `CONFIG_FILE` env var, default `config.toml` | `AGENTS.md` | all |
| NFR-PA4 | TLS must use pure Rust (rustls), not OpenSSL bindings | `rustls-tls-native-roots` feature on `tokio-tungstenite` | `AGENTS.md` | #1, #2 |
| NFR-PA5 | Bot must be startable via a single command with nohup for daemonization | `nohup ./target/release/rockbot < /dev/null > ./tmp/rockbot.log 2>&1 &` | `AGENTS.md` — Phase 3 | all |
| NFR-PA6 | Restart must be a single pkill + re-launch sequence, using process name (not `-f`) | `pkill rockbot` (process name); `pkill -f` prohibited (can hang on D-state threads) | `AGENTS.md` — Phase 3 | all |
| NFR-PA7 | Max retry exhaustion must exit the process (not hang) for supervisor restart | Reconnect max 5 retries → `GracefulShutdown`; external supervisor (systemd/docker) handles restart | `AGENTS.md` | all |
| NFR-PA8 | Process must shut down gracefully on OS signals | SIGTERM/SIGINT captured → shutdown sequence (flush dirty snapshots, close WebSocket) | `_dfds/agent-loop.md` 2b | all |

## 8. Observability

| ID | Requirement | Measure / Threshold | Source | Related Story |
|----|------------|---------------------|--------|---------------|
| NFR-OB1 | RocketChat DDP traffic must support verbose debug logging | `[rocketchat.server] debug = true/false` config toggle | `example.config.toml:64` | #1, #2 |
| NFR-OB2 | All log output must go to a project-local file | `&> ./tmp/rockbot.log` redirection in start script | `AGENTS.md` | all |
| NFR-OB3 | Snapshot dirty state must be observable at runtime | `MemoryManager.dirty_snapshots: HashSet<String>` — set of room IDs needing rebuild | `_dfds/base/memory.md` 3 | #8 |
| NFR-OB4 | Snapshot schema must be versioned for future migration | `PersistSnapshot.schema: "rockbot-snapshot/1"` | `_dfds/base/memory.md` 3 | #8 |
| NFR-OB5 | Knowledge index must be versioned for future migration | `KnowledgeIndex.version: "rockbot-knowledge/1"` | `_dfds/base/knowledge.md` 3 | #7 |

## 9. Testability

| ID | Requirement | Measure / Threshold | Source | Related Story |
|----|------------|---------------------|--------|---------------|
| NFR-TS1 | Unit tests must run offline without credentials or network | `cargo test` runs **259** non-ignored tests in isolation | `_docs/test_suite/running.md` | all |
| NFR-TS2 | HTTP-dependent tests must use `wiremock`, not real servers | **37** wiremock tests for DeepSeek, OpenRouter, and WebDAV provider behavior | `_docs/test_suite/README.md` | #1, #2, #3, #4, #9 |
| NFR-TS3 | AI provider tests must support an inline `MockProvider` | **5** harness tests using `MockProvider` to simulate completion responses | `_docs/test_suite/README.md` | #1, #2 |
| NFR-TS4 | Tool tests must support an inline `MockTool` | **6** registry tests using `MockTool` to verify tool dispatch | `_docs/test_suite/README.md` | #3–#10 |
| NFR-TS5 | Real integration tests (requiring live services) must be marked `#[ignore]` | **7** ignored tests requiring running servers or credentials | `_docs/test_suite/README.md` | #1, #2, #9 |
| NFR-TS6 | Real integration tests must source credentials from environment variables, never hardcoded | `WEBDAV_URL`, `WEBDAV_USERNAME`, `WEBDAV_PASSWORD`, `config.toml` | `_docs/test_suite/running.md` | #9 |
| NFR-TS7 | Test suite must cover all 3 crates with a minimum threshold | **266 tests** total: 138 unit, 121 integration, 7 ignored; across webdav (49), rocketchat (40), rockbot (177) | `_docs/test_suite/README.md` | all |

---

## Configuration-Driven Tuning Reference

The following NFR thresholds are user-configurable via `config.toml`:

| Parameter | Default | Config Section | NFR ID |
|-----------|---------|----------------|--------|
| `max_iterations` | 8 | `[rocketchat.model]` | NFR-PE1 |
| `max_history_size` | 12 | `[rocketchat.model]` | NFR-PE3 |
| `max_text_length` | 50,000 chars | `[rocketchat.model]` | NFR-PE3 |
| `max_summary_chars` | 8,000 chars | `[rocketchat.model]` | NFR-PE4 |
| `max_soul_chars` | 2,000 chars | `[rocketchat.model]` | NFR-PE5 |
| `summary_days` | 7 days | `[rocketchat.model]` | NFR-SC5 |
| `memory_ttl_secs` | 300s (5 min) | `[rocketchat.model]` | NFR-SC2 |
| `debug` | false | `[rocketchat.server]` | NFR-OB1 |

---

## Operational Conventions

These are project-specific rules from `AGENTS.md` that govern day-to-day operations.
They are not quality attributes of the system itself but are required for correct
operation.

| Rule | Description | Source |
|------|-------------|--------|
| `./tmp/` only | All runtime temp files (logs, state) use `./tmp/` — never `/tmp/` | `AGENTS.md` |
| `pkill` by process name | Kill bot with `pkill rockbot` (process name), never `pkill -f` | `AGENTS.md` |
| `Cargo.lock` gitignored | Not committed — prevents lockfile conflicts across dev machines | `AGENTS.md` |
| Room key naming | `r-{fname}` for channels, `d-{username}` for DMs — both use friendly name when available | `_dfds/base/rocketchat.md` 3 |
| `room_id` ≠ `webdav_dir` | RocketChat UUID is in-memory lookup key; path key is a separate `r-`/`d-`-prefixed string | `_dfds/base/rocketchat.md` 3 |
| Reactive ping only | Bot never proactively sends WebSocket pings; only responds to server pings | `_dfds/base/rocketchat.md` 2d |
| EDITME key rejection | `EDITME` placeholders in config must be rejected at provider construction time | `_docs/test_suite/README.md` |
