# AGENTS.md — rockbot

## Project layout

```
crate-rocketchat/     # lib + debug binary — standalone RocketChat DDP WebSocket client
crate-rockbot/        # lib + application binary — config, AiProvider trait, agent loop, tools, memory
crate-webdav/         # lib only — WebDAV client for NextCloud storage operations
_dfds/ _docs/         # Mermaid data flow diagrams and architecture docs
example.config.toml   # template for config; real config.toml is gitignored
```

## Runtime

- Use `./tmp/` for runtime temporary files (logs, state, etc.). Never use `/tmp/` or other system-wide temp directories.
- Start the bot: `./target/release/rockbot &> ./tmp/rockbot.log &`
- Restart: see Phase 6 — Ship (step 4) for the two-call pattern.
- Restart with debug: see Phase 6 — Ship (step 5).
- Use `pkill rockbot` (process name) — **not** `pkill -f` (full cmdline). The `-f` flag reads `/proc/*/cmdline` which can hang on systems with stuck D-state kernel threads.
- **Bot must run in background** — all start/restart commands end with `&`. When using the Bash tool, run `nohup ... &` alone (never chain after `;` or `&&`), then verify with a separate call.

## Build & test

```bash
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
- **"Parse, don't validate"** — make invalid states unrepresentable through types. Parse at boundaries (config loading, JSON deserialization, CLI args) into domain types once; the rest of the system works with infallible, type-safe data. Validate at the edge, use strong types internally.
- **High cohesion, low coupling** — each module/crate has a single well-defined responsibility. Modules communicate through narrow, explicit interfaces (traits, public functions returning typed data). Avoid implicit or hidden dependencies across crate boundaries.
- **Errors via `thiserror` + `?`** — use `thiserror` for error types, propagate with `?`. Avoid `unwrap()` and `expect()` in production code; reserve panics for truly unrecoverable states.
- **Ownership-first** — prefer `&T`/`&str` for transient data, `Arc<str>` or `String` where ownership is required. Avoid unnecessary cloning.

## Key facts

- `Cargo.lock` is gitignored. Do not create or commit it.
- `config.toml` is gitignored; use `example.config.toml` as a reference. Config uses `[rocketchat.server]` + `[rocketchat.model]` sub-sections, `[[chat_providers]]` for LLM backends, and `[[image_providers]]` for image generation (TOML array-of-tables), **not** the old Python `config.json` format.
- `CONFIG_FILE` env var sets the config path; defaults to `config.toml` (not a CLI argument).
- The `rocketchat` crate has both `lib.rs` (public API) and `main.rs` (manual debug binary that connects to a RocketChat server and logs events — no real bot logic).
- The `rocketchat` crate uses `thiserror`, `serde`/`serde_json`, `tokio-tungstenite` with `rustls-tls-native-roots` for WebSocket TLS.
- The `rockbot` crate uses `async-trait` for the `AiProvider` trait (OpenRouter, DeepSeek, Fal).
- Exa API key is read from `[tools.exa]` config section first, then falls back to `EXA_API_KEY` env var.
- Tools are registered conditionally: `WebDavTool` and `ImageGenTool` only if WebDAV is configured; `ImageGenTool` also requires an `image_provider` entry in config (uses `FalAiProvider` internally regardless of provider name).
- The main loop has exponential backoff reconnect (2^attempt seconds, max 5 retries, then exits).
- The `webdav` crate uses `quick-xml` and `base64` for WebDAV XML parsing and auth.

## DFD-driven implementation

Data Flow Diagrams in `_dfds/` define the system's architecture. The full
development flow follows the DFD Dev Flow defined in the
[`dfd-md` skill](.opencode/skills/dfd-md/SKILL.md):

### Phase 1 — Revise DFD

Design or update the DFD so it accurately models the desired data movement.
Follow the "multiple small happy flows + Level 2 detail diagrams" composition
pattern. Use data-structure coupling for cross-DFD links.

### Phase 2 — Real integration test (data collection)

Write a real integration test (no mocking; targets a live server, API, or
resource) and run it to collect actual data shapes. This verifies the DFD
flows work end-to-end against reality and provides reference data for the
implementation phase.

### Phase 3 — Concrete implementation

Code the types, core logic, and wiring described by the DFD. Follow
"parse, don't validate" and high-cohesion low-coupling principles. Prefer
incremental, type-first implementation.

### Phase 4 — Comprehensive test suite

Build and run the three test layers until every test passes:

| Layer | Name | Description |
| ----- | ---- | ----------- |
| **Core** | Core test suite | Per-DFD, fine-grained tests against a single diagram's processes and data structures — analogous to unit tests. Each DFD gets its own core tests. |
| **User** | User test suite | Tests driven by a user story, exceptional event, or end-to-end scenario — analogous to system/integration tests. Verifies multiple DFDs work together to satisfy a real usage narrative. |
| **Real** | Real test suite | Real integration tests against live resources or servers (no mocking). Usually `#[ignore]`-ed and run only on explicit request. Used to collect real-world data for reference during development or debugging. |

