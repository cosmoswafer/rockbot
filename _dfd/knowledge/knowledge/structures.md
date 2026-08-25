# Knowledge Management — Shared Structures

## 1. Overview

Knowledge persistence is **always enabled** when WebDAV is configured — no
separate config toggle required. The `save_knowledge`, `forget_knowledge`, and
`recall_knowledge` tools are automatically registered alongside other
WebDAV-backed tools.

Persistent per-room knowledge stored as `.md` files on WebDAV with a JSON
index for on-demand retrieval. Each entry lives in its own `.md` file
named `{slug}.md`. The `index.json` file lists every entry with a
`when_useful` field — a short description of the situation that makes this
knowledge relevant. The index is injected as a compact summary into agent
context; the AI uses `recall_knowledge` to fetch full `.md` bodies on demand.

### Write triggers

Knowledge is saved via the `save_knowledge` tool, which the AI provider can
call in two scenarios:

1. **Explicit command** — user says `!remember <thing>`, `!note <thing>`, or `!save <thing>`;
   the AI parses the instruction and emits `save_knowledge`
2. **Agent-initiated** — during normal conversation the AI determines
   something is worth persisting and emits `save_knowledge` autonomously

Magic words recognized by the system prompt:

| Magic word  | Maps to | Example |
|-------------|---------|---------|
| `!remember` | `save_knowledge` | `!remember that I prefer Python over JavaScript` |
| `!note`     | `save_knowledge` | `!note the prod server IP is 10.0.0.5` |
| `!save`     | `save_knowledge` | `!save that I prefer Python over JavaScript` |
| `!forget`   | `forget_knowledge` tool | `!forget the old database instructions` |

No frequency-based or periodic background extraction is planned.

### Retrieval

On room initialization (and on each `refresh_knowledge_context` call) the
harness loads `index.json` from WebDAV and formats a compact **index summary**.
The summary is merged into the single leading system message emitted by
`BuildContext` (between the soul block and any conversation summary) — never
sent as a separate system message, because strict chat templates reject any
system message not at index 0. The summary lists every entry's priority,
`display_title`, and `when_useful` — enough metadata for the AI to decide
whether to recall a full entry. The AI uses the `recall_knowledge` tool to
fetch individual `.md` bodies on demand when it determines an entry is
relevant to the current conversation.

- Upstream: [Agent Harness](../../agent/agent-harness/agent-loop.md) detects `save_knowledge` tool
  calls and loads knowledge on room init
- Upstream: [Configuration Management](../../infra/config/main-path.md) provides WebDAV access
  (knowledge is always enabled when WebDAV is configured)
- Downstream: WebDAV crate persists `.md` files and `index.json`
- Downstream: [AI Provider](../../ai/ai-provider/main-path.md) synthesizes knowledge entries from
  user instructions via `save_knowledge` tool calls
- Downstream: `BuildContext` merges the knowledge index summary into the
  single leading system message
- Downstream: [Knowledge Priority Algorithm](../knowledge-priority/priority-state.md) — static
  priority system (dormant since compression removed; entries stay at P1)

## 3. Data Structures

### `KnowledgeIndex`

Machine-readable JSON file at `{root}/{webdav_dir}/knowledge/index.json`.

| Field     | Type              | Notes                         |
| --------- | ----------------- | ----------------------------- |
| `version` | `String`          | `"rockbot-knowledge/1"`. Validates `min_length = 1` via `serde_valid`. |
| `room_id` | `String`          | WebDAV directory key. Validates `min_length = 1` via `serde_valid`. |
| `entries` | `Vec<IndexEntry>` | One descriptor per `.md` file. Validates via `serde_valid` (recursive validation of each `IndexEntry`). |

### `IndexEntry`

| Field         | Type               | Notes                                          |
| ------------- | ------------------ | ---------------------------------------------- |
| `filename`    | `String`           | `{slug}.md` — unique key and display identifier. Validates `min_length = 1` via `serde_valid`. |
| `when_useful` | `String`           | Situation description (retrieval trigger). Defaults to `""` (serde default). |
| `priority`    | `KnowledgePriority`| Current priority level. Static (dormant — see knowledge-priority/priority-state.md); default for new entries is `P1`. |
| `last_promoted_at` | `Option<String>` | ISO 8601 timestamp of last promotion; `None` if never promoted. Dormant field. |

