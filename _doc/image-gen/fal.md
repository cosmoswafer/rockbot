# fal.ai Image Generation — API Reference & Implementation Notes

## Config

```toml
[[image_providers]]
name = "fal"
api_key = "EDITME"
base_url = "https://queue.fal.run"

[image_providers.models]
seedream    = "fal-ai/bytedance/seedream/v4.5/text-to-image"
gptimage    = "openai/gpt-image-2"
gptimage_edit = "openai/gpt-image-2/edit"
grok_edit   = "xai/grok-imagine-image/quality/edit"
```

| Config field | Purpose |
|---|---|
| `base_url` | Queue submit endpoint (`POST {base_url}/{model_id}`) |
| `basecf_url` | CDN storage endpoint; defaults to `https://rest.fal.ai` if unset |
| `models` | Alias → full model ID resolution |

## API Pipeline (3-phase async)

### Phase 1 — Submit

```
POST {base_url}/{model_id}
Authorization: Key {api_key}
Content-Type: application/json

{
  "prompt": "...",
  "quality": "medium",
  "output_format": "png",
  "num_images": 1,
  "image_size": { "width": 3312, "height": 2480 },
  "image_urls": ["https://fal-cdn/uploaded.png"]   // img2img only
}
```

Success response (200):
```json
{
  "request_id": "019eb448-b6c5-7fe2-bc14-2b2498ae92d0",
  "status_url": "https://queue.fal.run/openai/gpt-image-2/requests/019eb448.../status",
  "response_url": "https://queue.fal.run/openai/gpt-image-2/requests/019eb448..."
}
```

Error responses carry `{"detail": "..."}` with a human-readable message.

### Phase 2 — Poll status

```
GET {status_url}
Authorization: Key {api_key}
```

Polling loop: 2 s interval, 300 attempts max (~10 min timeout). Logs progress every 5th attempt.

Status values:

| Status | Action |
|---|---|
| `COMPLETED` | Fetch result |
| `FAILED` | Return error from `body.error` field |
| anything else | Sleep 2s, retry |

### Phase 3 — Fetch result + download

```
GET {response_url}
Authorization: Key {api_key}
```

Success response (200):
```json
{
  "images": [
    { "url": "https://fal.media/files/.../result.png", "width": 1024, "height": 1024 }
  ]
}
```

The provider extracts `images[0].url`, then HTTP GETs that URL to download the raw bytes. The bytes are returned to the caller (`ImageProvider::generate_image` returns `Vec<u8>`).

## File Upload (img2img pre-upload)

Fal requires source images to be hosted at a public URL. We upload to fal's own CDN:

### Step 1 — Initiate

```
POST {storage_url}/storage/upload/initiate?storage_type=fal-cdn-v3
Authorization: Key {api_key}
Content-Type: application/json

{
  "content_type": "image/png",
  "file_name": "rockbot-{unix_ts}.png"
}
```

Returns:
```json
{
  "file_url": "https://fal-cdn/.../rockbot-1718123456.png",
  "upload_url": "https://storage.fal.ai/...presigned..."
}
```

### Step 2 — PUT binary

```
PUT {upload_url}
Content-Type: image/png

<raw bytes>
```

Returns `file_url` to the caller. This URL is then passed as an `image_urls` entry in the submit request.

### Turnaround times

| Step | Typical |
|---|---|
| Upload initiate + PUT | <1 s |
| Queue wait (submit → COMPLETED) | 30–180 s |
| Inference | ~30 ms |
| Result download | <1 s |
| **Total end-to-end** | **2–3 min** |

## Trait Mapping

`FalAiProvider` implements `ImageProvider`:

| Trait method | Implementation |
|---|---|
| `generate_image(params) -> Vec<u8>` | Submit → poll → fetch URL → HTTP GET bytes → return |
| `upload_file(data, content_type) -> String` | Initiate → PUT → return `file_url` |
| `provider_name() -> "fal"` | Static |
| `model_id() -> &str` | From config model resolution |

The inherent `generate_image_url(params) -> String` remains public for direct URL access (used by integration tests).

## Critical: Use API-returned URLs, do NOT reconstruct

The submit response includes `status_url` and `response_url`. Use them **as-is**. The API strips model action suffixes (`/edit`) from these URLs. Reconstructing from `base_url + model_id + request_id` would produce a wrong path that returns HTTP 405, causing `response.json()` to fail with "EOF while parsing a value".

## Real-image test

`crate-rockbot/tests/fal_real.rs` (`#[ignore]`) — exercises the full flow against the live API:

1. Reads `_docs/ref_img/p1.png` (~1.3 MB)
2. Uploads to fal CDN
3. Submits edit with image reference
4. Polls every 2 s until COMPLETED
5. Asserts result is an HTTPS URL

## Aspect Ratio Handling

The `image_gen` tool accepts an `aspect_ratio` parameter from the LLM as `W:H` string (e.g. `"16:9"`, `"2:3"`, `"1:1"`). The flow:

```
LLM tool call args { "aspect_ratio": "16:9" }
  → image_gen.rs:189-195   parsed from args, falls back to config default_image_size if absent
  → types.rs:386-397       lookup_preset() maps W:H string to (width, height) pixel pair
  → fal.rs:94-96           resolve_image_size() → sent as {"image_size": {"width": N, "height": M}}
```

### Preset dimensions (`types.rs:386-397`)

| Aspect ratio | Width | Height | Pixels |
|---|---|---|---|
| `square_hd` / `1:1` | 2880 | 2880 | 8,294,400 |
| `landscape_16_9` / `16:9` | 3840 | 2160 | 8,294,400 |
| `portrait_16_9` / `9:16` | 2160 | 3840 | 8,294,400 |
| `landscape_4_3` / `4:3` | 3312 | 2480 | 8,213,760 |
| `portrait_4_3` / `3:4` | 2480 | 3312 | 8,213,760 |
| `landscape_3_2` / `3:2` | 3504 | 2336 | 8,185,344 |
| `portrait_2_3` / `2:3` | 2336 | 3504 | 8,185,344 |

All satisfy: multiples of 16, max edge ≤ 3840px, aspect ratio ≤ 3:1, pixel count within 655,360–8,294,400.

Unknown aspect ratio strings (not in the table above) pass through as raw strings in `image_size` — fal.ai may or may not handle them.

### `size_tier`

`default_image_size_tier` in `[rocketchat.model]` config is a rockbot concept — fal.ai does **not** have a `size_tier`/`image_size_tier` API parameter. The config field is set on `ImageGenParams` but never sent to fal.ai (intentional — fal determines output resolution from `image_size` dimensions alone).

### Debugging

To see the actual `image_size` sent to fal.ai, enable debug logging (`RUST_LOG=debug`). The debug line at `image_gen.rs:234` prints the resolved params, including `image_size`.
