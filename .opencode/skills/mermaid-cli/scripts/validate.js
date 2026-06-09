// Usage: deno run -A --node-modules-dir=auto validate.js <file.md | directory>
// Validates all Mermaid blocks in .md files using mermaid.parse().
// No npm install needed — Deno handles npm: specifiers.

import { JSDOM } from "npm:jsdom";
import fs from "node:fs";
import path from "node:path";

const target = Deno.args[0] || Deno.cwd();

// Setup minimal DOM for mermaid
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

// Build file list: single .md file or all .md files in directory
let files = [];
const stat = fs.statSync(target);
if (stat.isFile) {
  if (!target.endsWith(".md")) {
    console.log("Error: file must be .md");
    Deno.exit(1);
  }
  files.push({ name: path.basename(target), dir: path.dirname(target) });
} else {
  files = [...Deno.readDirSync(target)]
    .filter((e) => e.isFile && e.name.endsWith(".md"))
    .map((e) => ({ name: e.name, dir: target }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

if (files.length === 0) {
  console.log("No .md files found in", target);
  Deno.exit(0);
}

let total = 0;
let errors = 0;

for (const { name: fname, dir } of files) {
  const content = fs.readFileSync(path.join(dir, fname), "utf-8");
  const re = /```mermaid\n([\s\S]*?)```/g;
  let match;
  let i = 0;

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
      const msg = err.message || String(err);
      for (const line of msg.split("\n").slice(0, 5)) {
        console.log(`     ${line.trim().slice(0, 120)}`);
      }
    }
    i++;
  }
}

console.log(`\nTotal: ${errors} errors / ${total} blocks`);
Deno.exit(errors > 0 ? 1 : 0);
