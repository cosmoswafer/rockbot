# Image Generation Tool

## 1. Purpose

Generates images via fal.ai's queue API and stores them on WebDAV. The agent loop
calls `image_gen` with a prompt; the tool submits to fal.ai, polls for completion,
downloads the result, uploads to WebDAV, and returns both the WebDAV path and the
original fal.ai CDN URL (the LLM should prefer the fal.ai URL when sharing with
the user).

- Upstream: [Agent Harness](../agent-harness.md) executes the tool during the
  agent loop via `ToolRegistry::execute_by_name()`
- Downstream: [AI Provider](../base/ai-provider.md) — `FalAiProvider` (provider/fal.rs)
  handles the fal.ai queue submit/poll/fetch cycle
- Downstream: WebDAV crate (`WebDavClient`, `WebDavPath`) persists image assets

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    AGENT[Agent Loop]
    PARSE(ParseArgs)
    FAL(FalAiProvider)
    SUBMIT(SubmitToQueue)
    POLL(PollStatus)
    FETCH(FetchResult)
    DOWNLOAD(DownloadImage)
    UPLOAD(UploadToWebDAV)
    DAV[(NextCloud WebDAV)]
    FORMAT(FormatResult)
    FAL_API[fal.ai API]

    AGENT -->|"prompt + room_id"| PARSE
    PARSE -->|"validated prompt"| FAL
    FAL -->|"submit request"| SUBMIT
    SUBMIT -->|"POST /{model_id}"| FAL_API
    FAL_API -->|"request_id"| SUBMIT
    SUBMIT -->|"request_id"| POLL
    POLL -->|"GET status every 2s"| FAL_API
    FAL_API -->|"COMPLETED"| POLL
    POLL -->|"request_id"| FETCH
    FETCH -->|"GET result"| FAL_API
    FAL_API -->|"images[0].url"| FETCH
    FETCH -->|"image URL"| DOWNLOAD
    DOWNLOAD -->|"image bytes"| UPLOAD
    UPLOAD -->|"PUT .png"| DAV
    DAV -->|"webdav path"| UPLOAD
    UPLOAD -->|"webdav path"| FORMAT
    FETCH -->|"fal.ai CDN URL"| FORMAT
    FORMAT -->|"webdav path + fal.ai URL"| AGENT
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    SUBMIT(SubmitToQueue)
    POLL(PollStatus)
    DOWNLOAD(DownloadImage)
    UPLOAD(UploadToWebDAV)
    ERR_SUBMIT[Error: Submit Failed]
    ERR_POLL[Error: Poll Failed / Timeout]
    ERR_DOWNLOAD[Error: Download Failed]
    ERR_UPLOAD[Error: WebDAV Upload Failed]
    FALLBACK[Return Error to Agent]

    SUBMIT -.->|"HTTP error / missing request_id"| ERR_SUBMIT
    POLL -.->|"FAILED status / timeout"| ERR_POLL
    DOWNLOAD -.->|"HTTP error / read error"| ERR_DOWNLOAD
    UPLOAD -.->|"WebDAV PUT error"| ERR_UPLOAD
    ERR_SUBMIT --> FALLBACK
    ERR_POLL --> FALLBACK
    ERR_DOWNLOAD --> FALLBACK
    ERR_UPLOAD --> FALLBACK
    FALLBACK -->|"error message"| AGENT[Agent Loop]
```

## 3. Data Structures

#### `ImageGenParams`

| Field       | Type     | Description                                      |
| ----------- | -------- | ------------------------------------------------ |
| `prompt`    | `string` | **Required.** Text description of the image      |
| `room_id`   | `string` | Room UUID for image storage (injected by harness if omitted) |
| `webdav_dir`| `string` | Type-prefixed room path (injected by harness; falls back to room_id) |
| `model_id`  | `string` | fal.ai model ID (default: `fal-ai/flux/schnell`) |

#### `ImageGenResult`

The tool returns a formatted string containing both paths:

```
Image generated and stored at {webdav_path}. Original fal.ai URL: {fal_url}
```

| Value        | Source                     | Purpose                                   |
| ------------ | -------------------------- | ----------------------------------------- |
| `webdav_path`| `WebDavPath::image_path()` | Persistent storage path in WebDAV         |
| `fal_url`    | `images[0].url`            | fal.ai CDN URL — prefer for sharing       |

#### fal.ai Queue API

The `FalAiProvider` (provider/fal.rs) implements a three-step queue workflow:

| Step   | Method | Endpoint                        | Response              |
| ------ | ------ | ------------------------------- | --------------------- |
| Submit | POST   | `{base_url}/{model_id}`        | `{"request_id": "..."}` |
| Poll   | GET    | `{base_url}/{model_id}/requests/{request_id}/status` | `{"status": "COMPLETED"}` |
| Fetch  | GET    | `{base_url}/{model_id}/requests/{request_id}`       | `{"images": [{"url": "..."}]}` |

Polling runs every 2 seconds for up to 90 attempts (3 minutes total), then times out.
