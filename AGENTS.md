# AGENTS.md — rockbot

## Project layout

```
crate-rocketchat/     # lib + debug binary — standalone RocketChat DDP WebSocket client
crate-rockbot/        # lib + application binary — config, AiProvider trait, MessagingClient trait (platform/), agent loop, tools, memory
crate-webdav/         # lib only — WebDAV client for NextCloud storage operations
_dfd/                # data flow diagrams (design spec): context diagram at root, subdirs by component (agent, infra, ai, memory, knowledge, tools, interception)
_doc/                # constraints, test suite inventory
.agents/skills/       # OpenCode skill definitions (dfd-md, mermaid-cli)
default.config.toml   # full config with all defaults (empty credentials)
example.config.toml   # minimal user overrides with EDITME placeholders
```

## Runtime

- Use `./tmp/` for runtime temporary files (logs, state, etc.). Never use `/tmp/` or other system-wide temp directories.
- All tool-generated artifact files (screenshots, console logs, network request/response dumps, heap snapshots, performance traces, probe output, etc.) **must** be written to `./tmp/`.
- Use `pkill rockbot` (process name) — **not** `pkill -f` (full cmdline). The `-f` flag reads `/proc/*/cmdline` which can hang on systems with stuck D-state kernel threads.
- **Bot must run in background** — all start/restart commands end with `&`. When using the Bash tool, run `nohup ... &` alone (never chain after `;` or `&&`), then verify with a separate call.
- Start the bot: `./target/release/rockbot &> ./tmp/rockbot.log &`
- Restart (two separate Bash calls — never chain after `nohup ... &`):
  1. `pkill rockbot 2>/dev/null; rm -f ./tmp/rockbot.log`
  2. `nohup ./target/release/rockbot < /dev/null > ./tmp/rockbot.log 2>&1 &`
- Restart with debug: same pattern, prepend `RUST_LOG=debug` to the `nohup` line.

### Multi-instance deployment (design)

Multiple bot instances may run concurrently, each driven by its own `CONFIG_FILE`. Two instances may intentionally share the same WebDAV root so they present **one shared identity** (different LLMs, one persona) to the same DM user. Constraints of this design:

- **One soul, two brains** — both instances read/write the same `soul.md`; per-bot identity must not live there.
- **Soul sync is pull-based** — `soul.md` is re-read from WebDAV on every incoming message; no background polling or cross-instance push.
- **No write coordination** — `edit_soul` does an unconditional PUT (last-write-wins); concurrent edits from both bots can lose a write.
- **`state_dir` must differ per instance** even when the WebDAV root is shared (Matrix SDK session stores must not collide).
- **Snapshots are isolated per bot** under `{root}/{snapshot_prefix}/{bot_id}/{webdav_dir}/snapshot.json` — see `_dfd/memory/memory/partitioning.md`.

Per-instance operational details (hostnames, account names, config files, restart commands) are deployment info and live only in a gitignored local note (`_doc/config-files.local.md`), never in the repo.

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
- **Edition 2024**, MSRV **1.93** (`matrix-sdk` dependency requires 1.93). Use modern Rust (`impl Trait` in return position allowed).
- **"Parse, don't validate"** — parse at boundaries (config, JSON, CLI args) into domain types once; the rest of the system works with infallible, type-safe data.
- **Errors via `thiserror` + `?`** — avoid `unwrap()` and `expect()` in production code.
- **Ownership-first** — prefer `&T`/`&str` for transient data, `Arc<str>` or `String` where ownership required.

## Key facts

- `Cargo.lock` is gitignored. Do not create or commit it.
- `config.toml` is gitignored. Two reference files exist:
  - `default.config.toml` — complete spec with all keys and default values (empty strings for credentials).
  - `example.config.toml` — minimal user override file with `EDITME` placeholders. Intended to be copied to `config.toml` and edited.
- `CONFIG_FILE` env var sets the config path; defaults to `config.toml` (not a CLI argument).
- Config uses TOML: `[platform] name` selects messaging platform (`"rocketchat"` or `"matrix"`), `[rocketchat.server]` + `[rocketchat.model]` or `[matrix.server]` + `[matrix.model]` sub-sections, `[[chat_providers]]` and `[[image_providers]]` arrays-of-tables.
- `rocketchat` crate has both `lib.rs` (public API) and `main.rs` (debug binary — connects to RocketChat and logs events, no bot logic).
- `rockbot` crate uses `async-trait` for the `AiProvider` trait (implementations: OpenRouter, DeepSeek, llama.cpp, Fal) and the `MessagingClient` trait (implementations: `RocketChatPlatform`, `MatrixPlatform`). Wiremock is available for mock HTTP testing.
- Exa API key: reads from `[tools.exa]` config section first, then falls back to `EXA_API_KEY` env var.
- Tools registered conditionally: `WebDavTool` and `ImageGenTool` only if WebDAV is configured; `ImageGenTool` also requires an `image_provider` entry (uses `FalAiProvider` internally regardless of provider name). `AcpTool` (`acp_delegate`) only when `[acp] enabled = true` — the ACP agent subprocess (`deno x opencode-ai acp`, `codex-acp`, …) spawns lazily on first tool call via `agent-client-protocol` v2 SDK over stdio; all SDK usage is encapsulated in `acp.rs`.
- Main loop: exponential backoff reconnect (2^attempt seconds, max 5 retries, then exits).
- `webdav` crate uses `quick-xml` and `base64` for WebDAV XML parsing and auth.

## DFD-driven development

Data Flow Diagrams in `_dfd/` are the design spec. The development flow is defined in the [`dfd-md` skill](.agents/skills/dfd-md/SKILL.md). Key rules:

