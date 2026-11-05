# Happy Flow — Write

## 1. Purpose

The write flow persists knowledge entries as `.md` files on WebDAV alongside a JSON `index.json`. It is triggered by explicit user commands (`!remember`, `!note`, `!save`) or autonomous agent decisions, and commits `index.json` before the `.md` file so the catalog always stays authoritative.

**References:**
- [Shared Structures](structures.md) — `KnowledgeIndex`, `IndexEntry`, markdown entry format
- [Write Deep Dive](./level-2/write-deep-dive.md) — `save_knowledge` tool internals
- [Error Handling](./level-2/error-handling.md) — write/load failure paths
- [save_knowledge Tool](../../tools/knowledge/save.md) — tool registration and parameters
- [Knowledge Priority](../knowledge-priority/priority-state.md) — `priority` field of index entries

## 2. Diagram

```mermaid
flowchart TD
    USER[User Message]
    AI[AiProvider]
    TOOL[save_knowledge Tool]
    MD[Write .md File]
    IDX_PARSE[Parse index.json]
    IDX_UPDATE[Update Index Entry]
    IDX_SER[Serialize index.json]
    DAV[(NextCloud WebDAV)]
    CTX_REFRESH[refresh_knowledge_context<br/>reload index summary]

    USER -->|"!remember / !note / !save / natural chat"| AI
    AI -->|"tool_call: save_knowledge"| TOOL
    TOOL -->|"topic + content + when_useful"| IDX_PARSE
    DAV -->|"existing index.json"| IDX_PARSE
    IDX_PARSE -->|"parsed index"| IDX_UPDATE
    IDX_UPDATE -->|"updated entries"| IDX_SER
    IDX_SER -->|"PUT index.json (committed first)"| DAV
    TOOL -->|"markdown body"| MD
    MD -->|"PUT .md file (after index committed)"| DAV
    TOOL -->|"triggers context refresh"| CTX_REFRESH[refresh_knowledge_context]
```

## 3. Data Structures

Shared data structures for these flows are documented in
[`structures.md`](structures.md) — see its `## 1. Overview` for
the subsystem context.
