# AGENTS.md — rockbot

## User directives

- **Ignore all Python code.** Do not read, edit, or reason about `bot/`, `util/`, `rock.py`, `requirements.txt`, or any `.py` files.

## Project layout

```
crate-rocketchat/     # lib + debug binary — standalone RocketChat DDP WebSocket client
crate-rockbot/        # lib only — config, AiProvider trait, agent loop, tools, memory
crate-webdav/         # lib only — WebDAV client for NextCloud storage operations
bot/                  # Python app (IGNORED per user directive)
_dfds/ _docs/         # Mermaid data flow diagrams and architecture docs
example.config.toml   # template for config; real config.toml is gitignored
```

The `crate-rockbot` application binary (`main.rs`) does not exist yet — the app binary is still Python.

## Build & test

```bash
cargo build --release                # workspace build (3 crates)

# Run all unit + non-ignored integration tests:
cargo test                           # from workspace root

# Run tests for a single crate:
cargo test -p rocketchat
cargo test -p rockbot
cargo test -p webdav

# Real integration tests (require credentials / running servers):
cargo test -- --ignored              # all ignored tests across workspace

# Specific ignored test files:
cargo test -p rocketchat --test integration_real -- --ignored   # needs config.toml + RC server
cargo test -p webdav --test integration_real -- --ignored       # needs WEBDAV_* env vars
```

No CI, no `rustfmt.toml`, no `clippy.toml`, no `rust-toolchain` file.
Run `cargo fmt` and `cargo clippy` with default settings.

## Code style

- **Use async Rust everywhere.** The only exception is `crate-rocketchat/src/main.rs` (debug binary) which uses a sync `fn main` with `block_on`.
- **Edition 2024**, MSRV **1.85**. Use modern Rust (`impl Trait` in return position allowed).
- Prefer `tokio` as the async runtime. All I/O (HTTP, WebSocket, file, subprocess) must be async.

## Key facts

- `Cargo.lock` is gitignored. Do not create or commit it.
- `config.toml` is gitignored; use `example.config.toml` as a reference. Config format uses `[rocketchat]` and `[[providers]]` (TOML array-of-tables), not `[server]`/`[ai]`.
- The `rocketchat` crate has both `lib.rs` (public API) and `main.rs` (manual debug binary that connects to a RocketChat server and logs events — no real bot logic).
- The `rocketchat` crate uses `thiserror`, `serde`/`serde_json`, `tokio-tungstenite` with `rustls-tls-native-roots` for WebSocket TLS.
- The `rockbot` crate uses `async-trait` for the `AiProvider` trait (OpenRouter, DeepSeek).
- The `webdav` crate uses `quick-xml` and `base64` for WebDAV XML parsing and auth.

## Testing

- `crate-rocketchat/tests/integration.rs` — message parsing and filtering tests (no server needed).
- `crate-rocketchat/tests/integration_real.rs` — `#[ignore]`d. Requires `config.toml` in workspace root with `[rocketchat]` section and a running RocketChat instance.
- `crate-webdav/tests/integration.rs` + `config_tests.rs` — client construction, path helpers, config deserialization (no server needed).
- `crate-webdav/tests/integration_real.rs` — `#[ignore]`d. Requires `WEBDAV_URL`, `WEBDAV_USERNAME`, `WEBDAV_PASSWORD` env vars (no config file!).
- `crate-rockbot/tests/provider_tests.rs` — config parsing and provider logic (no server needed).
- `crate-rockbot/tests/integration_mock.rs` — `wiremock`-based HTTP mock tests for provider clients (no server needed).

## OpenCode skills

- `dfd-md` — Creates Data Flow Diagrams as `.md` files using Mermaid flowchart syntax.
- `mermaid-cli` — Validates Mermaid syntax using `mermaid.parse()` with jsdom (no browser). Use only when asked to validate/fix Mermaid syntax.
