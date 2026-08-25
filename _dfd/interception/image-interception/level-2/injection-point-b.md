# Injection Point B — process_message Inline

## 1. Purpose

`current_image_urls` auto-injected unconditionally (no prompt matching) from
the DDP `urls` field filtered by `content_type: image/*`:

## 2. Diagram

```mermaid
flowchart LR
    MSG_URLS["4. Message Image URLs<br/>current_image_urls<br/>(from DDP urls —<br/>content_type image/*)"]
    INLINE["process_message inline<br/>auto-inject (unconditional)"]
    ARGS2["args[image_urls]<br/>(merged)"]

    MSG_URLS -->|"inject if non-empty"| INLINE
    INLINE --> ARGS2
```
