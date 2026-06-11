# Image Generation Tool

## 1. Purpose

Generates images via fal.ai's queue API and stores them on WebDAV. The agent loop
calls `image_gen` with a prompt and optional parameters (quality, image_size,
output_format, num_images, model_id); the tool submits to fal.ai, polls for
completion, downloads the result, uploads to WebDAV, and returns both the
WebDAV path and the original fal.ai CDN URL (the LLM should prefer the fal.ai
URL when sharing with the user). For [openai/gpt-image-2](https://fal.ai/models/openai/gpt-image-2)
the recommended defaults are `quality: "medium"`, `output_format: "png"`, and
the highest available resolution for the chosen aspect ratio.

- Upstream: [Agent Harness](../agent-harness.md) executes the tool during the
  agent loop via `ToolRegistry::execute_by_name()`
- Downstream: [AI Provider](../base/ai-provider.md) — `FalAiProvider` (provider/fal.rs)
  handles the fal.ai queue submit/poll/fetch cycle
- Downstream: WebDAV crate (`WebDavClient`, `WebDavPath`) persists image assets
- API reference: [fal.ai GPT Image 2 schema](https://fal.ai/models/openai/gpt-image-2/api)
  — full input/output spec including `image_size`, `quality`, `output_format`

## 2. Diagram

### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    AGENT[Agent Loop]
    PARSE(ParseArgs)
    RESOLVE(ResolveAspectRatio)
    FAL(FalAiProvider)
    SUBMIT(SubmitToQueue)
    POLL(PollStatus)
    FETCH(FetchResult)
    DOWNLOAD(DownloadImage)
    UPLOAD(UploadToWebDAV)
    DAV[(NextCloud WebDAV)]
    FORMAT(FormatResult)
    FAL_API[fal.ai API]

    AGENT -->|"prompt + image_size (LLM), room_id + webdav_dir + image_urls (harness injects)"| PARSE
    PARSE -->|"merged with config defaults (quality, output_format, num_images) + injected image_urls"| RESOLVE
    RESOLVE -->|"prompt + resolved {width, height} + quality + output_format + num_images"| FAL
    FAL -->|"submit request"| SUBMIT
    SUBMIT -->|"POST /{model_id}"| FAL_API
    FAL_API -->|"{request_id, status_url, response_url}"| SUBMIT
    SUBMIT -->|"status_url, response_url"| POLL
    POLL -->|"GET status_url every 2s"| FAL_API
    FAL_API -->|"COMPLETED"| POLL
    POLL -->|"response_url"| FETCH
    FETCH -->|"GET response_url"| FAL_API
    FAL_API -->|"images[0].url"| FETCH
    FETCH -->|"image URL"| DOWNLOAD
    DOWNLOAD -->|"image bytes (10 min timeout)"| UPLOAD
    UPLOAD -->|"PUT {output_format}"| DAV
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
    FETCH(FetchResult)
    DOWNLOAD(DownloadImage)
    UPLOAD(UploadToWebDAV)
    ERR_SUBMIT[Error: Submit Failed]
    ERR_POLL[Error: Poll Failed / Timeout]
    ERR_FETCH[Error: Fetch Result Failed]
    ERR_DOWNLOAD[Error: Download Failed]
    ERR_UPLOAD[Error: WebDAV Upload Failed]
    FALLBACK[Return Error to Agent]

    SUBMIT -.->|"HTTP error / missing request_id / status_url / response_url"| ERR_SUBMIT
    POLL -.->|"HTTP error / FAILED status / timeout"| ERR_POLL
    FETCH -.->|"HTTP error / missing image URL"| ERR_FETCH
    DOWNLOAD -.->|"HTTP error / read error"| ERR_DOWNLOAD
    UPLOAD -.->|"WebDAV PUT error"| ERR_UPLOAD
    ERR_SUBMIT --> FALLBACK
    ERR_POLL --> FALLBACK
    ERR_FETCH --> FALLBACK
    ERR_DOWNLOAD --> FALLBACK
    ERR_UPLOAD --> FALLBACK
    FALLBACK -->|"error message"| AGENT[Agent Loop]
```

### 2c. Model Selection (Text-to-Image vs Edit)

The tool holds two `FalAiProvider` instances (`fal` for t2i, `fal_img2img` for edit).
The decision is driven by whether `image_urls` are present — either auto-injected by the
harness from user attachments, or explicitly provided by the LLM (e.g. a fal.ai CDN URL
from a previous generation).

```mermaid
flowchart TD
    PARSE(ParseArgs)
    CHECK{Has image_urls?}
    T2I[fal provider<br/>default_text_model]
    UPLOAD[Upload DataURIs<br/>to fal storage]
    EDIT[fal_img2img provider<br/>default_edit_model]
    SUBMIT(SubmitToQueue)
    FAL_API[fal.ai API]

    PARSE --> CHECK
    CHECK -->|"yes<br/>(user attachments or LLM-provided URLs)"| UPLOAD
    CHECK -->|"no"| T2I
    UPLOAD -->|"hosted URLs"| EDIT
    T2I -->|"POST /{t2i_model}"| SUBMIT
    EDIT -->|"POST /{edit_model}"| SUBMIT
    SUBMIT --> FAL_API
```

**Decision rule**: if `image_urls` is non-empty → use `fal_img2img` (edit model), otherwise
use `fal` (text-to-image model). The edit model is a separate fal.ai endpoint
(e.g. `openai/gpt-image-2/edit`) that requires `image_urls` in the request body.

### 2d. Harness Attachment Injection

User-attached images are downloaded and labelled by the harness before the
agent loop. Images appear in the conversation as markdown tags `![title](title)`.
The LLM references images by their title in image_gen prompts. The harness
scans prompts for title matches and injects only the referenced data URIs.

```mermaid
flowchart TD
    RC_ATT[User Attachment]
    DLOAD(DownloadAttachment)
    ENCODE(EncodeDataURI)
    LABEL[AssignTitle<br/>from filename]
    BUILD_MSG["BuildUserMessage<br/>with image tags"]
    LLM_CTX[(LLM Context)]
    GEN_PROMPT{LLM prompt<br/>mentions title?}
    INJECT(InjectMatched<br/>DataURIs)
    TOOL_ARGS[(image_gen args<br/>with image_urls)]

    RC_ATT --> DLOAD
    DLOAD --> ENCODE
    ENCODE --> LABEL
    LABEL --> BUILD_MSG
    BUILD_MSG --> LLM_CTX
    LLM_CTX --> GEN_PROMPT
    GEN_PROMPT -->|"yes"| INJECT
    GEN_PROMPT -->|"no (new generation)"| TOOL_ARGS
    INJECT --> TOOL_ARGS
```

**Merge rule**: if the LLM also provides `image_urls` directly (e.g. fal.ai CDN
URLs from a previous result), those are merged with the harness-tagged URIs.
The LLM should only provide URLs for previously generated images, NOT for
user-attached images — those are handled automatically.

**Data URI upload**: all inline data URIs are uploaded to fal.ai storage first
via the [two-step initiate+PUT protocol](https://github.com/fal-ai/fal-js), converting them
to hosted URLs before the queue API call. This avoids HTTP 413 errors from oversized request
bodies.

## 3. Data Structures

#### `ImageGenParams`

LLM provides only `prompt` and `image_size`; all other fields come from `[image_model]` config.

| Field           | Source            | Type                                           | Description                                      |
| --------------- | ----------------- | ---------------------------------------------- | ------------------------------------------------ |
| `prompt`        | LLM               | `string`                                       | **Required.** Text description of the image      |
| `image_size`    | LLM               | preset name or `{width: int, height: int}`     | Aspect ratio preset or custom dimensions. Both edges must be multiples of 16, max edge 3840px, aspect ratio ≤ 3:1, total pixels 655,360–8,294,400. Default: `"landscape_4_3"` |
| `room_id`       | Harness           | `string`                                       | Room UUID for image storage (injected by harness if omitted) |
| `webdav_dir`    | Harness           | `string`                                       | Type-prefixed room path (injected by harness; falls back to room_id) |
| `image_urls`    | Harness (auto)    | `[]string`                                     | Injected automatically from user attachments or LLM-provided fal.ai URLs |
| `model_id`      | Config            | `string`                                       | Hardcoded from `default_text_model` / `default_edit_model` |
| `quality`       | Config            | `string`                                       | Hardcoded from `default_quality` (e.g. `"medium"`) |
| `output_format` | Config            | `string`                                       | Hardcoded from `default_output_format` (e.g. `"png"`) |
| `num_images`    | Config            | `integer`                                      | Hardcoded from `default_num_images` (e.g. `1`) |

**Resolution presets** (maps aspect ratio → highest available dimensions):

| Preset              | Aspect Ratio | Dimensions  | Pixel Count |
| ------------------- | ------------ | ----------- | ----------- |
| `"square_hd"`       | 1:1          | 2880×2880   | 8,294,400   |
| `"landscape_16_9"`  | 16:9         | 3840×2160   | 8,294,400   |
| `"portrait_16_9"`   | 9:16         | 2160×3840   | 8,294,400   |
| `"landscape_4_3"`   | 4:3          | 3328×2496   | 8,306,688*  |
| `"portrait_4_3"`    | 3:4          | 2496×3328   | 8,306,688*  |
| `"landscape_3_2"`   | 3:2          | 3520×2344†  | 8,250,880   |
| `"portrait_2_3"`    | 2:3          | 2344×3520†  | 8,250,880   |
| `"square"`          | 1:1          | 512×512     | 262,144     |
| `"auto"`            | —            | model picks | —           |

\* Slightly exceeds the 8,294,400 pixel max — implementation must clamp to
   `{[3328, 2496]}` with pixel product validated server-side; client-side
   clamp to 3312×2480 (8,213,760 px) on models that enforce the limit strictly.
† `3520×2344` — 2344 not a multiple of 16; clamp to `3520×2336` (8,222,720 px)
   or `3504×2336` (8,185,344 px). Final mapping validated in implementation.

Custom `{width, height}` is also supported, passed directly to the API.

#### `ImageGenResult`

The tool returns a JSON object:

```json
{"ok": true, "fal_url": "https://v3b.fal.media/...", "webdav_path": "..."}
```

| Value        | Source                     | Purpose                                   |
| ------------ | -------------------------- | ----------------------------------------- |
| `webdav_path`| `WebDavPath::image_path()` | Persistent storage path in WebDAV         |
| `fal_url`    | `images[0].url`            | fal.ai CDN URL — prefer for sharing       |

#### fal.ai Queue API (POST body)

```
{
  "prompt": "...",
  "image_size": { "width": 3840, "height": 2160 },
  "quality": "medium",
  "num_images": 1,
  "output_format": "png"
}
```

The `FalAiProvider` (provider/fal.rs) implements a three-step queue workflow:

| Step   | Method | Endpoint                        | Response                              |
| ------ | ------ | ------------------------------- | ------------------------------------- |
| Submit | POST   | `{base_url}/{model_id}`         | `{"request_id":"...","status_url":"...","response_url":"..."}` |
| Poll   | GET    | Use `status_url` from submit response (NOT reconstructed) | `{"status": "COMPLETED"}`            |
| Fetch  | GET    | Use `response_url` from submit response (NOT reconstructed) | `{"images": [{"url": "..."}]}`       |

**URL construction note**: The submit response includes `status_url` and `response_url`
fields that must be used as-is. Do NOT reconstruct URLs from `base_url + model_id +
request_id`. For example, submitting to `openai/gpt-image-2/edit` returns URLs with
`openai/gpt-image-2` (without the `/edit` action suffix). Reconstructing with the
full model_id including `/edit` produces a 405 empty response, causing a JSON parse
error.

Polling runs every 2 seconds for up to 300 attempts (10 minutes total), then times out.
