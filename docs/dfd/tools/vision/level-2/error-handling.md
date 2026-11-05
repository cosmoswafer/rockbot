# Error Handling & Fallbacks

## 1. Purpose

Level 2 detail for the happy-path diagram in [main-path](../main-path.md): all
non-success HTTP responses produce a single generic error:
`"Failed to download image: HTTP {status}"`. Network errors
(connection refused, DNS failure, timeout) bubble up as `reqwest::Error`.
The 30-second timeout is set on the HTTP client but not distinguished
from other network failures in the error message.

## 2. Diagram

```mermaid
flowchart TD
    DL(DownloadImage)
    WEB[(Remote Server)]
    ENCODE(Base64Encode)
    ERR_STATUS[Error: HTTP Non-200]
    ERR_NET[Error: Network / Timeout]
    ERR_SIZE[Error: Image Too Large]
    AGENT[Agent Loop]

    DL -.->|"!200 status"| ERR_STATUS
    DL -.->|"network error / timeout"| ERR_NET
    ENCODE -.->|"image > max_attachment_bytes"| ERR_SIZE
    ERR_STATUS -->|"error string"| AGENT
    ERR_NET -->|"error string"| AGENT
    ERR_SIZE -->|"error string"| AGENT
    ERR_SIZE -->|"error string"| AGENT
```

Errors during auto-attachment download/encode are logged and the attachment is
skipped; the message still enters chat history with text-only content. Errors
from the vision tool are returned as tool result errors.
The size limit is configurable via `rocketchat.model.max_attachment_bytes`
(default 25 MB).
