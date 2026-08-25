# Error Handling & Fallbacks

## 1. Purpose

On write failure, the tool relies on the underlying `write_file_with_fallback`
(AutoMkcol → mkcol parents → retry PUT). The tool does not implement its own
retry loop — errors bubble up directly via `?`. See the [happy path](../main-path.md).

## 2. Diagram

```mermaid
flowchart TD
    SOUL(EditSoulTool)
    PUT(WriteSoulMd)
    HTTP(HttpClient)
    DAV[(NextCloud WebDAV)]
    ERR_WRITE[Error: WebDAV Write Failed]
    AGENT[Agent Harness]

    PUT -.->|"write failure (after write_file_with_fallback exhausted)"| ERR_WRITE
    ERR_WRITE -->|"error string"| AGENT
```
