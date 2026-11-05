# Tool Interaction Overview

## 1. Purpose

Level 1 overview of how `save_knowledge`, `forget_knowledge`, and
`recall_knowledge` interact with the shared WebDAV `knowledge/` storage
(`index.json` + `.md` files).

**References:** [Shared Structures](structures.md) — params and file layout.

## 2. Diagram

```mermaid
flowchart TD
    SAVE[save_knowledge]
    FORGET[forget_knowledge]
    RECALL[recall_knowledge]

    subgraph Storage[WebDAV knowledge/]
        IDX(index.json)
        MD[(.md files)]
    end

    SAVE -->|"writes .md + upserts index"| Storage
    FORGET -->|"deletes .md + removes from index"| Storage
    RECALL -->|"reads index + gets matching .md"| Storage
    IDX -->|"entry metadata"| RECALL
    MD -->|"entry content"| RECALL
```

## 3. Data Structures

Shared data structures for these flows are documented in
[`structures.md`](structures.md) — see its `## 1. Overview` for
the subsystem context.
