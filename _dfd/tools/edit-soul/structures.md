# Edit Soul — Shared Structures

## 1. Overview

Manages the bot's permanent per-room "soul" memory — a single `soul.md` file
stored on WebDAV under `{room}/memory/soul.md`. The tool performs a **full
replace** of the entire file content using the standard soul template.

The LLM MUST use this exact template when calling edit_soul:

```markdown
# Soul Memory

- My name is YourName ✨
- (optional preference)
- (optional fact)
- (optional preference)
- (optional fact)
```

The soul is a **flat enumeration list** — each line is a `-` bullet item. The
**first item always** starts with `My name is ...`. The display name is
extracted by regex `My name is (.+)` from that first item. Keep it under 32
characters. Additional items follow the same flat list format with no
sub-headings.

- Upstream: [Configuration Management](../../infra/config/main-path.md) provides WebDAV
  credentials for file access
- Upstream: [Agent Harness](../../agent/agent-harness/agent-loop.md) invokes `EditSoulTool` with
  the full soul content
- Downstream: [WebDAV Tool](../webdav/main-path.md) performs the PUT operation
- Downstream: [Memory Management](../../memory/memory/soul-editing.md) — soul.md lives alongside
  other per-room memory archives under `{room}/memory/`

## 3. Data Structures

#### `EditSoulParams`

| Field        | Type              | Notes                                              |
| ------------ | ----------------- | -------------------------------------------------- |
| `content`    | `NonEmptyString`  | Full soul.md content using the standard template. Validated non-empty at deserialization. |
| `webdav_dir` | `string`          | Room WebDAV directory key (injected automatically). Falls back to `room_id` if absent. |

#### Soul File Format

Stored at `/{root}/{webdav_dir}/memory/soul.md`. The soul is a flat enumeration
list. The first item always starts with `My name is ...` — the display name is
extracted from that item by regex `My name is (.+)`.

```markdown
# Soul Memory

- My name is YourName ✨
- (optional preference)
- (optional fact)
- (optional preference)
- (optional fact)
```

#### Soul Operations

| Operation | Inputs   | Behavior                                 |
| --------- | -------- | ---------------------------------------- |
| `replace` | content  | Overwrites the entire soul.md file       |
