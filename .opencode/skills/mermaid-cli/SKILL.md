---
name: mermaid-cli
description: Use ONLY when asked to validate, check, parse, or fix Mermaid diagram syntax. Covers running mermaid.parse() with jsdom (no browser needed), common syntax errors (nested brackets, shape-chars inside labels, escaped quotes), and the validate.js tool. Do NOT use for creating or editing diagrams (see dfd-md skill for that).
---

# mermaid-cli — Validate Mermaid Syntax

## Purpose

Validate Mermaid diagram blocks in `.md` files against the official Mermaid
parser. Zero-install — Deno pulls `mermaid` + `jsdom` automatically on first
run. No headless browser, no npm install, no `/tmp` scripts.

## deno (preferred)

```bash
deno run -A --node-modules-dir=auto \
  .opencode/skills/mermaid-cli/scripts/validate.js \
  _dfds/agent-harness.md
```

Accepts a single `.md` file or a directory. First-run downloads + caches
dependencies (~5s). Subsequent runs are instant. Exit code is non-zero on
any parse failure.

## npx (fallback — needs Chrome)

```bash
npx @mermaid-js/mermaid-cli --input _dfds/agent-harness.md -o /dev/null -q
```

Catches syntax errors during rendering. Requires a headless Chrome binary
with `libnspr4` — may produce spurious "browser process" errors. Prefer
deno above.

## Common syntax errors

### 1. Shape-ambiguous characters in bracket labels

`[]` rectangle labels must not contain `()`, `{}`, or other shape-defining
characters unquoted. These characters are parsed as mermaid shape openers.

```mermaid
✗ START[main()]          → Parse error: '(' inside '[]' is seen as process start
✓ START["main()"]        → Quoted label suppresses shape parsing
```

```mermaid
✗ DAV[{room_id}/path/]   → '{' inside '[]' is seen as diamond start
✓ DAV["{room_id}/path/"] → Quoted label fixes it
```

### 2. Escaped quotes inside quoted labels

Mermaid uses `"` as the string delimiter for labels. Escaped quotes (`\"`)
inside a quoted label are NOT valid in the mermaid parser.

```mermaid
✗ NODE["\"text\" + var"] → Parse error: escaped quote breaks string
✓ NODE["text" + var]     → Drop the inner literal quotes
✓ NODE[text]             → Unquoted when no special chars
```

### 3. Nested shape syntax

A node definition like `ID[OTHER[...]]` is always invalid. Each node gets
exactly one shape wrapper.

```mermaid
✗ MEM[HIST[ConversationHistory]]  → Nested brackets, will not parse
✓ MEM[(ConversationHistory)]      → Single cylinder shape
```

### 4. Balancing brackets

Every opening `[`, `(`, `{` must have a matching close. Mermaid rejects
unbalanced lines.

```mermaid
✗ NODE[text                  → Missing ']'
✗ NODE([)]                   → Cross-nested shapes
✓ NODE[text]
✓ NODE([text])
```

## Interpreting parse errors

Parse errors give a line-number hint (counted within the mermaid block, not
the `.md` file) and an arrow pointing to the character that confused the
parser.

```
Parse error on line 10:
...Y[BotReply]    DAV[{room_id}/memory/]
----------------------^
Expecting 'TAGEND', ..., got 'DIAMOND_START'
```

- `got 'DIAMOND_START'` → a `{` inside `[]` was misread as a diamond shape.
- `got 'TAGEND'` → the parser expected to close the current tag but found
  something else (often an unexpected character like `\"` or nested brackets).

The "Expecting" list shows what the parser tried to find at that position.
