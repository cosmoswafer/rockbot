# Error Handling

## 1. Purpose

Handles failures in both directions of knowledge management: extraction failures are logged and skipped, WebDAV write failures are retried once before degrading to a warning, and index read/parse failures degrade to proceeding without knowledge.

**References:**
- [Write Flow](../write.md) — parent success path
- [Load Flow](../load.md) — parent success path
- [Shared Structures](../structures.md) — knowledge index structures

## 2. Diagram

```mermaid
flowchart TD
    AI[AiProvider]
    TOOL[save_knowledge Tool]
    DAV[(NextCloud WebDAV)]
    GET_IDX[GET index.json]
    INJECT[Inject into BuildContext]
    ERR_EXTRACT[Extraction Failed]
    ERR_WRITE[WebDAV Write Failed]
    ERR_LOAD[WebDAV Read Failed]
    SKIP[Skip Entry]
    WARN[Warn + Proceed]
    RETRY[Retry Once]

    AI -.->|"api error during synthesis"| ERR_EXTRACT
    ERR_EXTRACT -->|"log + skip"| SKIP
    TOOL -.->|"PUT .md / PUT index.json failed"| ERR_WRITE
    ERR_WRITE -->|"retry"| RETRY
    RETRY -.->|"still fails"| WARN
    GET_IDX -.->|"GET / parse failed"| ERR_LOAD
    ERR_LOAD -->|"proceed without knowledge"| WARN
```
