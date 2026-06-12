# Image Provider Parameter Mapping

How `ImageGenParams` fields map to each provider's API request body.

## Config (`[image_model]` in config.toml)

| Field | Default | Used By | Description |
|-------|---------|---------|-------------|
| `default_provider` | `"fal"` | Both | Which `[[image_providers]]` entry to use |
| `default_text_model` | `"seedream"` | Both | Model alias for text-to-image |
| `default_edit_model` | `"fal-ai/nano-banana-pro/edit"` | Both | Model alias for img2img |
| `default_quality` | `"medium"` | Both | Quality tier |
| `default_output_format` | `"png"` | Both | Output file format (models may ignore) |
| `default_num_images` | `1` | Both | Images per request |
| `default_image_size` | `"portrait_2_3"` | Both | Aspect ratio preset (hidden from LLM) |

`default_image_size` presets resolve to pixel dimensions via `ImageSizeValue::Preset`:

| Preset | Ratio | Resolved | Pixels |
|--------|-------|----------|--------|
| `portrait_2_3` | 2:3 | 2344×3520 | 8.25M |
| `landscape_16_9` | 16:9 | 3840×2160 | 8.3M (4K wide) |
| `portrait_16_9` | 9:16 | 2160×3840 | 8.3M (4K tall) |
| `square_hd` | 1:1 | 2880×2880 | 8.3M |
| `landscape_4_3` | 4:3 | 3312×2480 | 8.2M |
| `landscape_3_2` | 3:2 | 3504×2336 | 8.2M |
| `auto` | — | model picks | varies |

## OpenRouter (`OpenRouterImageProvider`)

**Request**: `POST /api/v1/chat/completions` with `modalities: ["image"]`

**Image params go in `image_config` object** (within the top-level request body):

```json
{
  "model": "bytedance-seed/seedream-4.5",
  "modalities": ["image"],
  "image_config": {
    "aspect_ratio": "2:3",
    "image_size": "4K",
    "num_images": 1,
    "output_format": "png",
    "quality": "medium"
  },
  "messages": [...]
}
```

| Source field | `image_config` key | Format | Example |
|-------------|-------------------|--------|---------|
| `image_size` (Preset) | `aspect_ratio` | Ratio string | `"2:3"`, `"16:9"` |
| `image_size` (Custom) | `aspect_ratio` | Width:Height | `"1920:1080"` |
| *(hardcoded)* | `image_size` | Tier string | `"4K"` |
| `output_format` | `output_format` | Format string | `"png"` |
| `quality` | `quality` | Quality string | `"medium"` |
| `num_images` | `num_images` | Integer | `1` |

**Key differences from fal**:
- Uses string `"4K"` for resolution tier (not pixel dimensions).
- Preset maps to `aspect_ratio` (ratio string), not `image_size`.
- `image_size: "4K"` is hardcoded — not configurable per-request.
- `image_config` is nested at request top level, not inside `messages`.
- Seedream ignores `output_format: "png"` and always returns JPEG.
- Upload file: 3-line no-op — returns `data:{mime};base64,{b64}`.
- Single synchronous POST (no queue polling).

## fal.ai (`FalAiProvider`)

**Request**: `POST {base_url}/{model_id}` (queue submit, then poll/fetch/download)

**Image params go at the top level of the submit body**:

```json
{
  "prompt": "...",
  "image_size": { "width": 2344, "height": 3520 },
  "quality": "medium",
  "num_images": 1,
  "output_format": "png"
}
```

| Source field | Submit body key | Format | Example |
|-------------|----------------|--------|---------|
| `image_size` (Preset) | `image_size` | `{width, height}` | `{"width":2344,"height":3520}` |
| `image_size` (Custom) | `image_size` | `{width, height}` | `{"width":1920,"height":1080}` |
| `output_format` | `output_format` | Format string | `"png"` |
| `quality` | `quality` | Quality string | `"medium"` |
| `num_images` | `num_images` | Integer | `1` |

**Key differences from OpenRouter**:
- Uses pixel dimensions `{width, height}` (not strings).
- 3-phase async pipeline: submit → poll (every 2s, up to 300 attempts) → fetch result URL → download image bytes.
- Data URIs must be uploaded to fal CDN first (60-line initiate+PUT).
- No separate `image_config` wrapper — params go directly in submit body.
- Respects `output_format` parameter.

## Comparison Table

| Aspect | OpenRouter | fal.ai |
|--------|-----------|--------|
| Endpoint | `/chat/completions` | `/{model_id}` (queue) |
| Protocol | Single POST, synchronous | Submit → Poll → Fetch (3-phase async) |
| Resolution control | `aspect_ratio` + `image_size: "4K"` | `{width, height}` |
| Resolution format | String tier (`"4K"`) | Pixel object |
| Aspect ratio format | String ratio (`"2:3"`) | Resolved pixels |
| Latency | 5–30s | 10–120s (queue wait) |
| Image delivery | Base64 inline in response | CDN URL → download |
| Data URI handling | Inline (no upload needed) | Pre-upload to CDN required |
| Output format | `output_format` (seedream ignores) | `output_format` (respected) |
| Auth | `Bearer {key}` | `Key {key}` |

## Tool Parameters (what LLM sees)

The LLM is NOT exposed to `image_size`. All resolution/ratio decisions are
config-driven. The LLM only provides `prompt` and optionally `image_urls`
(for edit/img2img mode). Room context (`room_id`, `webdav_dir`, `image_cache_key`)
is injected by the harness.

```json
{
  "type": "object",
  "properties": {
    "prompt": { "type": "string", "description": "Text description of the image to generate" },
    "room_id": { "type": "string", "description": "Room ID (injected automatically if omitted)" },
    "image_urls": { "type": "array", "description": "Image URLs for editing (injected automatically)" }
  },
  "required": ["prompt"]
}
```
