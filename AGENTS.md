# AGENTS.md — rockbot

## Project layout

```
crate-rocketchat/     # lib + debug binary — standalone RocketChat DDP WebSocket client
crate-rockbot/        # lib + application binary — config, AiProvider trait, agent loop, tools, memory
crate-webdav/         # lib only — WebDAV client for NextCloud storage operations
_dfds/                # Mermaid data flow diagrams (design spec)
_docs/                # constraints, test suite inventory
.agents/skills/       # OpenCode skill definitions (dfd-md, mermaid-cli)
default.config.toml   # full config with all defaults (empty credentials)
example.config.toml   # minimal user overrides with EDITME placeholders
```

## Runtime

- Use `./tmp/` for runtime temporary files (logs, state, etc.). Never use `/tmp/` or other system-wide temp directories.
- Use `pkill rockbot` (process name) — **not** `pkill -f` (full cmdline). The `-f` flag reads `/proc/*/cmdline` which can hang on systems with stuck D-state kernel threads.
- **Bot must run in background** — all start/restart commands end with `&`. When using the Bash tool, run `nohup ... &` alone (never chain after `;` or `&&`), then verify with a separate call.
- Start the bot: `./target/release/rockbot &> ./tmp/rockbot.log &`
- Restart (two separate Bash calls — never chain after `nohup ... &`):
  1. `pkill rockbot 2>/dev/null; rm -f ./tmp/rockbot.log`
  2. `nohup ./target/release/rockbot < /dev/null > ./tmp/rockbot.log 2>&1 &`
- Restart with debug: same pattern, prepend `RUST_LOG=debug` to the `nohup` line.

## Build & test

```bash
cargo build --release               # workspace build (3 crates)
cargo test                          # all unit + mock integration tests
cargo test -p rocketchat            # single crate
cargo test -p rockbot
cargo test -p webdav
cargo test -- --ignored             # live data integration probes (needs config.toml credentials)
cargo test --test integration_real -- --ignored   # single probe file (ignored)
RUST_LOG=debug cargo test -p rocketchat --test integration_real -- --ignored --nocapture  # with logging
```

No CI, no `rustfmt.toml`, no `clippy.toml`, no `rust-toolchain` file.

## Code style

- **Async Rust everywhere.** Only exception: `crate-rocketchat/src/main.rs` (debug binary) uses sync `fn main` with `block_on`.
- **Edition 2024**, MSRV **1.85**. Use modern Rust (`impl Trait` in return position allowed).
- **"Parse, don't validate"** — parse at boundaries (config, JSON, CLI args) into domain types once; the rest of the system works with infallible, type-safe data.
- **Errors via `thiserror` + `?`** — avoid `unwrap()` and `expect()` in production code.
- **Ownership-first** — prefer `&T`/`&str` for transient data, `Arc<str>` or `String` where ownership required.

## Key facts

- `Cargo.lock` is gitignored. Do not create or commit it.
- `config.toml` is gitignored. Two reference files exist:
  - `default.config.toml` — complete spec with all keys and default values (empty strings for credentials).
  - `example.config.toml` — minimal user override file with `EDITME` placeholders. Intended to be copied to `config.toml` and edited.
- `CONFIG_FILE` env var sets the config path; defaults to `config.toml` (not a CLI argument).
- Config uses TOML: `[rocketchat.server]` + `[rocketchat.model]` sub-sections, `[[chat_providers]]` and `[[image_providers]]` arrays-of-tables.
- `rocketchat` crate has both `lib.rs` (public API) and `main.rs` (debug binary — connects to RocketChat and logs events, no bot logic).
- `rockbot` crate uses `async-trait` for the `AiProvider` trait (implementations: OpenRouter, DeepSeek, Fal). Wiremock is available for mock HTTP testing.
- Exa API key: reads from `[tools.exa]` config section first, then falls back to `EXA_API_KEY` env var.
- Tools registered conditionally: `WebDavTool` and `ImageGenTool` only if WebDAV is configured; `ImageGenTool` also requires an `image_provider` entry (uses `FalAiProvider` internally regardless of provider name).
- Main loop: exponential backoff reconnect (2^attempt seconds, max 5 retries, then exits).
- `webdav` crate uses `quick-xml` and `base64` for WebDAV XML parsing and auth.

## DFD-driven development

Data Flow Diagrams in `_dfds/` are the design spec. The development flow is defined in the [`dfd-md` skill](.agents/skills/dfd-md/SKILL.md). Key rules:

