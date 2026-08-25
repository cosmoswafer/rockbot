# Error Handling & Fallbacks

## 1. Purpose

Level 2 detail for the happy-path diagram in [main-path](../main-path.md): how web fetch failures are reported to the Agent Harness — request timeouts, invalid HTTP methods, HTTP 4xx/5xx responses, non-UTF8 bodies, and WebDAV read/write failures.

## 2. Diagram

```mermaid
flowchart TD
    FETCH(FetchUrl)
    DAV[(NextCloud WebDAV)]
    HTTP[HTTP Client]
    SERVER[(Web Server)]
    TIMEOUT[Error: Request Timeout]
    NON200[Error: HTTP 4xx/5xx]
    PARSE_ERR[Error: Non-UTF8 Body]
    DAV_ERR[Error: WebDAV read/write failure]
    METHOD_ERR[Error: Invalid HTTP method]
    AGENT[Agent Harness]

    FETCH -.->|"30s elapsed"| TIMEOUT
    FETCH -.->|"invalid method"| METHOD_ERR
    HTTP -.->|"!200 status"| NON200
    SERVER -.->|"binary / non-text content"| PARSE_ERR
    DAV -.->|"path not found / auth failure"| DAV_ERR
    TIMEOUT -->|"error string"| AGENT
    NON200 -->|"error with status code"| AGENT
    PARSE_ERR -->|"content-type warning"| AGENT
    DAV_ERR -->|"error string"| AGENT
    METHOD_ERR -->|"error string"| AGENT
```
