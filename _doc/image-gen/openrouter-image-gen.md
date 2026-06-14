# OpenRouter Image Generation — API Reference & Implementation Notes

## Config

```toml
[[image_providers]]
name = "openrouter"
api_key = "sk-or-v1-EDITME"
base_url = "https://openrouter.ai/api/v1"
basecf_url = ""
draw_path = "/images/generations"

[image_providers.models]
seedream = "bytedance-seed/seedream-4.5"
banana   = "google/gemini-3.1-flash-image-preview"
```

> **Note:** `draw_path` is defined in config but **not used** by our implementation. OpenRouter image generation goes through the standard chat completions endpoint (`/chat/completions`) with the `modalities` parameter — this is OpenRouter's documented approach. The dedicated `/images/generations` endpoint (OpenAI-compatible) is an alternative API surface that our implementation does not consume.

## API: Single-Request (Synchronous)

Unlike fal.ai's 3-phase submit/poll/fetch pipeline, OpenRouter image generation is a **single synchronous POST** to the chat completions endpoint. The image bytes are returned inline in the response as base64-encoded data URIs.

### Request

```
POST {base_url}/chat/completions
Authorization: Bearer {api_key}
Content-Type: application/json
HTTP-Referer: https://github.com/anomalyco/rockbot
X-Title: RockBot

{
  "model": "google/gemini-3.1-flash-image-preview",
  "stream": false,
  "modalities": ["image"],
  "messages": [
    {
      "role": "user",
      "content": "Generate a beautiful sunset over mountains"
    }
  ],
  "image_config": {
    "aspect_ratio": "16:9",
    "output_format": "png",
    "quality": "medium",
    "num_images": 1
  }
}
```

### Text-to-image (t2i)

The user message contains a plain text prompt. `modalities: ["image"]` tells OpenRouter this is an image generation request (not a text chat).

### Image-to-image (img2img)

When `image_urls` is present in the tool arguments, the user message switches to multipart content:

```json
{
  "messages": [
    {
      "role": "user",
      "content": [
        { "type": "text", "text": "edit this image: add a hat" },
        {
          "type": "image_url",
          "image_url": { "url": "data:image/png;base64,iVBOR...", "detail": "high" }
        }
      ]
    }
  ]
}
```

URLs may be HTTP(S) URLs, or data URIs (`data:image/...;base64,...`). Both are passed directly — no pre-upload to external storage is required.

### `image_config` Parameters

Our `ImageSizeValue::Preset` names are mapped to `aspect_ratio`. The resolution
tier is hardcoded to `"4K"` (highest available). Both are set from config —
the LLM does not control image size.

| Parameter | Type | Source | Description |
|---|---|---|---|
| `aspect_ratio` | string | Config `default_image_size` → preset_to_aspect_ratio | `"2:3"`, `"16:9"`, `"9:16"`, `"4:3"`, `"3:4"`, `"3:2"`, `"1:1"` |
| `image_size` | string | Hardcoded `"4K"` | `"1K"`, `"2K"`, `"4K"`, `"0.5K"`; or `"WxH"` for custom |
| `output_format` | string | Config `default_output_format` | `"png"`, `"jpeg"`, `"webp"` |
| `quality` | string | Config `default_quality` | `"low"`, `"medium"`, `"high"` |
| `num_images` | integer | Config `default_num_images` | Number of images to generate |

Example request with all `image_config` fields:

```json
{
  "model": "bytedance-seed/seedream-4.5",
  "modalities": ["image"],
  "image_config": {
    "aspect_ratio": "2:3",
    "image_size": "4K",
    "output_format": "png",
    "quality": "medium",
    "num_images": 1
  },
  "messages": [{"role": "user", "content": "..."}]
}
```

> **Seedream note**: the `output_format: "png"` parameter is ignored by
> `bytedance-seed/seedream-4.5` — it always returns JPEG regardless.
> At `image_size: "4K"` with `aspect_ratio: "2:3"` the output is
> ~2730×4096 px (11.2 MP).

