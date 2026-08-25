# WebDAV — Path Sanitization Checkpoint (Security)

## 1. Purpose

Every LLM-supplied `path` passes through a validation checkpoint before
being joined with the room directory. This is a non-functional requirement
(security boundary) applied at the `WebDavPath::room_path()` and
`WebDavPath::image_path()` entry points. The harness-injected `webdav_dir`
is trusted (computed server-side from room metadata, not LLM-controlled).
See [Main Path](../main-path.md) for the overall flow.

## 2. Diagram

```mermaid
flowchart TD
    RAW[Raw path from LLM]
    TRIM(TrimWhitespace)
    DETECT_DOTS{Contains '..'?}
    REJECT_TRAVERSE[Reject: PathTraversal]
    DETECT_ABS{Starts with '/'?}
    REJECT_ABS[Reject: PathEscape]
    STRIP_DOTS(Strip standalone '.' segments)
    COLLAPSE_SLASH(Collapse multiple '/')
    TRIM_SEP(Trim leading/trailing '/')
    VALID{Remaining path non-empty?}
    DEFAULT_ROOT[Accept: root dir '']
    JOIN("Join {room_id}/{path}")
    FINAL[Sanitized room-scoped path]

    RAW --> TRIM
    TRIM --> DETECT_DOTS
    DETECT_DOTS -->|"yes"| REJECT_TRAVERSE
    DETECT_DOTS -->|"no"| DETECT_ABS
    DETECT_ABS -->|"yes"| REJECT_ABS
    DETECT_ABS -->|"no"| STRIP_DOTS
    STRIP_DOTS --> COLLAPSE_SLASH
    COLLAPSE_SLASH --> TRIM_SEP
    TRIM_SEP --> VALID
    VALID -->|"empty"| DEFAULT_ROOT
    VALID -->|"has content"| JOIN
    DEFAULT_ROOT --> FINAL
    JOIN --> FINAL
```

**Implementation note**: The `url::Url::parse()` call in `full_url()` does
not normalize `../` segments — they pass through unchanged to the server
(unlike a browser which would resolve them client-side). The sanitization
must therefore happen at the `WebDavPath` layer, before the path reaches
`full_url()`.
