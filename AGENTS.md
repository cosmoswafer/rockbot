# AGENTS.md — rockbot

## User directives

- **Ignore all Python code.** Do not read, edit, or reason about `bot/`, `util/`, `rock.py`, `requirements.txt`, or any `.py` files.

## Project layout

```
crate-rocketchat/     # lib + debug binary — standalone RocketChat DDP WebSocket client
crate-rockbot/        # lib + application binary — config, AiProvider trait, agent loop, tools, memory
crate-webdav/         # lib only — WebDAV client for NextCloud storage operations
bot/                  # Python app (IGNORED per user directive)
_dfds/ _docs/         # Mermaid data flow diagrams and architecture docs
example.config.toml   # template for config; real config.toml is gitignored
```

## Runtime

- Use `./tmp/` for runtime temporary files (logs, state, etc.). Never use `/tmp/` or other system-wide temp directories.
- Start the bot in background:

  ```bash
  nohup ./target/release/rockbot < /dev/null > ./tmp/rockbot.log 2>&1 &
  ```

- Restart the bot:

  ```bash
  pkill rockbot 2>/dev/null; sleep 1; nohup ./target/release/rockbot < /dev/null > ./tmp/rockbot.log 2>&1 &
  ```

  Note: Use `pkill rockbot` (by process name) — **not** `pkill -f` (full cmdline). The `-f` flag reads `/proc/*/cmdline` which can hang on systems with stuck D-state kernel threads.

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
```

No CI, no `rustfmt.toml`, no `clippy.toml`, no `rust-toolchain` file.

## Code style

- **Use async Rust everywhere.** The only exception is `crate-rocketchat/src/main.rs` (debug binary) which uses a sync `fn main` with `block_on`.
- **Edition 2024**, MSRV **1.85**. Use modern Rust (`impl Trait` in return position allowed).
- Prefer `tokio` as the async runtime. All I/O (HTTP, WebSocket, file, subprocess) must be async.

## Key facts

- `Cargo.lock` is gitignored. Do not create or commit it.
- `config.toml` is gitignored; use `example.config.toml` as a reference. Config uses `[rocketchat.server]` + `[rocketchat.model]` sub-sections and `[[providers]]` (TOML array-of-tables), **not** the old Python `config.json` format.
- `CONFIG_FILE` env var sets the config path; defaults to `config.toml` (not a CLI argument).
- The `rocketchat` crate has both `lib.rs` (public API) and `main.rs` (manual debug binary that connects to a RocketChat server and logs events — no real bot logic).
- The `rocketchat` crate uses `thiserror`, `serde`/`serde_json`, `tokio-tungstenite` with `rustls-tls-native-roots` for WebSocket TLS.
- The `rockbot` crate uses `async-trait` for the `AiProvider` trait (OpenRouter, DeepSeek, Fal).
- Exa API key is read from `[tools.exa]` config section first, then falls back to `EXA_API_KEY` env var.
- Tools are registered conditionally: `WebDavTool` and `ImageGenTool` only if WebDAV is configured; `ImageGenTool` also requires a `fal` provider in config.
- The main loop has exponential backoff reconnect (2^attempt seconds, max 5 retries, then exits).
- The `webdav` crate uses `quick-xml` and `base64` for WebDAV XML parsing and auth.

## DFD-driven implementation

Data Flow Diagrams in `_dfds/` define the system's architecture. When a DFD is modified, align the Rust implementation to match:

1. **Read the changed DFD** to understand the updated data flow, process steps, and decision nodes.
2. **Map DFD processes to source files** — each DFD rectangle/process node typically corresponds to a function, method, or module. Use the DFD annotations (e.g. references to `src/harness.rs`, `src/memory.rs`) as entry points.
3. **Implement in iteration order** — follow the DFD flow top-to-bottom. Start with data structure changes (`types.rs`, `config.rs`), then the core logic (harness, provider, tools), then wiring (`main.rs`, `lib.rs`).
4. **Add/update tests** in the corresponding test file (inline `#[cfg(test)]` modules for unit tests, `tests/*.rs` for integration). Mock external dependencies (HTTP, WebDAV, RocketChat) — see existing `wiremock` patterns in `integration_mock.rs`.
5. **Verify with `cargo test`** — all tests must pass before committing. Run `cargo fmt` and `cargo clippy` to keep code clean.

### DFD-to-code mapping

| DFD file | Primary Rust source | Secondary sources |
| -------- | ------------------- | ----------------- |
| `agent-harness.md` | `harness.rs` | `memory.rs`, `tool.rs`, `provider/mod.rs` |
| `agent-loop.md` | `main.rs` | `harness.rs`, `config.rs` |
| `base/ai-provider.md` | `provider/mod.rs`, `provider/deepseek.rs`, `provider/openrouter.rs`, `provider/fal.rs` | `types.rs` |
| `base/config.md` | `config.rs` | `example.config.toml` |
| `base/memory.md` | `memory.rs` | `harness.rs`, webdav crate (`client.rs`, `path.rs`) |
| `base/rocketchat.md` | rocketchat crate (`client.rs`, `ddp.rs`, `types.rs`) | — |
| `tools/webdav.md` | `tools/webdav.rs` | webdav crate (`client.rs`, `path.rs`) |
| `tools/calendar.md` | `tools/calendar.rs` | webdav crate (`client.rs`, `path.rs`) |
| `tools/exa-search.md` | `tools/web_search.rs` | `tools/web_fetch.rs` |
| `tools/web-fetch.md` | `tools/web_fetch.rs` | `tools/web_search.rs` |
| `base/knowledge.md` | *(planned — no implementation)* | — |
| `context-diagram.md` | (Level 0 — system boundary, no code changes) | — |

## Testing

Test suite inventory and run instructions are in [`_docs/test_suite/running.md`](_docs/test_suite/running.md) and [`_docs/test_suite/README.md`](_docs/test_suite/README.md).

## OpenCode skills

- `dfd-md` — Creates Data Flow Diagrams as `.md` files using Mermaid flowchart syntax.
- `mermaid-cli` — Validates Mermaid syntax using `mermaid.parse()` with jsdom (no browser). Use only when asked to validate/fix Mermaid syntax.
