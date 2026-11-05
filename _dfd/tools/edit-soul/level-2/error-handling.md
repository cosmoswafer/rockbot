# Error Handling & Fallbacks

## 1. Purpose

Front matter is validated before any write: a `---` block that does not parse as
flat `key: value` lines is rejected with `RockBotError::SoulFormat` and no PUT
is issued, so the LLM can retry instead of silently destroying metadata. On
transport failure the tool relies on the underlying `write_file_with_fallback`
(AutoMkcol → mkcol parents → retry PUT); errors bubble up directly via `?`. See
the [happy path](../main-path.md).

## 2. Diagram

```mermaid
flowchart TD
    SOUL(EditSoulTool)
    PARSE(Parse SoulDocument)
    PUT(WriteSoulMd)
    HTTP(HttpClient)
    DAV[(NextCloud WebDAV)]
    ERR_FORMAT[Error: SoulFormat]
    ERR_WRITE[Error: WebDAV Write Failed]
    AGENT[Agent Harness]

    SOUL -->|"raw content"| PARSE
    PARSE -.->|"broken front matter"| ERR_FORMAT
    PARSE -->|"canonical markdown"| PUT
    PUT -.->|"write failure (after write_file_with_fallback exhausted)"| ERR_WRITE
    ERR_FORMAT -->|"retryable error string"| AGENT
    ERR_WRITE -->|"error string"| AGENT
```
