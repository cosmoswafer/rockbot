# Knowledge Tools

## 1. Purpose

Three tools for per-room knowledge management on WebDAV — `save_knowledge`
creates `.md` entries with a JSON index, `forget_knowledge` removes entries,
and `recall_knowledge` searches and retrieves stored knowledge. All three
share the same storage backend documented in [Knowledge Management](../knowledge/knowledge.md).

- Upstream: [Configuration Management](../infra/config.md) provides WebDAV
  credentials (knowledge is always enabled when WebDAV is configured)
- Upstream: [Agent Harness](../agent/agent-harness.md) registers and invokes all
  three tools during the agent loop
- Downstream: [Knowledge Management](../knowledge/knowledge.md) defines the
  storage format, index structure, and directory layout
- Downstream: [WebDAV Tool](webdav.md) performs GET/PUT/DELETE operations
  on `knowledge/` directory files

## 2. Diagram

### 2a. Happy Flow — save_knowledge

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

### 2b. Happy Flow — forget_knowledge

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

### 2c. Happy Flow — recall_knowledge

When `query` is non-empty, entries are matched by keyword overlap against
`when_useful`, `tags`, and topic. When `query` is empty, all entries in the
index are returned without filtering — the MATCH step is bypassed. Result
format: `[Knowledge: {display_title}]\n{body}`.

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
    MATCH -->|"topic / when_useful / tags match"| GET_MD
    GET_MD -->|"GET each .md"| HTTP
    HTTP -->|"http request"| DAV
    DAV -->|"200 .md bodies"| GET_MD
    GET_MD -->|"entry contents"| FORMAT_CONTENT
    FORMAT_CONTENT -->|"[Knowledge: {display_title}]\n{body}"| RECALL
    RECALL -->|"formatted result"| AGENT
    AGENT -->|"context"| AI
```

### 2d. Error Handling & Fallbacks

```mermaid
flowchart TD
    SAVE(SaveKnowledgeTool)
    FORGET(ForgetKnowledgeTool)
    RECALL(RecallKnowledgeTool)
    PUT_MD(PutMdFile)
    PUT_IDX(PutIndexJson)
    GET_IDX(GetIndexJson)
    PARSE(ParseArguments)
    HTTP(HttpClient)
    DAV[(NextCloud WebDAV)]
    ERR_PARSE[Error: Invalid Arguments]
    ERR_CAT[Error: Invalid Category]
    ERR_TOPIC[Error: Topic Not Found]
    ERR_WRITE[Error: WebDAV Write Failed]
    ERR_READ[Error: WebDAV Read Failed]
    ERR_EMPTY[Info: No Entries Found]
    AGENT[Agent Harness]

    SAVE --> PARSE
    FORGET --> PARSE
    RECALL --> PARSE
    PARSE -.->|"missing / invalid fields"| ERR_PARSE
    PARSE -.->|"category != skill/secret/note"| ERR_CAT
    ERR_PARSE -->|"error string"| AGENT
    ERR_CAT -->|"error string"| AGENT
    PUT_MD -.->|"write failure"| ERR_WRITE
    PUT_IDX -.->|"write failure"| ERR_WRITE
    ERR_WRITE -->|"error string"| AGENT
    GET_IDX -.->|"404 / parse error"| ERR_READ
    ERR_READ -->|"proceed with empty index"| RECALL
    FORGET -.->|"no matching entry found"| ERR_TOPIC
    ERR_TOPIC -->|"error string"| AGENT
    RECALL -.->|"index empty / no match"| ERR_EMPTY
    ERR_EMPTY -->|"No knowledge entries found"| AGENT
```

### 2e. Tool Interaction Overview

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

All data structures are shared with [Knowledge Management](../knowledge/knowledge.md#3-data-structures).

#### SaveKnowledgeParams

| Field        | Type                    | Notes                                           |
| ------------ | ----------------------- | ----------------------------------------------- |
| `category`   | `KnowledgeCategory`     | Enum: `skill`, `secret`, or `note`              |
| `topic`      | `NonEmptyString`        | Short title for the entry. Validated newtype.   |
| `content`    | `NonEmptyString`        | Markdown body of the knowledge entry. Validated newtype. |
| `when_useful`| `NonEmptyString`        | Situation description for retrieval. Validated non-empty (required field). |
| `tags`       | `Option<String>`        | Comma-separated keywords. Serde default: `None`. |
| `priority`   | `KnowledgePriority`     | Enum: `P0`, `P1`, `P2`, `P3`. Required — no default. |
| `webdav_dir` | `Option<String>`        | Room WebDAV key. Serde default: `None` (injected by harness). |

#### ForgetKnowledgeParams

| Field        | Type               | Notes                                    |
| ------------ | ------------------ | ---------------------------------------- |
| `topic`      | `NonEmptyString`   | Title or slug of the entry to delete. Validated newtype. |
| `webdav_dir` | `Option<String>`   | Room WebDAV key. Serde default: `None` (injected by harness). |

#### RecallKnowledgeParams

| Field        | Type               | Notes                                           |
| ------------ | ------------------ | ----------------------------------------------- |
| `query`      | `Option<String>`   | Keyword or topic to search. `None` = return all entries. Serde default: `None`. |
| `webdav_dir` | `Option<String>`   | Room WebDAV key. Serde default: `None` (injected by harness). |

#### File Layout

```
{root}/{webdav_dir}/knowledge/
├── index.json
├── skill_db_api.md
├── secret_github_token.md
├── note_driver_contact.md
└── ...
```
