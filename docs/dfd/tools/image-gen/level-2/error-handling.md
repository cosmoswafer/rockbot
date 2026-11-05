# Error Handling & Fallbacks

## 1. Purpose

Level 2 detail for the happy-path diagram in [main-path](../main-path.md): failure paths when image generation or the WebDAV upload fails, surfacing errors to the agent loop.

## 2. Diagram

```mermaid
flowchart TD
    GEN(GenerateImage)
    DAV_UPLOAD(UploadToWebDAV)
    ERR_GEN[Error: GenerateImage Failed]
    ERR_UPLOAD[Error: WebDAV Upload Failed]
    FALLBACK[Return Error to Agent]

    GEN -.->|"HTTP error / timeout / missing result"| ERR_GEN
    DAV_UPLOAD -.->|"WebDAV PUT error"| ERR_UPLOAD
    ERR_GEN --> FALLBACK
    ERR_UPLOAD --> FALLBACK
    FALLBACK -->|"error message"| AGENT[Agent Loop]
```
