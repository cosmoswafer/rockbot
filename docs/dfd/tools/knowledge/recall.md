# Happy Flow — recall_knowledge

## 1. Purpose

When `query` is non-empty, entries are matched by keyword overlap against
`when_useful` and topic title. When `query` is empty, all entries in the
index are returned without filtering — the MATCH step is bypassed. Result
format: `[Knowledge: {display_title}]\n{body}`.

**References:** [Shared Structures](structures.md) — params and file layout.

## 2. Diagram

```mermaid
flowchart TD
    AGENT[Agent Harness]
    RECALL(RecallKnowledgeTool)
    GET_IDX(GetIndexJson)
    MATCH(MatchEntriesByQuery)
    GET_MD(GetMatchingMdFiles)
    FORMAT_CONTENT(FormatResult)
    HTTP(HttpClient)
    DAV[(NextCloud WebDAV)]
    AI[AiProvider]

    AGENT -->|"query (optional)"| RECALL
    RECALL -->|"GET knowledge/index.json"| GET_IDX
    GET_IDX -->|"http request"| HTTP
    HTTP -->|"GET request"| DAV
    DAV -->|"200 index.json"| GET_IDX
    GET_IDX -->|"parsed index entries"| MATCH
    MATCH -->|"topic / when_useful match"| GET_MD
    GET_MD -->|"GET each .md"| HTTP
    HTTP -->|"http request"| DAV
    DAV -->|"200 .md bodies"| GET_MD
    GET_MD -->|"entry contents"| FORMAT_CONTENT
    FORMAT_CONTENT -->|"[Knowledge: {display_title}]\n{body}"| RECALL
    RECALL -->|"formatted result"| AGENT
    AGENT -->|"context"| AI
```

## 3. Data Structures

Shared data structures for these flows are documented in
[`structures.md`](structures.md) — see its `## 1. Overview` for
the subsystem context.
