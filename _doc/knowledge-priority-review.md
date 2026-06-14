# Knowledge Priority Algorithm — Deep Review

Reviewed 2026-06-14 from `_dfd/base/knowledge-priority.md` and `crate-rockbot/src/knowledge.rs`.

---

## Bug 1 (CRITICAL): Timestamp format mismatch breaks all decay logic

### Root cause

`now_iso_string()` (`crate-rockbot/src/utils.rs:2`) produces full ISO 8601 timestamps:

```text
2026-06-14T12:30:45Z
```

But `parse_iso_to_secs()` (`crate-rockbot/src/knowledge.rs:478`) only handles `YYYY-MM-DD` format:

```rust
fn parse_iso_to_secs(iso: &str) -> Option<u64> {
    let parts: Vec<&str> = iso.split('-').collect();
    // ...
    let d: u32 = parts[2].parse().ok()?;  // "14T12:30:45Z" — FAILS
    // ...
}
```

`"14T12:30:45Z".parse::<u32>()` returns `Err`, `ok()` → `None`, `?` → function returns `None`.

Then in `review_priorities()` (`knowledge.rs:456`):

```rust
let promoted_secs = parse_iso_to_secs(promoted_at).unwrap_or(0);
let days_since = (now_secs.saturating_sub(promoted_secs)) / 86400;
```

With `promoted_secs = 0` (epoch), `days_since` ≈ 20,600 days, which exceeds every decay
threshold (1, 3, 7). Every promoted entry decays one level per compression cycle until
it hits P3.

### Concrete simulation

| Step | Entry state | Action | `last_promoted_at` | `parse_iso_to_secs` | `days_since` | Match | New priority |
|------|-------------|--------|---------------------|----------------------|--------------|-------|-------------|
| 0    | P1, just promoted | — | `"2026-06-14T12:00:00Z"` | — | — | — | P0 |
| 1    | P0, not used | decay | `"2026-06-14T12:00:00Z"` | `None` → 0 | 20,600 | `(P0, d) d>=1` | P1 |
| 2    | P1, not used | decay | `"2026-06-14T12:00:00Z"` | `None` → 0 | 20,600 | `(P1, d) d>=3` | P2 |
| 3    | P2, not used | decay | `"2026-06-14T12:00:00Z"` | `None` → 0 | 20,600 | `(P2, d) d>=7` | P3 |

After 3 compression cycles, any promoted entry reaches P3 — regardless of actual
elapsed time. The intended gradual decay (P0→P1 after 1 day, P1→P2 after 3 days,
P2→P3 after 7 days) never happens.

### Fix

Two options, both valid:

**Option A** — Make `parse_iso_to_secs` handle full ISO 8601:
Parse `"2026-06-14T12:30:45Z"` → extract just the date portion → convert to
epoch days, then `* 86400`. Or use a proper date/time library.

**Option B** — Change `now_iso_string()` (or introduce a date-only variant) to
produce `"2026-06-14"` format, and use that in `last_promoted_at`. Other
timestamp fields (`created_at`, `updated_at` in `.md` files) still need full
ISO, so a new helper is cleaner.

**Option C** — Change `last_promoted_at` to store epoch seconds directly (a `u64`),
eliminating the string round-trip entirely.

---

## Bug 2 (DFD SPEC): Transition table in knowledge-priority.md section 2b contradicts state diagram

The state diagram (section 2b mermaid) and code both implement **one-step-per-cycle** decay:

```
P0 --> P1 : 1 day passes (no promotion)
P1 --> P2 : 3 days pass (no promotion)
P2 --> P3 : 7 days pass (no promotion)
```

But the **transition table** on line 112 shows **multi-step jumps**:

| Current | 3-7 days since | >7 days since |
|---------|---------------|--------------|
| **P0**  | → P2          | → P3         |

A P0 entry not promoted for 8 days would jump P0→P2 or P0→P3 in a single cycle,
which contradicts the "Promotion is one step up/down per compression cycle" rule
on line 124.

### Fix

Correct the transition table to show single-step decay only (or remove it
entirely — the state diagram is sufficient and unambiguous):