Preset → `aspect_ratio` mapping:

| Preset | → `aspect_ratio` |
|---|---|
| `landscape_16_9` | `"16:9"` |
| `portrait_16_9` | `"9:16"` |
| `landscape_4_3` | `"4:3"` |
| `portrait_4_3` | `"3:4"` |
| `landscape_3_2` | `"3:2"` |
| `portrait_2_3` | `"2:3"` |
| `square` / `square_hd` | `"1:1"` |
| unknown / `auto` | passed through as-is |

Custom sizes (`ImageSizeValue::Custom { width, height }`) are sent as
`"W:H"` ratio in `aspect_ratio`.

### Success Response

```
200 OK
```

```json
{
  "id": "gen-...",
  "choices": [{
    "index": 0,
    "message": {
      "role": "assistant",
      "content": "Here is the generated image.",
      "images": [
        {
          "type": "image_url",
          "image_url": {
            "url": "data:image/png;base64,iVBORw0KGgoAAAANS..."
          }
        }
      ]
    },
    "finish_reason": "stop"
  }],
  "usage": { "prompt_tokens": 12, "completion_tokens": 1, "total_tokens": 13 }
}
```

The provider:
1. Navigates `choices[0].message.images[0].image_url.url`
2. Splits on `";base64,"` to extract the base64 payload
3. Decodes to raw bytes → returns `Vec<u8>`

### Error Response

```json
{
  "error": {
    "message": "Model not found or doesn't support image modality",
    "code": 404
  }
}
```

Errors are extracted from `error.message`. Non-JSON response bodies fall back to the raw text.

## File Upload (img2img pre-upload)

OpenRouter does **not** require pre-upload for img2img. Data URIs are accepted inline in the request `messages` array. Our `upload_file` implementation is a trivial encode:

```
upload_file(data, "image/png") → "data:image/png;base64,{base64(data)}"
```

This is a 3-line no-op compared to fal.ai's 60-line init/PUT/presigned-URL pipeline.

## Supported Models

OpenRouter image generation requires models with `"image"` in their `output_modalities`. Query programmatically:

```bash
curl "https://openrouter.ai/api/v1/models?output_modalities=image"
```

Known working models (as of writing):

| Model slug | Notes |
|---|---|
| `google/gemini-3.1-flash-image-preview` | Extended aspect ratios (`1:4`, `4:1`, `1:8`, `8:1`), `0.5K` resolution tier |
| `google/gemini-2.5-flash-image` | |
| `black-forest-labs/flux.2-pro` | |
| `black-forest-labs/flux.2-flex` | |
| `sourceful/riverflow-v2-standard-preview` | |
| `bytedance-seed/seedream-4.5` | |
| `openai/gpt-5-image` | High quality, default for server-side tool |

## Trait Mapping

`OpenRouterImageProvider` implements `ImageProvider`:

| Trait method | Implementation |
|---|---|
| `generate_image(params) -> Vec<u8>` | POST to `/chat/completions` with `modalities: ["image"]`, decode base64 from response |
| `upload_file(data, content_type) -> String` | Return `data:{content_type};base64,{encode(data)}` |
| `provider_name() -> "openrouter"` | Static |
| `model_id() -> &str` | From config model resolution |

## Comparison: OpenRouter vs fal.ai

| Aspect | OpenRouter | fal.ai |
|---|---|---|
| Protocol | Single POST, synchronous | Submit → poll → fetch (3-phase async) |
| Latency | <30 s (single request) | 2–10 min (queue wait) |
| Image delivery | Base64 inline in response | CDN URL → separate download |
| img2img input | Data URI inline in messages | Pre-upload to fal CDN required |
| Auth header | `Bearer {key}` | `Key {key}` |
| Error shape | `{"error": {"message": "..."}}` | `{"detail": "..."}` |
| Config used | `chat_path` (defaults to `/chat/completions`) | `base_url` |
| Config ignored | `draw_path` (unused by impl) | `basecf_url` (fallback for storage) |
