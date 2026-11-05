# Vision — Shared Structures

## 1. Overview

The agent harness natively "sees" images: when a user uploads an attachment to
RocketChat, the harness downloads it, encodes it as a base64 data URI, and embeds
it directly in the user's `ChatMessage` as `ContentPart::ImageUrl` parts — no
tool call needed.

The **vision tool** exists for the cases where the image is NOT already
attached to the incoming message and NOT available as a message URL with a
known image content type:

- **Public URL**: fetch any image on the public web (HTTP/HTTPS URL)
- **WebDAV file**: fetch an image stored in the room's WebDAV directory
- **Describe/analyze an image**: when the user explicitly asks what's in an image

> **Important**: when a user shares an image URL with `image/*` content type
> (e.g. NextCloud share links), the harness auto-injects those URLs into
> `image_gen` calls. The LLM should **not** call the vision tool in this case
> — call `image_gen` directly with the edit prompt. The harness provides the
> image URL automatically as the `image_urls` parameter. Vision should only be
> used when the user needs image analysis/description, not for editing.

The vision tool downloads the image, base64-encodes it, and returns a **markdown
image tag** as a standard tool result: `![name](data:mime/type;base64,...)`.
The LLM receives this as tool result text; it can embed the markdown tag in its
reply so RocketChat renders the image inline, or it can reference the base64
data URI for multimodal analysis by the AI provider.

> The vision tool does **not** perform OCR or image analysis — it is an image
> fetch-and-encode utility. Image analysis is done by the AI provider when the
> base64 data URI appears in chat context.
>
> **ContentPart injection**: The tool result text (`![name](data:...)`) is
> plain markdown — the LLM cannot see image pixels from it. The harness
> intercepts vision tool results, caches the decoded base64 data URIs in a
> per-room image pool, and injects them as `ContentPart::ImageUrl` parts
> in a synthetic user message before the next LLM call. This is the
> mechanism by which vision-tool-fetched images actually reach the
> multimodal model.

**Coworking with webdav tool**: the `webdav` tool's `read` action detects image
files by extension (`.png`, `.jpg`, etc.) and returns them as base64 markdown
tags — the same format as vision tool results. The harness intercepts both
`vision` AND `webdav` tool results for `ContentPart::ImageUrl` injection,
so images read from WebDAV storage pass transparently into the LLM context.
The `vision` tool handles public URLs; the `webdav` tool handles authenticated
WebDAV paths. The LLM chooses which to use based on the image's location.

- Upstream: [Agent Harness](../../agent/agent-harness/auto-attachment-vision.md) invokes the tool during the
  agent loop via `ToolRegistry::execute_by_name()`. The harness intercepts
  the result for injection into LLM context.
- Downstream: [AI Provider](../../ai/ai-provider/level-2/vision-payload.md) receives the tool result
  text and the injected `ContentPart::ImageUrl` for multimodal analysis.

## 3. Data Structures

#### `VisionParams`

| Field    | Type              | Notes                                                  |
| -------- | ----------------- | ------------------------------------------------------ |
| `url`    | `NonEmptyString`  | URL of the image to download (public or WebDAV). Validated non-empty at deserialization (LLM tool call boundary). |
| `prompt` | `string`          | Optional prompt declared in tool schema but **not consumed** by execution — reserved for future LLM image-analysis context |

#### Tool Result (markdown string)

The vision tool returns a markdown image tag:

```
![{name}](data:{mime_type};base64,{base64_encoded_bytes})
```

| Component         | Source                          | Example                    |
| ----------------- | ------------------------------- | -------------------------- |
| `{name}`          | URL path basename               | `photo.png`                |
| `{mime_type}`     | HTTP Content-Type or URL ext    | `image/png`                |
| `{encoded_bytes}` | base64-encoded image bytes      | `iVBORw0KGgo...`           |

#### MIME Detection

Detection uses the HTTP `Content-Type` header + URL file extension fallback:

| Extension          | MIME Type        |
| ------------------ | ---------------- |
| `.png`             | `image/png`      |
| `.jpg` / `.jpeg`   | `image/jpeg`     |
| `.gif`             | `image/gif`      |
| `.webp`            | `image/webp`     |
| `.svg`             | `image/svg+xml`  |
| *(other)*          | `image/png`      |

If the HTTP response includes a `Content-Type` header with a recognized image
MIME type, that takes precedence over extension-based detection.
