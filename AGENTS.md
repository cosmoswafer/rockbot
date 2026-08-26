# AGENTS.md — rockbot

## Project layout

```
crate-rocketchat/     # lib + debug binary — standalone RocketChat DDP WebSocket client
crate-rockbot/        # lib + application binary — config, AiProvider trait, MessagingClient trait (platform/), agent loop, tools, memory
crate-webdav/         # lib only — WebDAV client for NextCloud storage operations
_dfd/                # data flow diagrams (design spec): context diagram at root, subdirs by component (agent, infra, ai, memory, knowledge, tools, interception)
_doc/                # constraints, test suite inventory
.agents/skills/       # OpenCode skills (dfd-md, gitea-issues, mermaid-cli)
default.config.toml   # full config spec with defaults (empty credentials)
example.config.toml   # minimal overrides with EDITME placeholders
```

## Runtime

- Everything generated at runtime or by tools (logs, state, screenshots, request/response dumps, traces, probe output) goes in `./tmp/` — never `/tmp/`.
- `pkill rockbot` (process name), never `pkill -f` — reads `/proc/*/cmdline`, can hang on stuck D-state kernel threads.
- Bot always runs in background: run `nohup ... &` alone in its own Bash call (never chain after `;`/`&&`), verify in a separate call.
- Start: `./target/release/rockbot &> ./tmp/rockbot.log &`
- Restart (two separate Bash calls):
  1. `pkill rockbot 2>/dev/null; rm -f ./tmp/rockbot.log`
  2. `nohup ./target/release/rockbot < /dev/null > ./tmp/rockbot.log 2>&1 &`

  Debug variant: prepend `RUST_LOG=debug` to the `nohup` line.

### Multi-instance deployment (design)

Archived in [`_doc/multi-instance-deployment.md`](_doc/multi-instance-deployment.md) — concurrent instances, shared WebDAV root, one persona; `state_dir` per instance.

## Build & test

```bash
cargo build --release               # workspace build (3 crates)
cargo test                          # all unit + mock integration tests; -p {crate} for one crate
cargo test -- --ignored             # live-data probes / real integration tests (needs config.toml credentials)
RUST_LOG=debug cargo test -p rocketchat --test integration_real -- --ignored --nocapture  # with logging
```

No CI, no `rustfmt.toml`/`clippy.toml`/`rust-toolchain`.

## Code style

- Async Rust everywhere (sole exception: `crate-rocketchat/src/main.rs` debug binary uses sync `fn main` + `block_on`). Edition 2024, MSRV 1.93, modern Rust welcome.
- Ownership-first — `&T`/`&str` transient, `Arc<str>`/`String` owned.

## Key facts

- `Cargo.lock` gitignored — do not create/commit. `config.toml` gitignored — copy `example.config.toml` and fill EDITME placeholders; `default.config.toml` documents every key/default.
- `CONFIG_FILE` env var selects the config path (default `config.toml`; not a CLI argument).
- TOML config: `[platform] name` selects `"rocketchat"` or `"matrix"`; `[rocketchat.server]`+`[rocketchat.model]` or `[matrix.server]`+`[matrix.model]`; `[[chat_providers]]` and `[[image_providers]]` arrays-of-tables.
- `rocketchat` crate: `lib.rs` (public API) + `main.rs` (debug binary — logs events, no bot logic).
- `rockbot` crate: `async-trait` traits — `AiProvider` (OpenRouter, DeepSeek, llama.cpp, Fal) and `MessagingClient` (`RocketChatPlatform`, `MatrixPlatform`). Wiremock available for mock HTTP tests.
- Exa key: `[tools.exa]` config first, then `EXA_API_KEY` env var.
- Tools registered conditionally: `WebDavTool` and `ImageGenTool` need WebDAV configured; `ImageGenTool` also needs an `image_provider` entry (always uses `FalAiProvider` internally). `AcpTool` (`acp_delegate`) only when `[acp] enabled = true` — ACP agent subprocess (`deno x opencode-ai acp`, `codex-acp`, …) spawns lazily on first tool call via `agent-client-protocol` v2 SDK over stdio, encapsulated in `acp.rs`.
- Main loop: exponential backoff reconnect (2^attempt s, max 5 retries, then exits).
- `webdav` crate: `quick-xml` + `base64`.

## DFD-driven development

DFDs in `_dfd/` are the design spec; notation/layout/file rules come from the [`dfd-md` skill](.agents/skills/dfd-md/SKILL.md). Before work, check open issues with the `gitea-issues` skill; note issue numbers — commits resolving them must say `closes #<N>`.

1. **Probe** (optional, explicit request only) — live-data probe collecting real data shapes; skip if real-world data exists; feeds phase 2.
2. **Revise DFD** — model desired data movement; base section-3 structures on probe shapes; keep levels clean.
3. **Validation constraints** — enforce structures in code per the rules below; parse at subsystem entry points; shared cross-DFD types defined once, imported by producer+consumer → compile-time mismatch errors.
4. **Implementation** — code types/logic/wiring from the DFD; incremental, type-first.
5. **Review all DFDs** (optional, explicit request only) — confirm every DFD matches code; newer DFD `mtime` ⇒ stale code, vice versa ⇒ stale DFD.
6. **Integration test** — Wiremock-mocked end-to-end tests; every DFD happy path covered by mocks; run `cargo test`.
7. **Release** — `cargo build --release` → bump `Cargo.toml` (fix → patch, feature → minor; bump rides the change commit) → commit (never `Cargo.lock`/`config.toml`) → push → restart bot only if requested; `closes #<N>` for resolved issues.

