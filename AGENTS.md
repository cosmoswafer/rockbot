# AGENTS.md — rockbot

## Project layout

```
crate-rocketchat/     # lib + debug binary — standalone RocketChat DDP WebSocket client
crate-rockbot/        # lib + application binary — config, AiProvider trait, MessagingClient trait (platform/), agent loop, tools, memory
crate-webdav/         # lib only — WebDAV client for NextCloud storage operations
docs/                 # reference docs — scenarios, test suite, historical design notes
models/image.toml     # canonical image_gen alias manifest (embedded at build time)
.agents/skills/       # OpenCode skills (std-dev, gitea-issues, gitea-bots, issue-labels, mermaid-cli, canonical-sync)
scripts/              # repo-local helpers (prune_tmp.py, opencode-task-status.sh) + their tests
default.config.toml   # full config spec with defaults (empty credentials)
example.config.toml   # minimal overrides with EDITME placeholders
```

## Runtime

- `pkill rockbot` (process name), never `pkill -f` — reads `/proc/*/cmdline`, can hang on stuck D-state kernel threads.
- Bot always runs in background: run `nohup ... &` alone in its own Bash call (never chain after `;`/`&&`), verify in a separate call.
- Start: `./target/release/rockbot &> ./tmp/rockbot.log &`
- Restart (two separate Bash calls):
  1. `pkill rockbot 2>/dev/null; rm -f ./tmp/rockbot.log`
  2. `nohup ./target/release/rockbot < /dev/null > ./tmp/rockbot.log 2>&1 &`

  Debug variant: prepend `RUST_LOG=debug` to the `nohup` line.

### Temp directory

- Everything generated at runtime or by tools (logs, state, screenshots, request/response dumps, traces, probe output) goes in `./tmp/` — never `/tmp/`.
- Structured layout — write new artifacts as:
  - `tmp/gitea-bots/scratch/` — per-issue dumps, plans, comment payloads, ad-hoc scripts.
  - `tmp/artifacts/` — screenshots, console logs, traces, network dumps.
  - Flat files at the `tmp/` root are legacy leftovers and prunable scratch.
- **Retention policy** — never delete live gitea-bots state: `tmp/gitea-bots/state.db*`, `pending.md`, `poll.lock`, `pending.lock`, `watcher.pid`, `watcher.meta`, `watcher.log`, `notify.log`. Scratch, build output and other artifacts are prunable at any time. Stop the watcher (`watch-issues.py stop`) and let reply workers finish before pruning; never delete `state.db` while the watcher runs.
- **Prune helper** — `python3 scripts/prune_tmp.py` previews the safe prune (dry run by default), `--apply` deletes it, `--truncate-logs` empties append-only watcher logs only while the watcher is stopped.

### Multi-instance deployment (design)

Archived in [`docs/multi-instance-deployment.md`](docs/multi-instance-deployment.md) — concurrent instances, shared WebDAV root, one persona; `state_dir` per instance.

## Build & test

```bash
cargo build --release               # workspace build (3 crates)
cargo test                          # all unit + mock integration tests; -p {crate} for one crate
cargo test -- --ignored             # live-data probes / real integration tests (needs config.toml credentials)
RUST_LOG=debug cargo test -p rocketchat --test integration_real -- --ignored --nocapture  # with logging

python3 .agents/skills/gitea-bots/scripts/run-tests.py    # watcher/notification unit tests
python3 .agents/skills/issue-labels/scripts/run-tests.py  # shared label-set tests
python3 .agents/skills/canonical-sync/tests/sync-test.py --root ./tmp/canonical-sync-sandbox  # skill-sync tooling (offline sandbox)
python3 scripts/run-tests.py                              # repo helper tests (prune_tmp)
bash .agents/skills/gitea-bots/tests/opencode-task-status.test.sh  # job-observability helper tests
```

No CI, no `rustfmt.toml`/`clippy.toml`/`rust-toolchain`. Python tooling needs 3.10+ and `requests` only.

## Background job observability

