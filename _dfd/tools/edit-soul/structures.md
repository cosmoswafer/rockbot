# Edit Soul — Shared Structures

## 1. Overview

Manages the bot's permanent per-room "soul" memory — a single `soul.md` file
stored on WebDAV under `{room}/memory/soul.md`. The tool performs a **full
replace** of the entire file content using the standard soul template.

The LLM MUST use this template when calling edit_soul (front matter optional):

```markdown
---
name: YourName ✨
address_user_as: Master 🐵
language: en
---

# Soul Memory

- (optional preference)
- (optional fact)
```

`name` is a single line, max 32 characters, no markdown. `pronouns`,
`address_user_as` and `language` are optional. `schema` and `updated` are
stamped by the bot on write — the LLM must not author them. Front matter may be
omitted entirely for a plain bullet list (legacy format). The body remains a
flat `-` bullet list with no sub-headings; legacy souls are migrated on write
(the `- My name is X` bullet moves into `name:`).

- Upstream: [Configuration Management](../../infra/config/main-path.md) provides WebDAV
  credentials for file access
- Upstream: [Agent Harness](../../agent/agent-harness/agent-loop.md) invokes `EditSoulTool` with
  the full soul content and injects `previous_name`
- Downstream: [WebDAV Tool](../webdav/main-path.md) performs the PUT operation
- Downstream: [Memory Management](../../memory/memory/soul-editing.md) — soul.md lives alongside
  other per-room memory archives under `{room}/memory/`

## 3. Data Structures

#### `EditSoulParams`

| Field           | Type              | Notes                                              |
| --------------- | ----------------- | -------------------------------------------------- |
| `content`       | `NonEmptyString`  | Full soul.md content using the standard template. Validated non-empty at deserialization. |
| `webdav_dir`    | `string`          | Room WebDAV directory key (injected automatically). Falls back to `room_id` if absent. |
| `previous_name` | `Option<string>`  | Injected by the harness, not LLM-facing. Last known identity name; prevents an LLM rewrite from erasing the name. |
| `room_id`       | `Option<string>`  | Fallback directory key (injected automatically). |

#### `SoulDocument` / `SoulFrontMatter`

Defined once in [`memory/structures.md`](../../memory/memory/structures.md);
the tool parses the raw `content` into `SoulDocument`, normalizes it, and
renders the canonical file. Broken front matter is rejected with
`RockBotError::SoulFormat` and nothing is written.

#### Soul File Format

Stored at `/{root}/{webdav_dir}/memory/soul.md`.

```markdown
---
name: YourName ✨
address_user_as: Master 🐵
language: en
schema: 2
updated: 2026-11-05
---

# Soul Memory

- (optional preference)
- (optional fact)
```

#### Soul Operations

| Operation | Inputs   | Behavior                                 |
| --------- | -------- | ---------------------------------------- |
| `replace` | content  | Parse → normalize → render → PUT the entire soul.md file |
