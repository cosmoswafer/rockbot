---
name: mermaid-cli
description: Use ONLY when asked to validate, check, parse, or fix Mermaid diagram syntax. Covers running mermaid.parse() with jsdom for headless validation, common syntax errors (nested brackets, shape-chars inside labels, escaped quotes), and the validate.js script. Do NOT use for creating or editing diagrams (see dfd-md skill for that).
---

# mermaid-cli — Validate Mermaid Syntax

## Purpose

Validate Mermaid diagram blocks in `.md` files against the official Mermaid
parser (`mermaid.parse()`). Uses Deno with `npm:` specifiers — no npm install
or headless browser needed. Dependencies are pulled and cached automatically on
first run.

- Validate: `deno run -A --node-modules-dir=auto validate.js [directory]`
- Fix: interpret parse errors to correct common syntax mistakes.

## Run validation

The `validate.js` script is bundled with this skill. Run it directly from the
skill directory:

```bash
deno run -A --node-modules-dir=auto \
  /home/claw/rockbot/.opencode/skills/mermaid-cli/validate.js \
  /home/claw/rockbot/_dfds
```

First run downloads + caches `mermaid` and `jsdom` automatically. Subsequent
runs use the cache.

Pass any directory containing `.md` files. The script extracts every
` ```mermaid` block, runs `mermaid.parse()` on each, and exits non-zero on
any parse failure.

## validate.js

The canonical script lives at
`.opencode/skills/mermaid-cli/validate.js`. It uses Deno's `npm:` specifiers so
zero local dependencies are required:

```js
import { JSDOM } from "npm:jsdom";
import fs from "node:fs";
import path from "node:path";

const targetDir = Deno.args[0] || Deno.cwd();

const dom = new JSDOM(
  '<!DOCTYPE html><html><body><div id="mermaid-root"></div></body></html>',
  { url: "http://localhost", pretendToBeVisual: true },
);

globalThis.window = dom.window;
globalThis.document = dom.window.document;
globalThis.HTMLElement = dom.window.HTMLElement;
globalThis.HTMLDivElement = dom.window.HTMLDivElement;
globalThis.SVGElement = dom.window.SVGElement || class {};
globalThis.getComputedStyle = dom.window.getComputedStyle;
globalThis.requestAnimationFrame = dom.window.requestAnimationFrame ||
  ((cb) => setTimeout(cb, 0));
globalThis.cancelAnimationFrame = dom.window.cancelAnimationFrame ||
  clearTimeout;

const mermaid = (await import("npm:mermaid")).default;
mermaid.initialize({ startOnLoad: false, securityLevel: "loose" });

const files = [...Deno.readDirSync(targetDir)]
  .filter((e) => e.isFile && e.name.endsWith(".md"))
  .map((e) => e.name)
  .sort();

let total = 0, errors = 0;

for (const fname of files) {
  const content = fs.readFileSync(path.join(targetDir, fname), "utf-8");
  const re = /```mermaid\n([\s\S]*?)```/g;
  let match, i = 0;
  while ((match = re.exec(content)) !== null) {
    const block = match[1].trim();
    const firstLine = block.split("\n")[0].trim();
    total++;
    try {
      await mermaid.parse(block);
      console.log(`OK  ${fname} [block ${i}]  (${firstLine})`);
    } catch (err) {
      errors++;
      console.log(`FAIL ${fname} [block ${i}]  (${firstLine})`);
      for (const line of (err.message || String(err)).split("\n").slice(0, 5))
        console.log(`     ${line.trim().slice(0, 120)}`);
    }
    i++;
  }
}

console.log(`\nTotal: ${errors} errors / ${total} blocks`);
Deno.exit(errors > 0 ? 1 : 0);
```

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

The "Expecting" list shows what the parser tried to find at that position —
use it to identify which character is being misinterpreted.

## Troubleshooting

**`deno run` fails with "Could not find a matching package"**

Add `--node-modules-dir=auto` to let Deno auto-resolve `npm:` packages:

```bash
deno run -A --node-modules-dir=auto validate.js _dfds
```

**First-run latency**

The initial run downloads mermaid + jsdom + their transitive dependencies
(~200 packages) to the Deno cache. Subsequent runs are instant.

**`mmdc` (the Mermaid CLI renderer)**

Avoid `@mermaid-js/mermaid-cli` for validation — it requires a headless Chrome
binary and `libnspr4` system library. The `mermaid.parse()` approach here
neither.
