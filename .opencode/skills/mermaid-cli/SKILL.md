---
name: mermaid-cli
description: Use ONLY when asked to validate, check, parse, or fix Mermaid diagram syntax. Covers running mermaid.parse() with jsdom for headless validation, common syntax errors (nested brackets, shape-chars inside labels, escaped quotes), and the validate.js script. Do NOT use for creating or editing diagrams (see dfd-md skill for that).
---

# mermaid-cli — Validate Mermaid Syntax

## Purpose

Validate Mermaid diagram blocks in `.md` files against the official Mermaid
parser (`mermaid.parse()`). Uses a `jsdom`-backed Node.js script so no
headless browser (Chrome/Puppeteer) is needed.

- Setup: one-time npm install of `mermaid` + `jsdom` in a temp workdir.
- Validate: extract ` ```mermaid` blocks from `.md` files and run
  `mermaid.parse()` on each.
- Fix: interpret parse errors to correct common syntax mistakes.

## Run validation

1. Install dependencies (once per environment):

   ```bash
   mkdir -p /tmp/opencode/mermaid_validate
   cd /tmp/opencode/mermaid_validate
   npm init -y --silent 2>&1
   npm install mermaid jsdom 2>&1 | tail -3
   ```

2. Create the validation script (see [validate.js](#validatejs)).

3. Run:

   ```bash
   node /tmp/opencode/mermaid_validate/validate.js
   ```

   Exit code is non-zero on any parse failure.

## validate.js

Write this to `/tmp/opencode/mermaid_validate/validate.js`, updating the
`dfdsDir` path to point at the target directory:

```js
import { JSDOM } from 'jsdom';
import fs from 'fs';
import path from 'path';

const dom = new JSDOM('<!DOCTYPE html><html><body><div id="mermaid-root"></div></body></html>', {
  url: 'http://localhost',
  pretendToBeVisual: true,
});

global.window = dom.window;
global.document = dom.window.document;
global.HTMLElement = dom.window.HTMLElement;
global.HTMLDivElement = dom.window.HTMLDivElement;
global.SVGElement = dom.window.SVGElement || class {};
global.getComputedStyle = dom.window.getComputedStyle;
global.requestAnimationFrame = dom.window.requestAnimationFrame || (cb => setTimeout(cb, 0));
global.cancelAnimationFrame = dom.window.cancelAnimationFrame || clearTimeout;
if (!global.CSSStyleSheet) global.CSSStyleSheet = class {};

const mermaid = (await import('mermaid')).default;
mermaid.initialize({ startOnLoad: false, securityLevel: 'loose' });

// --- EDIT THIS ---
const dfdsDir = '/path/to/your/_dfds';  // or any dir with .md files
// -----------------

const files = fs.readdirSync(dfdsDir).filter(f => f.endsWith('.md')).sort();
let total = 0, errors = 0;

for (const fname of files) {
  const content = fs.readFileSync(path.join(dfdsDir, fname), 'utf-8');
  const re = /```mermaid\n([\s\S]*?)```/g;
  let match, i = 0;
  while ((match = re.exec(content)) !== null) {
    const block = match[1].trim(), firstLine = block.split('\n')[0].trim();
    total++;
    try {
      await mermaid.parse(block);
      console.log(`OK  ${fname} [block ${i}]  (${firstLine})`);
    } catch (err) {
      errors++;
      console.log(`FAIL ${fname} [block ${i}]  (${firstLine})`);
      for (const line of (err.message || String(err)).split('\n').slice(0, 5))
        console.log(`     ${line.trim().slice(0, 120)}`);
    }
    i++;
  }
}
console.log(`\nTotal: ${errors} errors / ${total} blocks`);
process.exit(errors > 0 ? 1 : 0);
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

**`ReferenceError: window is not defined`**

Mermaid's ESM entry point accesses `window` during import. Ensure all jsdom
globals (`window`, `document`, `HTMLElement`, etc.) are set before the
`import('mermaid')` call. If a Node.js getter error appears (e.g. `navigator`
in Node 22+), omit that global and add `SVGElement` and `CSSStyleSheet`
fallbacks as shown in the script above.

**Browser / Puppeteer errors**

If using `mmdc` (the CLI renderer) instead of this validation script, it
requires a headless Chrome binary and `libnspr4` system library. The
`parse()` approach described here avoids this entirely.