### Phase 5 — Review all DFDs

Once the implementation and test suite are stable, re-read every DFD in the
project and confirm it still matches the code. Update any DFD that has drifted.

**DFD-driven alignment**: DFDs are the design spec. If a DFD's modification time
is newer than its corresponding Rust source, the code is stale and must be
updated to match the DFD. If the code was updated first (e.g., a bug fix),
update the DFD to match the code — DFD and code must always be in sync.

### Phase 6 — Ship

1. **Build release**: `cargo build --release`
2. **Commit**: `git add -A` and `git commit` with a descriptive message.
3. **Push**: `git push`
4. **Restart bot** (two separate Bash calls — never chain after `nohup ... &`):
   - `pkill rockbot 2>/dev/null; rm -f ./tmp/rockbot.log`
   - `nohup ./target/release/rockbot < /dev/null > ./tmp/rockbot.log 2>&1 &`
5. **Restart with debug** (two separate Bash calls):
   - `pkill rockbot 2>/dev/null; rm -f ./tmp/rockbot.log`
   - `RUST_LOG=debug nohup ./target/release/rockbot < /dev/null > ./tmp/rockbot.log 2>&1 &`

### DFD-to-code mapping

| DFD file | Primary Rust source | Secondary sources |
| -------- | ------------------- | ----------------- |
| `agent-harness.md` | `harness.rs` | `memory.rs`, `tool.rs`, `provider/mod.rs` |
| `agent-loop.md` | `main.rs` | `harness.rs`, `config.rs` |
| `base/ai-provider.md` | `provider/mod.rs`, `provider/deepseek.rs`, `provider/openrouter.rs`, `provider/fal.rs` | `types.rs` |
| `base/config.md` | `config.rs` | `example.config.toml` |
| `base/memory.md` | `memory.rs` | `harness.rs`, webdav crate (`client.rs`, `path.rs`) |
| `base/rocketchat.md` | rocketchat crate (`client.rs`, `ddp.rs`, `types.rs`) | — |
| `base/rocketchat-rest.md` | rocketchat crate (`rest.rs`), `harness.rs` | `client.rs` (token capture), webdav crate |
| `tools/webdav.md` | `tools/webdav.rs` | webdav crate (`client.rs`, `path.rs`) |
| `tools/calendar.md` | `tools/calendar.rs` | webdav crate (`client.rs`, `path.rs`) |
| `tools/exa-search.md` | `tools/web_search.rs` | `tools/web_fetch.rs` |
| `tools/web-fetch.md` | `tools/web_fetch.rs` | `tools/web_search.rs` |
| `tools/image-gen.md` | `tools/image_gen.rs` | `provider/fal.rs`, `webdav` crate |
| `tools/vision.md` | `tools/vision.rs` | — |
| `tools/datetime.md` | `tools/datetime.rs` | — |
| `tools/edit-soul.md` | `tools/edit_soul.rs` | `memory.rs`, `webdav` crate |
| `tools/knowledge.md` | `tools/save_knowledge.rs`, `tools/forget_knowledge.rs`, `tools/recall_knowledge.rs` | `knowledge.rs`, `webdav` crate |
| `base/knowledge.md` | `knowledge.rs` | `tools/save_knowledge.rs`, `tools/forget_knowledge.rs`, `tools/recall_knowledge.rs` |
| `base/knowledge-priority.md` | `knowledge.rs` | `harness.rs`, `memory.rs` |
| `context-diagram.md` | (Level 0 — system boundary, no code changes) | — |
| `image-interception.md` | `harness.rs` | `tools/image_gen.rs`, `tools/vision.rs`, `tools/webdav.rs`, `provider/fal.rs` |

## Testing

Test suite inventory and run instructions are in [`_docs/test_suite/running.md`](_docs/test_suite/running.md) and [`_docs/test_suite/README.md`](_docs/test_suite/README.md).

## OpenCode skills

- `dfd-md` — Creates Data Flow Diagrams as `.md` files using Mermaid flowchart syntax.
- `mermaid-cli` — Validates Mermaid syntax using `mermaid.parse()` with jsdom (no browser). Use only when asked to validate/fix Mermaid syntax.