### Rust type-driven design rules

Every DFD section-3 structure becomes a Rust type; make violations compile-time errors:

- **Parse at boundaries** — external input (JSON/TOML/CLI) parsed into domain types once at entry; never pass `serde_json::Value` or raw strings inward.
- **Input protection** — [`serde_valid`](https://crates.io/crates/serde_valid) (format/shape at deserialization, `FromJsonValue` single-step) and/or [`validator`](https://crates.io/crates/validator) (domain rules) may both derive on one struct. Invariant newtypes: private field + fallible constructor (`TryFrom`/`FromStr`/factory) — holding the type guarantees the invariant, no `.is_valid()` checks.
- **Cross-DFD shared types** — defined once in a canonical module, imported by producer and consumer.
- **Errors via [`thiserror`](https://crates.io/crates/thiserror) + `?`** — messages name the DFD structure and offending field; no `unwrap()`/`expect()` in production (panics only for broken internal invariants).

### DFD-to-code mapping

| DFD | Primary source | Key secondary sources |
| --- | -------------- | --------------------- |
| `_dfd/context-diagram.md` | Level 0 system boundary (no code) | — |
| `_dfd/infra/config/main-path.md` | `config.rs` | `example.config.toml`, `default.config.toml` |
| `_dfd/infra/rocketchat/main-path.md` | rocketchat (`client.rs`, `ddp.rs`, `types.rs`), `platform/rocketchat.rs` | — |
| `_dfd/infra/rocketchat-rest/rest-alias-send.md` | rocketchat (`rest.rs`), `harness.rs` | — |
| `_dfd/infra/matrix/main-path.md` | `platform/matrix.rs` | `platform/mod.rs` |
| `_dfd/ai/ai-provider/main-path.md` | `provider/mod.rs`, `provider/deepseek.rs`, `provider/openrouter.rs`, `provider/fal.rs`, `provider/llamacpp.rs` | `types.rs` |
| `_dfd/memory/memory/retrieve-two-layers.md` | `memory.rs` | `harness.rs`, webdav crate |
| `_dfd/memory/memory-reset/post-reply-decision.md` | `harness.rs` (`reset_room_if_needed`, `check_token_pressure`, `trim_context`) | `memory.rs`, `config.rs` |
| `_dfd/knowledge/knowledge/write.md` | `knowledge.rs` | `tools/save_knowledge.rs`, `tools/forget_knowledge.rs`, `tools/recall_knowledge.rs` |
| `_dfd/knowledge/knowledge-priority/priority-state.md` | `knowledge.rs` | `harness.rs`, `memory.rs` |
| `_dfd/agent/agent-loop/main-path.md` | `main.rs`, `platform/mod.rs` | `harness.rs`, `config.rs`, `platform/rocketchat.rs`, `platform/matrix.rs` |
| `_dfd/agent/agent-harness/agent-loop.md` | `harness.rs` | `memory.rs`, `tool.rs`, `provider/mod.rs` |
| `_dfd/interception/image-interception/complete-pipeline.md` | `harness.rs` | `tools/image_gen.rs`, `tools/vision.rs`, `tools/webdav.rs`, `provider/fal.rs`, `image_cache.rs` |
| `_dfd/interception/secret-interception/uuidv5-scoped-injection.md` | `harness.rs` (`load_secrets_from_webdav`, `filter_secrets_by_host`, `resolve_secret_refs_deep`, `replace_secret_refs`) | `tools/web_fetch.rs`, `tools/webdav.rs`, webdav crate |
| `_dfd/tools/reset-memory/flag-driven.md` | `tools/reset_memory.rs` | `harness.rs`, `memory.rs` |
| `_dfd/tools/webdav/main-path.md` | `tools/webdav.rs` | webdav crate |
| `_dfd/tools/calendar/main-path.md` | `tools/calendar.rs` | webdav crate, `utils.rs` |
| `_dfd/tools/search-web/main-path.md` | `tools/web_search.rs` | `tools/web_fetch.rs` |
| `_dfd/tools/web-fetch/main-path.md` | `tools/web_fetch.rs` | `tools/web_search.rs`, `harness.rs` (secret interception) |
| `_dfd/tools/image-gen/main-path.md` | `tools/image_gen.rs` | `provider/fal.rs`, webdav crate |
| `_dfd/tools/vision/main-path.md` | `tools/vision.rs` | — |
| `_dfd/tools/edit-soul/main-path.md` | `tools/edit_soul.rs` | `memory.rs`, webdav crate |
| `_dfd/tools/acp-delegate/main-path.md` | `tools/acp.rs` | `acp.rs`, `config.rs` |
| `_dfd/tools/knowledge/save.md` | `tools/save_knowledge.rs`, `tools/forget_knowledge.rs`, `tools/recall_knowledge.rs` | `knowledge.rs`, webdav crate |


## OpenCode skills

- `dfd-md` — Creates Data Flow Diagrams as `.md` files using Mermaid flowchart syntax.
- `gitea-issues` — Lists, creates, comments on, and closes Gitea issues on the repo's Gitea server; also handles issue investigation (analysis-only workflow).
- `mermaid-cli` — Validates/fixes Mermaid syntax using `mermaid.parse()` with jsdom (no browser). Use only when asked to validate or fix Mermaid syntax.
