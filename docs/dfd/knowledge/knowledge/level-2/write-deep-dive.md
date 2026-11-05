# Write Deep Dive — save_knowledge Tool

## 1. Purpose

The `save_knowledge` tool writes the index first, then the `.md` file after the index is committed. This ensures the index is always authoritative — a missing `.md` file (partial write) won't corrupt the catalog. Existence checks are performed against the in-memory index, not the WebDAV filesystem.

**References:**
- [Write Flow](../write.md) — parent success path

## 2. Diagram

```mermaid
flowchart TD
    CALL[ToolCall: save_knowledge]
    PARSE[Parse Arguments]
    SLUG[Generate Filename Slug]
    FORMAT[Format .md Content]
    MD_BODY[Markdown Body]
    READ_IDX[Read index.json]
    UPSERT[Upsert Index Entry]
    PUT_MD[PUT .md to WebDAV]
    PUT_IDX[PUT index.json to WebDAV]
    DAV[(NextCloud WebDAV)]

    CALL -->|"topic, content, when_useful, priority"| PARSE
    PARSE -->|"validated args"| SLUG
    SLUG -->|"{slug}.md"| FORMAT
    FORMAT -->|"frontmatter + body"| MD_BODY
    MD_BODY --> READ_IDX
    DAV -->|"GET knowledge/index.json"| READ_IDX
    READ_IDX -->|"parsed IndexEntry list"| UPSERT
    UPSERT -->|"upsert in-memory index (add or replace filename)"| PUT_IDX
    PUT_IDX -->|"PUT knowledge/index.json"| DAV
    PUT_IDX -->|"index committed"| PUT_MD
    PUT_MD -->|"PUT knowledge/{file}"| DAV
```
