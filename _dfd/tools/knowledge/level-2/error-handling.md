# Error Handling & Fallbacks

## 1. Purpose

Error paths and fallbacks shared by all three knowledge tools — invalid
arguments, write/read failures, and empty-result handling. See the happy
flows: [save](../save.md), [forget](../forget.md), and [recall](../recall.md).

## 2. Diagram

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
