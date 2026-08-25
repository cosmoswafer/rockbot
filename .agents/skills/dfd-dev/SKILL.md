---
name: dfd-dev
description: |
  Complete DFD-driven development workflow for rockbot. Use when starting,
  planning, or executing a code change: check Gitea issues, run the optional
  integration probe, revise the DFD, implement type-first, test, build, bump
  version, commit with `closes #N`, push, and restart the bot. Combines the
  dfd-md skill (diagram design) with the gitea-issues skill (issue tracking)
  and the release process (version bump, commit, push).
license: CC0-1.0
---

# dfd-dev — DFD-Driven Development Workflow

The end-to-end development flow for the rockbot workspace. Every change
follows these phases in order. The authoritative design spec lives in `_dfd/`;
the DFD creation/notation rules come from the **`dfd-md` skill**; issue
tracking and issue-closing commits use the **`gitea-issues` skill**.

## Start: check open issues

Before writing any code:

1. Load the `gitea-issues` skill.
2. List open issues on the repo's Gitea server and read the ones related to the
   planned change.
3. If the change resolves one or more open issues, note their numbers — the
   commit message must include `closes #<N>` for each so Gitea auto-closes them
   on push (see Phase 7).

## Phases

### Phase 0 — Integration probe (optional, manual, explicit request only)

Data collection against the live server/API to capture actual data shapes.
**Only run it when the user explicitly requests a probe.**

- Write a probe (no mocking — targets the real server) under `crate-*` or a
  throwaway harness in `./tmp/`, or use an existing `--ignored` probe test.
- Run it, record request/response shapes into `./tmp/` artifacts.
- **Skip if sufficient real-world data already exists.**
- Probe output feeds Phase 1's data-structure tables.

### Phase 1 — Revise DFD (use the `dfd-md` skill)

Design or update the DFD(s) in `_dfd/` so they accurately model the desired
data movement.

1. Load the `dfd-md` skill and follow its rules:
   - One level per diagram, **one dataflow per .md file**; notation
     (squares/rounded/cylinders), naming conventions, and document structure
     exactly as defined.
   - Base data structures on shapes observed in the Phase 0 probe when
     available.
   - Level 1 flows live as `{dfd-name}/{flow}.md`; Level 2/3 detail diagrams
     live in `{dfd-name}/level-2/` (and `level-3/`); shared shapes live in
     `{dfd-name}/structures.md`, all per the skill's layout rules.
   - Cross-reference, never duplicate, data structures across DFDs.
2. Update the DFD-to-code mapping table in `AGENTS.md` if the change touches a
   module with no mapped DFD, or if the primary source file moves.
3. Validate Mermaid syntax with the `mermaid-cli` skill (`mermaid.parse()`) if
   asked to or if the diagrams fail to render.

### Phase 3 — Implement data flow validation constraints

Enforce data-structure correctness through code-level constraints, per the
"Rust type-driven design rules" in `AGENTS.md`:

- **Parse, don't validate** — parse and validate at subsystem entry points
  (config, JSON, CLI args) once; the rest of the system uses infallible,
  type-safe data.
- **Input protection layer** — `serde_valid` (format/shape at deserialization
  boundaries) and/or `validator` (business-logic rules); both can be derived
  on the same struct. Newtypes with invariants get a private field + fallible
  constructor (`TryFrom` / `FromStr` / factory fn).
- **Cross-DFD shared types** — defined once in a canonical location, imported
  by producer and consumer so mismatches are compile-time errors.
- **Errors via `thiserror` + `?`** — error messages name the DFD data structure
  and offending field. No `unwrap()`/`expect()` in production.

### Phase 4 — Concrete implementation

- Code the types, core logic, and wiring described by the DFD.
- Favour incremental, **type-first** implementation — design the types from
  the DFD section 3 tables before writing functions.
- Follow the code style in `AGENTS.md`: async Rust everywhere (edition 2024,
  MSRV 1.93), ownership-first (`&T`/`&str` transient, `Arc<str>`/`String`
  owned).

### Phase 5 — Review all DFDs

- Re-read every DFD in `_dfd/` and confirm it matches the code.
- If a DFD's `mtime` is newer than its corresponding Rust source (see the
  DFD-to-code mapping table in `AGENTS.md`), the code is stale — update the
  code to match the DFD. If the code changed first, update the DFD.
- Both directions are required: DFD drift and code drift are both bugs.

### Phase 6 — Integration test

- Write mock-backed (Wiremock) integration tests verifying the implementation
  end-to-end.
- Each DFD's happy-path flow should have corresponding mock integration
  coverage.
- Run: `cargo test` (all unit + mock integration tests).

### Phase 7 — Release (build, version bump, commit, push)

1. **Build**: `cargo build --release` — must succeed before committing.
2. **Version bump** in `Cargo.toml` (the crate(s) whose code changed). Semver:
   - **Bug fix** → **patch** bump: `x.y.z` → `x.y.z+1`
     (from 0.0.0: bug fix lands at **0.0.1**).
   - **New feature** → **minor** bump: `x.y.z` → `x.y+1.0`
     (from 0.0.0: new feature lands at **0.1.0**).
   - Make the bump part of the commit that introduces the change — never a
     separate commit.
3. **Commit** (use the `gitea-issues` skill if working with issues):
   - `git status`, `git diff`, `git log --oneline -10` to inspect; stage only
     intended files. Never commit `Cargo.lock` or `config.toml` (both
     gitignored).
   - Concise message matching repo style.
   - If the work resolves open Gitea issues, the message **must** include
     `closes #<N>` for each issue number so Gitea auto-closes them on push.
4. **Push** — `git push`; Gitea closes the referenced issues.