- Before starting work, check the repo's open issues using the `gitea-issues` skill; if the change resolves an open issue, note it for the commit message.

- **Phase 1**: Integration probe (data collection; optional) — live-data probe against real server/API to collect actual data shapes. Skip if sufficient real-world data already exists.
- **Phase 2**: Revise DFD — design or update the DFD to accurately model desired data movement. Base data structures (section 3) on shapes observed in the probe when available. Keep levels clean; use notation rules from the skill.
- **Phase 3**: Implement data flow validation constraints — enforce data structure correctness through code-level constraints. Parse and validate at subsystem entry points ("parse, don't validate"). Cross-DFD shared structures defined once in a canonical location, imported by both producer and consumer modules, making mismatches compile-time errors.
- **Phase 4**: Concrete implementation — code types, core logic, and wiring described by the DFD. Favour incremental, type-first implementation.
- **Phase 5**: Review all DFDs (optional, explicit request only — not part of the routine change cycle) — re-read every DFD and confirm it matches the code. If a DFD's `mtime` is newer than its corresponding Rust source, the code is stale and must be updated to match the DFD. If the code was updated first, update the DFD.
- **Phase 6**: Integration test — write mock-backed (Wiremock) integration tests to verify the implementation works end-to-end. Each DFD's happy-path flow should have corresponding mock integration coverage.
- **Phase 7**: `cargo build --release` → bump the version in `Cargo.toml` (bug fix → patch, new feature → minor; make the bump part of the commit that introduces the change) → commit → push → restart bot (only restart if explicitly requested). When the work resolves one or more open Gitea issues, the commit message must include `closes #<N>` for each issue so Gitea auto-closes them on push.

### Rust type-driven design rules

Every DFD data structure (section 3) becomes a Rust type. Follow these rules
to make data flow violations compile-time errors rather than runtime surprises:

- **Input protection layer** — all external input must be validated at the
  boundary before entering the system. Use two complementary crates:
  - [`serde_valid`](https://crates.io/crates/serde_valid) (JSON Schema-based)
    for format/shape constraints at deserialization boundaries — `max_length`,
    `min_length`, `pattern`, `maximum`, `minimum`, `multiple_of`, `max_items`,
    `min_items`, `unique_items`, `enum`. Use `FromJsonValue` to deserialize
    and validate in a single step. Cross-field custom validation is supported
    via closure with `self` access.
  - [`validator`](https://crates.io/crates/validator) (business-logic
    validators) for domain-level constraints — `email`, `url`, `length`,
    `range`, `must_match`, `contains`, `regex`, `required`, `nested`, struct-
    level `schema` validation, `custom` function references, `ValidateArgs`
    for context passing. Works alongside `serde` `Deserialize`.
  Both can be derived on the same struct when a type needs both layers.
  Newtypes with invariants (non-empty strings, bounded numbers, well-formed
  URLs, IDs) should still be single-field structs with a fallible constructor
  (`TryFrom` / `FromStr` / factory fn) and a private field, guaranteeing the
  invariant — no downstream `.is_valid()` checks.
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

## Gitea issue investigation

When the user asks to **investigate a Gitea issue** (e.g. "investigate issue #42",
"debug #17", "look into gitea issue 23"):

1. **Do NOT modify source code.** No edits to `crate-*/`, `_dfd/`, `_doc/`,
   config files, or any other tracked source. The deliverable is analysis only.
2. **Use the `gitea-issues` skill** to fetch the issue body, comments, labels,
   and linked PRs. Read every comment — earlier discussion often contains the
   key clue.
3. **Do deep analysis.** Trace the relevant code paths (Read / Grep / Glob),
   consult the matching DFDs in `_dfd/`, inspect recent `git log` history for
   the affected modules, and correlate the symptoms against the implementation.
   Surface the *root cause*, not just the surface symptom.
4. **Probe real data when useful.** You may write and run throwaway test
   scripts, `cargo test -- --ignored` probes, or small debugging binaries under
   `./tmp/` to capture actual request/response shapes, log output, or error
   traces from the live server. Treat these as disposable — delete or leave in
   `./tmp/`; never commit them.
5. **Post full findings as a comment on the Gitea issue.** Use the
   `gitea-issues` skill to add the comment. The comment must include:
   - **Summary** (1–2 sentences): what is actually happening.
   - **Root cause**: which module/function/DFD is responsible, with
     `file_path:line_number` references.
   - **Evidence**: relevant log snippets, probe output, code excerpts, or DFD
     mismatches that prove the diagnosis.
   - **Recommended fix**: concrete next steps (which file to change, what to
     change, and why). Do *not* implement it.
   - **Risks / open questions**: anything the implementer should watch for
     (regressions, DFD updates needed, related issues).
6. **Keep the user informed.** Before posting, give the user a one-sentence
   heads-up in chat that the analysis is being posted to the issue. Do not
   surprise-post without context.

## OpenCode skills

- `dfd-dev` — Complete DFD-driven development workflow: check Gitea issues, probe, revise DFD (via `dfd-md`), implement type-first, test, build, bump version (bug fix → patch, e.g. 0.0.1; new feature → minor, e.g. 0.1.0), commit with `closes #N`, push, restart bot.
- `dfd-md` — Creates Data Flow Diagrams as `.md` files using Mermaid flowchart syntax.
- `gitea-issues` — Lists, creates, comments on, and closes Gitea issues on the repo's Gitea server.
- `mermaid-cli` — Validates/fixes Mermaid syntax using `mermaid.parse()` with jsdom (no browser). Use only when asked to validate or fix Mermaid syntax.
