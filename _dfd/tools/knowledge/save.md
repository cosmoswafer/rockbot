# Happy Flow — save_knowledge

## 1. Purpose

`save_knowledge` creates a per-room knowledge entry on WebDAV — it writes
the entry as a `.md` file under `knowledge/` and upserts the entry into the
`index.json` index, using the storage backend documented in
[Knowledge Management — Write](../../knowledge/knowledge/write.md).

**References:** [Shared Structures](structures.md) — params and file layout.

## 2. Diagram

```mermaid
flowchart TD
    AGENT[Agent Harness]
    SAVE(SaveKnowledgeTool)
    SLUG(GenerateFilenameSlug)
    FORMAT(FormatMarkdownBody)
    PUT_MD(PutMdFile)
    GET_IDX(GetIndexJson)
    UPSERT(UpsertIndexEntry)
    PUT_IDX(PutIndexJson)
    HTTP(HttpClient)
    DAV[(NextCloud WebDAV)]
    AI[AiProvider]

    AGENT -->|"category + topic + content + when_useful + tags"| SAVE
    SAVE -->|"category topic"| SLUG
    SLUG -->|"{category}_{slug}.md"| FORMAT
    FORMAT -->|"markdown body"| PUT_MD
    PUT_MD -->|"PUT knowledge/{filename}"| HTTP
    HTTP -->|"http request"| DAV
    DAV -->|"201 created"| PUT_MD
    PUT_MD -->|"success"| GET_IDX
    GET_IDX -->|"GET knowledge/index.json"| HTTP
    HTTP -->|"http request"| DAV
    DAV -->|"200 index.json or 404"| GET_IDX
    GET_IDX -->|"existing index"| UPSERT
    UPSERT -->|"add or update entry"| PUT_IDX
    PUT_IDX -->|"PUT knowledge/index.json"| HTTP
    HTTP -->|"http request"| DAV
    DAV -->|"204 / 201"| PUT_IDX
    PUT_IDX -->|"confirmation"| SAVE
    SAVE -->|"tool result"| AGENT
    AGENT -->|"context"| AI
```

## 3. Data Structures

Shared data structures for these flows are documented in
[`structures.md`](structures.md) — see its `## 1. Overview` for
the subsystem context.
