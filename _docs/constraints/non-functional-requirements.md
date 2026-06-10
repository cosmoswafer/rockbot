# Non-Functional Requirements — RockBot

## 1. Performance Efficiency
- **PE1** Agent loop terminates within `max_iterations` (default 8).
- **PE2** Web fetch times out at 30s.
- **PE3** Chat history capped at 50K chars / 12 messages per room.
- **PE4** Summaries capped at 8K chars.
- **PE5** Soul memory capped at 2K chars.
- **PE6** Snapshot writes coalesced every 60s.
- **PE7** Exa results: 5 results, 10K chars per highlight.
- **PE8** Web fetch body truncated at 10K chars (json mode).

## 2. Reliability
- **RL1** WebSocket reconnect: exponential backoff, max 5 retries, then exit.
- **RL2** AI provider errors: 5xx/429 retry with backoff, 401 immediate fail.
- **RL3** WebDAV auto-creates parent dirs on write.
- **RL4** Snapshot write failures don't block agent loop (dirty flag retries).
- **RL5** Soul write failures warn + continue.
- **RL6** Room state survives restart: cache-first snapshot.json, fallback to individual files.
- **RL7** DDP subscription loss triggers re-subscribe.
- **RL8** Tool execution errors produce fallback reply (not crash).
- **RL9** AI summary failure truncates oldest messages without summary.
- **RL10** Knowledge load failure on room init skips knowledge gracefully.
- **RL11** Max iterations exceeded forces final reply.
- **RL12** Knowledge forget is idempotent.
- **RL13** CalDAV 409 ETag mismatch → refetch + merge + retry.

## 3. Security
- **SC1** Credentials in gitignored `config.toml` only.
- **SC2** RocketChat password SHA-256 hashed before DDP transmission.
- **SC3** WebDAV credentials via HTTP Basic auth over TLS.
- **SC4** WebSocket uses TLS (wss://) only.
- **SC5** Provider API keys in HTTP headers only (never query/body).
- **SC6** `config.toml` and `Cargo.lock` gitignored.
- **SC7** Exa API key supports `EXA_API_KEY` env var fallback.
- **SC8** Bot never responds to its own messages.

## 4. Maintainability
- **MN1** 3 workspace crates: rocketchat, rockbot, webdav.
- **MN2** AI providers implement shared `AiProvider` trait.
- **MN3** Tools implement shared `Tool` trait, registered in `ToolRegistry`.
- **MN4** Registration config-driven (conditional on config sections).
- **MN5** Typed `serde` deserialization with nested sub-structs.
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
- **SC1** Per-room memory fully isolated (separate WebDAV dirs).
- **SC2** Idle rooms evicted after 300s TTL.
- **SC3** Evicted rooms restore with single WebDAV GET.
- **SC4** Snapshot is performance cache; individual files are source of truth.
- **SC5** Old daily summaries auto-deleted (7-day rolling window).
- **SC6** Shared HTTP connection pool for WebDAV.
- **SC7** Calendar events globally scoped (not per-room).

## 7. Portability & Availability
- **PA1** Linux only.
- **PA2** Zero local disk writes (all state on WebDAV).
- **PA3** Config path via `CONFIG_FILE` env var only.
- **PA4** TLS via rustls (no OpenSSL).
- **PA5** Single nohup command for daemonization.
- **PA6** Restart via `pkill rockbot` (process name, never `-f`).
- **PA7** Max retry exhaustion exits for supervisor restart.
- **PA8** SIGTERM/SIGINT triggers graceful shutdown.

## 8. Observability
- **OB1** DDP debug logging via `[rocketchat.server] debug` toggle.
- **OB2** Logs to `./tmp/rockbot.log`.
- **OB3** Snapshot dirty state observable via `dirty_snapshots` set.
- **OB4** Snapshot schema versioned (`rockbot-snapshot/1`).
- **OB5** Knowledge index versioned (`rockbot-knowledge/1`).

## 9. Testability
- **TS1** Unit tests run offline (259 tests, no network).
- **TS2** HTTP-dependent tests use wiremock (37 tests).
- **TS3** AI provider tests use MockProvider (5 tests).
- **TS4** Tool tests use MockTool (6 tests).
- **TS5** Real integration tests marked `#[ignore]` (7 tests).
- **TS6** Integration tests source creds from env vars only.
- **TS7** 266 tests across all 3 crates.