- **Phase 1**: Integration probe (data collection; optional) — live-data probe against real server/API to collect actual data shapes. Skip if sufficient real-world data already exists.
- **Phase 2**: Revise DFD — design or update the DFD to accurately model desired data movement. Base data structures (section 3) on shapes observed in the probe when available. Keep levels clean; use notation rules from the skill.
- **Phase 3**: Implement data flow validation constraints — enforce data structure correctness through code-level constraints. Parse and validate at subsystem entry points ("parse, don't validate"). Cross-DFD shared structures defined once in a canonical location, imported by both producer and consumer modules, making mismatches compile-time errors.
- **Phase 4**: Concrete implementation — code types, core logic, and wiring described by the DFD. Favour incremental, type-first implementation.
- **Phase 5**: Review all DFDs — re-read every DFD and confirm it matches the code. If a DFD's `mtime` is newer than its corresponding Rust source, the code is stale and must be updated to match the DFD. If the code was updated first, update the DFD.
- **Phase 6**: Integration test — write mock-backed (Wiremock) integration tests to verify the implementation works end-to-end. Each DFD's happy-path flow should have corresponding mock integration coverage.
- **Phase 7**: `cargo build --release` → commit → push → restart bot.

### Rust type-driven design rules

Every DFD data structure (section 3) becomes a Rust type. Follow these rules
to make data flow violations compile-time errors rather than runtime surprises:

- **Newtype wrapping** — primitives carrying invariants (non-empty strings,
  bounded numbers, well-formed URLs, IDs) must be single-field structs with a
  fallible constructor (`TryFrom` / `FromStr` / factory fn). Use
  [`nutype`](https://crates.io/crates/nutype) for attribute-macro newtypes
  with built-in validation, or hand-roll with a private field. Holding an
  instance guarantees the invariant — no downstream `.is_valid()` checks.
- **Parse at boundaries** — all external input (JSON, TOML config, CLI args)
  is parsed into domain types once, at the subsystem entry point.  Use `serde`
  `Deserialize` on validated types directly; never pass `serde_json::Value`
  or raw `String` through internal layers.
- **Cross-DFD shared types** — a type consumed by multiple DFDs lives in a
  canonical crate or module.  Both producer and consumer import it — a field
  rename or type change becomes a compile-time error everywhere at once.
- **Errors via [`thiserror`](https://crates.io/crates/thiserror)** — every
  fallible constructor and parsing step returns a specific error type. Error
  messages name the DFD data structure and offending field.
- **No `unwrap()` / `expect()` in production** — use `?` exclusively. Panics
  are only for unrecoverable programmer bugs (broken invariants that indicate
  a logic error).

### DFD-to-code mapping

| DFD | Primary source | Key secondary sources |
| --- | -------------- | --------------------- |
| `_dfds/context-diagram.md` | Level 0 system boundary (no code) | — |
| `_dfds/base/config.md` | `config.rs` | `example.config.toml`, `default.config.toml` |
| `_dfds/base/rocketchat.md` | rocketchat (`client.rs`, `ddp.rs`, `types.rs`) | — |
| `_dfds/base/rocketchat-rest.md` | rocketchat (`rest.rs`), `harness.rs` | — |
| `_dfds/base/ai-provider.md` | `provider/mod.rs`, `provider/deepseek.rs`, `provider/openrouter.rs`, `provider/fal.rs` | `types.rs` |
| `_dfds/base/memory.md` | `memory.rs` | `harness.rs`, webdav crate |
| `_dfds/base/knowledge.md` | `knowledge.rs` | `tools/save_knowledge.rs`, `tools/forget_knowledge.rs`, `tools/recall_knowledge.rs` |
| `_dfds/base/knowledge-priority.md` | `knowledge.rs` | `harness.rs`, `memory.rs` |
| `_dfds/agent-loop.md` | `main.rs` | `harness.rs`, `config.rs` |
| `_dfds/agent-harness.md` | `harness.rs` | `memory.rs`, `tool.rs`, `provider/mod.rs` |
| `_dfds/image-interception.md` | `harness.rs` | `tools/image_gen.rs`, `tools/vision.rs`, `tools/webdav.rs`, `provider/fal.rs`, `image_cache.rs` |
| `_dfds/tools/webdav.md` | `tools/webdav.rs` | webdav crate |
| `_dfds/tools/calendar.md` | `tools/calendar.rs` | webdav crate |
| `_dfds/tools/exa-search.md` | `tools/web_search.rs` | `tools/web_fetch.rs` |
| `_dfds/tools/web-fetch.md` | `tools/web_fetch.rs` | `tools/web_search.rs` |
| `_dfds/tools/image-gen.md` | `tools/image_gen.rs` | `provider/fal.rs`, webdav crate |
| `_dfds/tools/vision.md` | `tools/vision.rs` | — |
| `_dfds/tools/datetime.md` | `tools/datetime.rs` | — |
| `_dfds/tools/edit-soul.md` | `tools/edit_soul.rs` | `memory.rs`, webdav crate |
| `_dfds/tools/knowledge.md` | `tools/save_knowledge.rs`, `tools/forget_knowledge.rs`, `tools/recall_knowledge.rs` | `knowledge.rs`, webdav crate |

## OpenCode skills

- `dfd-md` — Creates Data Flow Diagrams as `.md` files using Mermaid flowchart syntax.
- `mermaid-cli` — Validates/fixes Mermaid syntax using `mermaid.parse()` with jsdom (no browser). Use only when asked to validate or fix Mermaid syntax.