| Current | Promoted (used now) | Unused (days since promo ≥ threshold) |
|---------|---------------------|---------------------------------------|
| **P0**  | → P0                | → P1  (if ≥ 1 day)                    |
| **P1**  | → P0                | → P2  (if ≥ 3 days)                   |
| **P2**  | → P1                | → P3  (if ≥ 7 days)                   |
| **P3**  | → P2                | stays P3                              |

(This matches both the state diagram and the code at `knowledge.rs:458`.)

---

## Bug 3 (DFD SPEC): Flow diagram 2a implies `last_promoted_at` updates on decay too

Line 64:

```mermaid
CHANGED -->|"yes: update last_promoted_at"| TICK
```

This path is reachable from both PROMOTE and DECAY nodes. If implemented literally,
every decay would reset `last_promoted_at` to now — freezing the entry at its
decayed level forever (it would never accumulate enough days to decay further).

The code (`knowledge.rs:442`) correctly only updates `last_promoted_at` on promotion:

```rust
entry.last_promoted_at = Some(now.clone());  // only inside the promotion branch
```

### Fix

Split the flow in the DFD: promotion path updates `last_promoted_at`; decay path
does not. Or annotate the edge with a note that "updated only on promotion."

---

## Bug 4 (MINOR): .md file writes priority despite DFD saying it's index-only

DFD page, line 269:

> the priority field lives exclusively in index.json's IndexEntry — not in .md file frontmatter

But `save_entry()` at `knowledge.rs:252` writes:

```rust
format!("# {}\n\n**Category:** {}\n**Priority:** {}\n...", topic, category, priority, ...)
```

The `**Priority:** P1` line appears in every `.md` file. If a user edits a `.md`
file, the priority badge becomes stale/incorrect. The DFD intent is correct
(priority in index only) — the `.md` file should not contain priority.

### Fix

Remove the `**Priority:**` line from the `.md` template. The existing `created_at`
and `updated_at` fields are sufficient for user-facing display.

---

## Bug 5 (MINOR): `parse_compression_output` strips leading bullets but only one level

```rust
let trimmed = line.trim().trim_start_matches('-').trim();
```

Hyphens inside filenames (e.g. `skill_db-api.md`) are NOT affected — `trim_start_matches`
only removes leading characters. LLM may output `- skill_db-api.md` or
`- - skill_db-api.md` (nested list); the latter would leave a stray `-` in the
filename. Low-risk since the LLM prompt specifies a flat list.

---

## What works correctly

1. **One-step promotion** — P1→P0, P2→P1, P3→P2, P0 stays P0. Correct.
2. **Never-promoted entries don't decay** — `last_promoted_at = None` skips the
   decay branch. Their priority stays at the default P1 indefinitely. Correct.
3. **Only changed entries trigger Write** — guards against unnecessary WebDAV PUTs.
4. **P0 always-recall** — the retrieval logic correctly loads P0 entries regardless
   of keyword overlap.
5. **new entries default to P1** — `#[default]` on the enum + `save_entry()` passes
   `&KnowledgePriority::P1` from the tool default. Correct.
6. **Load failure skips room** — if `index.json` can't be read, `review_priorities`
   returns `Ok(false)` and the compression cycle continues without knowledge updates.
7. **Compression output parsing** — splits on `## Used Knowledge`, collects `.md`
   filenames. Reasonably robust.

---

## Summary

| Bug | Severity | Location | Effect |
|-----|----------|----------|--------|
| #1  | CRITICAL | `knowledge.rs:456,478` + `utils.rs:2` | All promoted entries decay to P3 within 3 cycles regardless of time |
| #2  | SPEC     | `knowledge-priority.md:112` (table) | Transition table contradicts state diagram + code |
| #3  | SPEC     | `knowledge-priority.md:64` (flow) | Flow diagram shows incorrect `last_promoted_at` update on decay |
| #4  | MINOR    | `knowledge.rs:252`              | `.md` files contain `Priority` field despite DFD saying index-only |
| #5  | MINOR    | `harness.rs:1412`               | Double-bullet could corrupt filename matching (unlikely) |
