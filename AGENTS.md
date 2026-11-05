# AGENTS.md — rockbot

## Project layout

```
crate-rocketchat/     # lib + debug binary — standalone RocketChat DDP WebSocket client
crate-rockbot/        # lib + application binary — config, AiProvider trait, MessagingClient trait (platform/), agent loop, tools, memory
crate-webdav/         # lib only — WebDAV client for NextCloud storage operations
docs/                 # reference docs — scenarios, test suite, historical design notes
models/image.toml     # canonical image_gen alias manifest (embedded at build time)
.agents/skills/       # OpenCode skills (std-dev, gitea-issues, mermaid-cli)
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

Archived in [`docs/multi-instance-deployment.md`](docs/multi-instance-deployment.md) — concurrent instances, shared WebDAV root, one persona; `state_dir` per instance.

## Build & test

```bash
cargo build --release               # workspace build (3 crates)
cargo test                          # all unit + mock integration tests; -p {crate} for one crate
cargo test -- --ignored             # live-data probes / real integration tests (needs config.toml credentials)
RUST_LOG=debug cargo test -p rocketchat --test integration_real -- --ignored --nocapture  # with logging
```

No CI, no `rustfmt.toml`/`clippy.toml`/`rust-toolchain`.

## Versioning, tags & releases

- **App version**: `crate-rockbot/Cargo.toml` → `[package] version`. The root `[workspace.package] version` is unused legacy — never edit it.
- **Library versions**: `crate-rocketchat/Cargo.toml` and `crate-webdav/Cargo.toml` carry their own `[package] version`; bump one only when that crate itself changes.
- **Semver**: fix → patch, feature → minor, breaking → major. The bump rides the change commit (no separate `chore: bump …` commit).
- **Tag every release**: lightweight `vX.Y.Z` on the commit that carries the bump (`git tag v2.11.0` right after committing, or `git tag v2.10.0 <sha>` to backfill). Push to both remotes: `git push origin <tag> && git push github <tag>`.
- **Create the Gitea release** for every pushed tag. Derive `$GITEA_API`/`$REPO` from `origin` exactly as in the [`gitea-issues` skill](.agents/skills/gitea-issues/SKILL.md), then `POST $GITEA_API/repos/$REPO/releases` with `{"tag_name":"vX.Y.Z","name":"vX.Y.Z","body":"## vX.Y.Z\n\n### New\n- …\n\n### Fixes\n- …","draft":false,"prerelease":false}` (write the body to `./tmp/release.json`, pass with `-d @./tmp/release.json`). Body mirrors the commits since the previous tag, grouped `### New` / `### Fixes`, with `#<N>` references.

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

## Development flow (std-dev)

Follow the [`std-dev` skill](.agents/skills/std-dev/SKILL.md). Before work, check open issues with the `gitea-issues` skill; note issue numbers and reference them in commits as `#<N>` — never use auto-closing keywords. No design documents are written: the plan lives in the conversation, and the types in the code are the durable artifact.

1. **Probe** (optional) — only when an external API's real response shape is unknown or uncertain; capture live responses under `./tmp/`; skip when schemas/docs already cover the shape.
2. **Plan the data flow** — map boundaries, canonical structures, transformation path, and constraints. For non-trivial work, agree on the plan with the user before coding.
3. **Validation schemas first** — write the Rust types identified in step 2 before any business logic, following the type-driven rules below.
4. **Implement type-first** — incremental: types → core logic → wiring; keep the change scoped and fix root causes.
5. **Test** — `cargo test` must pass; add unit tests for new logic and Wiremock-mocked end-to-end tests for new paths in `crate-rockbot/tests/`.
6. **Commit and push** — conventional commits describing the why; reference issues as `#<N>`; leave a comment on the issue when work is done and let a human close it. Never commit `Cargo.lock`/`config.toml`.
7. **Release** — `cargo build --release` → bump `crate-rockbot/Cargo.toml` (fix → patch, feature → minor; bump rides the change commit) → commit → tag `vX.Y.Z` → push commits + tag to both remotes → create the Gitea release (see [Versioning, tags & releases](#versioning-tags--releases)) → restart bot only if requested.

### Rust type-driven design rules

Every planned data structure becomes a Rust type; make violations compile-time errors:

- **Parse at boundaries** — external input (JSON/TOML/CLI) parsed into domain types once at entry; never pass `serde_json::Value` or raw strings inward.
- **Input protection** — [`serde_valid`](https://crates.io/crates/serde_valid) (format/shape at deserialization, `FromJsonValue` single-step) and/or [`validator`](https://crates.io/crates/validator) (domain rules) may both derive on one struct. Invariant newtypes: private field + fallible constructor (`TryFrom`/`FromStr`/factory) — holding the type guarantees the invariant, no `.is_valid()` checks.
- **Shared types** — defined once in a canonical module, imported by producer and consumer.
- **Errors via [`thiserror`](https://crates.io/crates/thiserror) + `?`** — messages name the data structure and offending field; no `unwrap()`/`expect()` in production (panics only for broken internal invariants).

## OpenCode skills

- `std-dev` — Standard data-type-driven development flow: probe, plan the data flow, define validation schemas, implement type-first, test, commit and push.
- `gitea-issues` — Lists, creates, comments on, and manages Gitea issues on the repo's Gitea server; never closes issues automatically.
- `mermaid-cli` — Validates/fixes Mermaid syntax using `mermaid.parse()` with jsdom (no browser). Use only when asked to validate or fix Mermaid syntax.