The `filename` doubles as the display key — `display_title()` strips the `.md`
suffix. The index summary injected into context is formatted as one line per
entry: `[{priority}] {display_title} — {when_useful}`. The AI uses this summary
to decide which entries to recall via the `recall_knowledge` tool, which
downloads the full `.md` body on demand. `when_useful`, `priority`, and
`last_promoted_at` are denormalized into the index for fast retrieval
and priority updates without reading every `.md` file.

### `KnowledgePriority`

```rust
enum KnowledgePriority {
    P0, // highest priority — always included in recall_knowledge results
    P1, // default — strong recall bonus (+5)
    P2, // moderate recall bonus (+2)
    P3, // baseline (+0)
}
```

**Priority**: the `priority` field lives exclusively in `index.json`'s `IndexEntry` —
not in `.md` file frontmatter. This keeps `.md` files as pure user-editable
knowledge content. Priority is defined by the
[Knowledge Priority Algorithm](../knowledge-priority/priority-state.md) — currently dormant
(no compression cycle to trigger promotion). Priority appears as a `[P0]`/`[P1]`
/`[P2]`/`[P3]` tag in the index summary, helping the AI identify high-priority
entries to recall. In `recall_knowledge` results, P0 entries are always included
regardless of keyword overlap; P1-P3 get score bonuses added to keyword overlap.

### Markdown Entry Format

Each `.md` file uses a simple structure with optional frontmatter.
Priority and promotion timestamps are **index-only** — they do not appear in
`.md` files, keeping them purely user-editable knowledge content.

```markdown
# {title}

**When Useful:** {when_useful}
**Tags:** {tag1}, {tag2}
**Created:** {created_at}
**Updated:** {updated_at}

{content — free-form markdown body}
```

### File Layout

```
{root}/{webdav_dir}/knowledge/
├── index.json
├── db_api.md
├── openai_key.md
├── driver_contact.md
└── ...
```

Examples:

```
rockbot/r-general/knowledge/index.json
rockbot/r-general/knowledge/db_api.md
rockbot/d-alice/knowledge/github_token.md
rockbot/r-project-x/knowledge/build_commands.md
```

## 4. Integration with Agent Harness

### Tool: `save_knowledge`

Registered in `ToolRegistry`. Parameters:

| Parameter     | Type     | Required | Description                                      |
| ------------- | -------- | -------- | ------------------------------------------------ |
| `topic`       | `string` | Yes      | Short title for the entry                        |
| `content`     | `string` | Yes      | Markdown body                                    |
| `when_useful` | `string` | Yes      | Situation description (retrieval trigger)        |
| `tags`        | `string` | No       | Comma-separated keywords                         |
| `priority`    | `string` | Yes      | `"P0"`, `"P1"`, `"P2"`, or `"P3"` |

### Tool: `forget_knowledge`

Removes a knowledge entry and its index record. Parameters:

| Parameter | Type     | Description                              |
| --------- | -------- | ---------------------------------------- |
| `topic`   | `string` | Title or slug of the entry to delete     |

Deletes the `.md` file, removes the entry from `index.json`, and PUTs the
updated index back to WebDAV. If the file doesn't exist the index entry is
still removed (idempotent).

### Tool: `recall_knowledge`

Registered in `ToolRegistry`. Parameters:

| Parameter | Type     | Description                              |
| --------- | -------- | ---------------------------------------- |
| `query`   | `string` | Topic or keyword to search in the index  |

Returns the matching `.md` content (or all entries if no query).

### Context Injection

During `BuildContext` assembly (`MemoryManager::build_context`):
1. If WebDAV is configured, load `index.json` from WebDAV
2. Format a compact summary — one line per entry:
   ```
   [Knowledge Index — use recall_knowledge to retrieve full entries]
   [P0] critical — Always important
   [P1] db_api — When working with database APIs
   [P1] openai_key — When authenticating with OpenAI
   ```
3. Merge the summary into the single leading system message (system prompt +
   soul + knowledge + leading conversation summary, joined by `\n\n`)
4. The AI calls `recall_knowledge` to fetch full `.md` bodies on demand
