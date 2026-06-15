# Non-Functional Requirements — RockBot

## 1. Performance Efficiency
- **PE1** Agent loop terminates within `max_iterations` (code default 28, configurable in `config.toml`).
- **PE2** Web fetch times out at 30s.
- **PE3** Chat context: hardcoded `MAX_CONTEXT_MESSAGES` = 30 messages in context window (not configurable), `max_context_bytes` default 4 MB. Context-length errors trigger compression + hard-truncation to last 2 messages + 200K char per-message truncation.
- **PE4** Soul memory capped at `max_soul_chars` (default 2000 chars, configurable).
- **PE5** Snapshot writes coalesced every `persist_interval_secs` (default 60s).
- **PE6** Exa search: default 5 results (max 20), highlights always enabled, text mode maxCharacters 15K, 3 retries with exponential backoff starting at 1s (1s → 2s → 4s).
- **PE7** Web fetch body truncated at 10K chars (all output modes).
- **PE8** Knowledge context cache TTL: 60s (`KNOWLEDGE_CACHE_TTL_SECS`).
- **PE9** Model context length configurable via `model_context_length` (code default 1M tokens).

## 2. Reliability
- **RL1** WebSocket reconnect: exponential backoff (2^attempt seconds), max 5 retries, then flush snapshots and exit.
- **RL2** AI provider errors: 3 retries for 429/5xx with exponential backoff (2s, 4s, 8s); 401 and all other errors fail immediately.
- **RL3** WebDAV write auto-creates parent dirs: tries NextCloud `X-NC-WebDAV-AutoMkcol` header first, falls back to explicit `MKCOL` per-directory-level.
- **RL4** Snapshot write failures don't block agent loop (dirty flag retries on next maintenance tick).
- **RL5** Soul write failures warn + continue; snapshot marked dirty regardless.
- **RL6** Room state survives restart: cache-first `snapshot.json` (single GET), fallback to individual `soul.md` + `summary.md` files.
- **RL7** DDP subscription loss (`nosub` message) triggers automatic re-subscribe (unlimited retries within connection lifetime).
- **RL8** Tool execution errors produce `ToolResult` with `is_error: true` (fed back to LLM, not crash).
- **RL9** AI summary failure falls back to static `"{count} messages compressed"` message.
- **RL10** Knowledge load failure on room init skips knowledge gracefully (warn + continue).
- **RL11** Max iterations exceeded appends apology fallback message and returns to user.
- **RL12** Knowledge forget returns error to LLM if entry not found; underlying WebDAV delete is idempotent (accepts 404).
- **RL13** CalDAV update sends `If-Match` header with ETag; no automatic refetch+merge+retry on conflict.

## 3. Security
- **SC1** Credentials in gitignored `config.toml` only.
- **SC2** RocketChat password SHA-256 hashed before DDP transmission (`ddp.rs:sha256_digest`).
- **SC3** WebDAV credentials via HTTP Basic auth over TLS.
- **SC4** WebSocket uses TLS (wss://) only.
- **SC5** Provider API keys in HTTP headers only (never query/body).
- **SC6** `config.toml` and `Cargo.lock` gitignored.
- **SC7** Exa API key supports `EXA_API_KEY` env var fallback.
- **SC8** Bot never responds to its own messages (`MessageFilter` discards by `sender_id`).
- **SC9** Secret values never exposed to LLM — `secrets.toml` sanitization replaces all `value` fields with `"abcd"` when read via webdav tool. Secret UUIDs (`secret:<UUID>`) resolved transparently at tool-call time.

## 4. Maintainability
- **MN1** 3 workspace crates: rocketchat, rockbot, webdav.
- **MN2** AI chat providers implement shared `AiProvider` trait; image providers implement `ImageProvider` trait.
- **MN3** Tools implement shared `Tool` trait, registered in `ToolRegistry`.
- **MN4** Registration config-driven (conditional on config sections: WebDAV, image_providers, exa).
- **MN5** Typed `serde` deserialization with nested sub-structs and `serde_valid` / `validator` at boundaries.
- **MN6** All I/O async via tokio.
- **MN7** DFDs are source of truth (not reverse-engineered).
- **MN8** Mermaid syntax validated before commit.
- **MN9** MSRV 1.85, Edition 2024.
- **MN10** No CI, no rustfmt/clippy config, no rust-toolchain.

## 5. Compatibility
- **CM1** AI provider APIs are OpenAI-compatible.
- **CM2** RocketChat via DDP over WebSocket.
- **CM3** WebDAV targets NextCloud API.
- **CM4** Calendar follows RFC 4791 (CalDAV) and RFC 5545 (iCalendar).
- **CM5** Config uses TOML format (not JSON).
- **CM6** DDP deprecation risk accepted; no Apps-Engine migration.

## 6. Scalability
- **SC1** Per-room memory fully isolated (separate WebDAV dirs: `r-{name}` for channels, `d-{name}` for DMs).
- **SC2** Idle rooms evicted after `memory_ttl_secs` (default 300s).
- **SC3** Evicted rooms restore with single WebDAV GET (`snapshot.json`).
- **SC4** Snapshot is performance cache; individual files are source of truth.
- **SC5** WebDAV `WebDavClient` cloned and shared across tools; HTTP clients per-module (not pooled).
- **SC6** Calendar events per-room scoped (separate CalDAV calendar per room).

## 7. Portability & Availability
- **PA1** Linux only.
- **PA2** Zero local disk writes (all state on WebDAV; only reads config files at startup). `./tmp/rockbot.log` is a shell redirect, not a code path.
- **PA3** Config path via `CONFIG_FILE` env var only (falls back to `config.toml` in cwd).
- **PA4** TLS via rustls (no OpenSSL).
- **PA5** Single nohup command for daemonization.
- **PA6** Restart via `pkill rockbot` (process name, never `-f`).
- **PA7** Max retry exhaustion exits for supervisor restart.
- **PA8** SIGTERM/SIGINT triggers graceful shutdown (flushes all dirty snapshots first).

## 8. Observability
- **OB1** DDP debug logging via `[rocketchat.server] debug` config toggle.
- **OB2** Structured tracing output to stderr (log level via `RUST_LOG` env var; `./tmp/rockbot.log` is a shell redirect, not code path).
- **OB3** Snapshot dirty state observable via `dirty_snapshots` set.
- **OB4** Snapshot schema versioned (`rockbot-snapshot/1`).
- **OB5** Knowledge index versioned (`rockbot-knowledge/1`).

## 9. Testability
- **TS1** 690 total tests across all 3 crates (unit, integration, and real).
- **TS2** HTTP-dependent tests use wiremock (2 integration test files).
- **TS3** AI provider integration tests use MockProvider (harness.rs test module, 27 references).
- **TS4** Tool tests use MockTool (tool.rs test module).
- **TS5** Real integration tests marked `#[ignore]` (25 tests).
- **TS6** Integration tests source creds from env vars only.
- **TS7** Test distribution: ~532 rockbot, ~89 rocketchat, ~69 webdav.