The agent tool catalog does not expose live status/output/kill/wait for background shell jobs (upstream gap: #127), and `/api/shell` defaults `location[directory]` to the service cwd. Use the repo-local fallback `scripts/opencode-task-status.sh`, which always passes `location[directory]`, supports cursor-based output, refuses unbounded `wait`, and requires `kill <id> --yes`:

```bash
scripts/opencode-task-status.sh --directory "$PWD" list
scripts/opencode-task-status.sh --directory "$PWD" status <shell-id>
scripts/opencode-task-status.sh --directory "$PWD" output <shell-id> --cursor 0 [--tail 80]
scripts/opencode-task-status.sh --directory "$PWD" kill <shell-id> --yes
scripts/opencode-task-status.sh --directory "$PWD" [--session "$OPENCODE_SESSION_ID"] wait <shell-id> --timeout 120 --interval 2
```

Inspect long-running jobs (gitea-bots watcher, `cargo test`) through that helper or their own logs/status files — never by guessing the service cwd.

## Versioning, tags & releases

- **App version**: `crate-rockbot/Cargo.toml` → `[package] version`. The root `[workspace.package] version` is unused legacy — never edit it.
- **Library versions**: `crate-rocketchat/Cargo.toml` and `crate-webdav/Cargo.toml` carry their own `[package] version`; bump one only when that crate itself changes.
- **Semver**: fix → patch, feature → minor, breaking → major. The bump rides the change commit (no separate `chore: bump …` commit).
- **Tag every release**: lightweight `vX.Y.Z` on the commit that carries the bump (`git tag v2.11.0` right after committing, or `git tag v2.10.0 <sha>` to backfill). Push the tag to the Gitea remote: `git push origin <tag>`.
- **Create the Gitea release** for every pushed tag. Derive `$GITEA_API`/`$GITEA_OWNER_REPO` from `origin` exactly as in the [`gitea-issues` skill](.agents/skills/gitea-issues/SKILL.md), then `POST $GITEA_API/repos/$GITEA_OWNER_REPO/releases` with `{"tag_name":"vX.Y.Z","name":"vX.Y.Z","body":"## vX.Y.Z\n\n### New\n- …\n\n### Fixes\n- …","draft":false,"prerelease":false}` (write the body to `./tmp/release.json`, pass with `-d @./tmp/release.json`). Body mirrors the commits since the previous tag, grouped `### New` / `### Fixes`, with `#<N>` references.

## Code style

- Async Rust everywhere (sole exception: `crate-rocketchat/src/main.rs` debug binary uses sync `fn main` + `block_on`). Edition 2024, MSRV 1.96, modern Rust welcome.
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
6. **Commit and push** — conventional commits describing the why; reference issues as `#<N>`; leave a comment on the issue when work is done and let a human close it. Never commit `Cargo.lock`/`config.toml`. If the change touched a shared skill under `.agents/skills/`, sync it out to the other repos right after committing (see [Shared skills](#shared-skills--canonical-store-and-sync)).
7. **Release** — `cargo build --release` → bump `crate-rockbot/Cargo.toml` (fix → patch, feature → minor; bump rides the change commit) → commit → tag `vX.Y.Z` → push commit + tag to `origin` → create the Gitea release (see [Versioning, tags & releases](#versioning-tags--releases)) → restart bot only if requested.

### Rust type-driven design rules

Every planned data structure becomes a Rust type; make violations compile-time errors:

- **Parse at boundaries** — external input (JSON/TOML/CLI) parsed into domain types once at entry; never pass `serde_json::Value` or raw strings inward.
- **Input protection** — [`serde_valid`](https://crates.io/crates/serde_valid) (format/shape at deserialization, `FromJsonValue` single-step) and/or [`validator`](https://crates.io/crates/validator) (domain rules) may both derive on one struct. Invariant newtypes: private field + fallible constructor (`TryFrom`/`FromStr`/factory) — holding the type guarantees the invariant, no `.is_valid()` checks.
- **Shared types** — defined once in a canonical module, imported by producer and consumer.
- **Errors via [`thiserror`](https://crates.io/crates/thiserror) + `?`** — messages name the data structure and offending field; no `unwrap()`/`expect()` in production (panics only for broken internal invariants).

## Shared skills — canonical store and sync

`~/Workspaces/qoder-skills/skills/` is the canonical store for every skill shared
between the workspace repos. A shared skill must be **byte-identical** in each repo
that carries it (one git blob), so diffs between repos come up empty and any of them
can serve as the source of truth. Use the [`canonical-sync` skill](.agents/skills/canonical-sync/SKILL.md)
rather than copying files by hand.

```bash
# 1. Drift check — a skill, or the whole workspace
python3 .agents/skills/canonical-sync/scripts/scan-skills.py --skill gitea-issues
python3 .agents/skills/canonical-sync/scripts/scan-skills.py --only-drifted

# 2. Plan (dry run). --repos scopes the run; without it, EVERY repo holding the
#    skill is in scope, so name the four that share this one explicitly.
python3 .agents/skills/canonical-sync/scripts/sync-skill.py gitea-issues --from qoder-skills \
  --repos rockbot,weight-management-frontend,openrouter-image-runner

# 3. Merge, ship, commit only the skill dir, push where the branch allows it
python3 .agents/skills/canonical-sync/scripts/sync-skill.py gitea-issues --from qoder-skills \
  --repos rockbot,weight-management-frontend,openrouter-image-runner \
  --commit --push --message 'docs(skills): adopt the cross-repo canonical gitea-issues text'
```

Repos deliberately outside the unified set (`sharkapi`, `weasel`,
`consent-cross-backend`) keep their older copies; list them nowhere in
`--repos`, and read `DRIFT` in the scan output with that in mind. Naming a repo
that does not have the skill yet rolls it out there.

Rules that decide the merge, not the tooling:

- Judge "newer" by `git log` dates plus content coverage, never by file mtime — copies
  and checkouts move mtimes.
- Two-way fork is the normal case: fold each repo's unique knowledge into the canonical
  text first (the plan's `coverage gaps` list is exactly that check), then distribute.
  Never one-way copy over another repo's work.
- Generalise repo-specific wording on the way in — "the project's own checks" instead of
  a hardcoded `cargo test`/`deno task check`, conditional phrasing for sibling-skill
  pointers — so one text serves every repo. Tooling that legitimately differs per repo
  (the `gitea-bots` skill's own `$REPO` derivation block; the `issue-labels` directory,
  which only some repos carry) is not drift and stays as it is.
- Commit only the skill directory, and only in repos whose branch can fast-forward; a
  repo trailing its own remote is fixed by a human, never by pushing into it.

## OpenCode skills

- `std-dev` — Standard data-type-driven development flow: probe, plan the data flow, define validation schemas, implement type-first, test, commit and push.
- `gitea-issues` — Lists, creates, comments on, and manages Gitea issues on the repo's Gitea server; reads the token from `.env`; never closes issues automatically.
- `gitea-bots` — Gitea issue watcher and notification plumbing: `scripts/watch-issues.py` (Python, SQLite state under `tmp/gitea-bots/`) polls for new issues and at-mention replies, dispatches workers from `pending.md`, and the skill defines the filing → plan → implementation bookkeeping. No command vocabulary; work follows `AGENTS.md`. `scripts/run-tests.py` is the offline test suite.
- `issue-labels` — Canonical status label set (`👀 seeing`, `💬 responded`, `🔨 working`, `❓ question`, `✅ committed`), lifecycle, `--reconcile` derivation and migration helpers; `scripts/set-status.py` is the shared CLI.
- `mermaid-cli` — Validates/fixes Mermaid syntax using `mermaid.parse()` with jsdom (no browser). Use only when asked to validate or fix Mermaid syntax.
- `canonical-sync` — Keeps shared skills byte-identical across the workspace repos with `scripts/scan-skills.py` (drift inventory) and `scripts/sync-skill.py` (two-way merge, apply, commit, push); `tests/sync-test.py` is its offline sandbox suite. See [Shared skills](#shared-skills--canonical-store-and-sync).
