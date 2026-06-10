# Vision

## 1. Purpose

Downloads an image from a given URL and reports its metadata (byte size, MIME
type detected from file extension) along with an optional user prompt. This is a
read-only introspective tool — true vision (sending image data to an AI provider)
is planned but not yet implemented.

- Upstream: [Agent Harness](../agent-harness.md) invokes `VisionTool` with a
  URL and optional prompt
- Downstream: [AI Provider](../base/ai-provider.md) consumes the returned metadata
  as context for chat completions

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    AGENT[Agent Harness]
    VISION(VisionTool)
    HTTP_DL(DownloadImage)
    MIME(DetectMimeType)
    WEB[(Remote Web Server)]
    AI[AiProvider]

    AGENT -->|"url + prompt (optional)"| VISION
    VISION -->|"GET image"| HTTP_DL
    HTTP_DL -->|"http request"| WEB
    WEB -->|"image bytes + content-type"| HTTP_DL
    HTTP_DL -->|"image bytes + url"| MIME
    MIME -->|"mime type"| VISION
    VISION -->|"size + type + prompt"| AI
```

### 2b. Error Handling & Fallbacks

```mermaid
flowchart TD
    HTTP_DL(DownloadImage)
    WEB[(Remote Web Server)]
    ERR_STATUS[Error: HTTP Non-200]
    ERR_TIMEOUT[Error: Request Timeout]
    ERR_NET[Error: Network Unreachable]
    AGENT[Agent Harness]

    HTTP_DL -.->|"!200 status"| ERR_STATUS
    HTTP_DL -.->|"30s timeout"| ERR_TIMEOUT
    HTTP_DL -.->|"connection refused / dns failure"| ERR_NET
    ERR_STATUS -->|"error string"| AGENT
    ERR_TIMEOUT -->|"error string"| AGENT
    ERR_NET -->|"error string"| AGENT
```

## 3. Data Structures

#### `VisionParams`

| Field    | Type     | Notes                                                  |
| -------- | -------- | ------------------------------------------------------ |
| `url`    | `string` | URL of the image to download (required)                |
| `prompt` | `string` | Optional description of what to look for in the image  |

#### `VisionResult`

| Field       | Type     | Notes                                       |
| ----------- | -------- | ------------------------------------------- |
| `bytes`     | `u64`    | Image file size in bytes                    |
| `mime_type` | `string` | Detected MIME type (`image/png`, `image/jpeg`, etc.) |
| `prompt`    | `string` | Original prompt or default description      |

#### MIME Detection

Detection is based on URL file extension only (not content sniffing):

| Extension       | MIME Type        |
| --------------- | ---------------- |
| `.png`          | `image/png`      |
| `.jpg` / `.jpeg`| `image/jpeg`     |
| `.gif`          | `image/gif`      |
| `.webp`         | `image/webp`     |
| `.svg`          | `image/svg+xml`  |
| *(other)*       | `image/png`      |
