# Happy Flow — forget_knowledge

## 1. Purpose

`forget_knowledge` removes a per-room knowledge entry on WebDAV — it deletes
the entry's `.md` file under `knowledge/` and removes its record from the
`index.json` index, using the storage backend documented in
[Knowledge Management — Write](../../knowledge/knowledge/write.md).

**References:** [Shared Structures](structures.md) — params and file layout.

## 2. Diagram

```mermaid
flowchart TD
    AGENT[Agent Harness]
    FORGET(ForgetKnowledgeTool)
    DEL_MD(DeleteMdFile)
    GET_IDX(GetIndexJson)
    REMOVE(RemoveIndexEntry)
    PUT_IDX(PutIndexJson)
    HTTP(HttpClient)
    DAV[(NextCloud WebDAV)]

    AGENT -->|"topic"| FORGET
    FORGET -->|"DELETE knowledge/{filename}"| DEL_MD
    DEL_MD -->|"http delete"| HTTP
    HTTP -->|"DELETE request"| DAV
    DAV -->|"204 / 404"| DEL_MD
    DEL_MD -->|"ok or not found"| GET_IDX
    GET_IDX -->|"GET knowledge/index.json"| HTTP
    HTTP -->|"http request"| DAV
    DAV -->|"200 index.json"| GET_IDX
    GET_IDX -->|"existing index"| REMOVE
    REMOVE -->|"remove by topic match"| PUT_IDX
    PUT_IDX -->|"PUT knowledge/index.json"| HTTP
    HTTP -->|"http request"| DAV
    DAV -->|"204 / 201"| PUT_IDX
    PUT_IDX -->|"confirmation"| FORGET
    FORGET -->|"tool result"| AGENT
```

## 3. Data Structures

Shared data structures for these flows are documented in
[`structures.md`](structures.md) — see its `## 1. Overview` for
the subsystem context.
