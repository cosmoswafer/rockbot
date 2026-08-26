# Image Generation — Shared Structures

## 1. Overview

Generates images via an `ImageProvider` (fal.ai queue API or OpenRouter
Image API / chat-completions fallback), stores them on WebDAV for persistence, and caches the
raw image bytes in the shared `ImageCache`. The agent loop calls `image_gen`
with a prompt and optional parameters; the tool delegates to the provider,
writes to WebDAV, stores to the cache, and returns a minimal result
(`{ok, webdav_path, image_key}`) so the LLM context stays lightweight.

The LLM can select any model from the active image provider's `models`
catalog per call via an optional `model` alias arg; omitting it falls back to
the `[image_model]` defaults (`default_text_model` / `default_edit_model`).
The catalog (alias → model id) is a shared type
(`ImageModelCatalog`) produced from config at startup and consumed by the tool.

The tool's description and `parameters()` schema are **generated from the
catalog at registry time** (issue #95, see
[level-2/tool-description.md](level-2/tool-description.md)) — config is the
single source of truth; no model names are hardcoded anywhere in the schema.

- Upstream: [Agent Harness](../../agent/agent-harness/image-interception.md) injects `room_id`, `webdav_dir`,
  and `image_cache_key` (call_id) into tool args before invoking `execute_by_name()`
- Upstream: [Image Injection Pipeline](../../agent/agent-harness/image-sharing.md)
  retrieves the image from ImageCache by key and uploads it as a RocketChat attachment
- Downstream: [Image Provider](../../ai/ai-provider/main-path.md) — `FalAiProvider` (CDN-hosted URLs)
  and `OpenRouterImageProvider` (inline base64) implement `generate_image() -> Vec<u8>`
- Downstream: WebDAV crate persists image assets
- Shared: `ImageCache` (`image_cache.rs`) is the central store keyed by call_id

## 3. Data Structures

#### `ImageGenParams`

LLM provides `prompt` and `aspect_ratio` (both required); all other fields come from config.

| Field           | Source            | Type                                           | Description                                      |
| --------------- | ----------------- | ---------------------------------------------- | ------------------------------------------------ |
| `prompt`        | LLM               | `NonEmptyString`                               | **Required.** Validated at JSON deserialization — empty prompt fails at parse boundary. |
| `aspect_ratio`  | LLM               | `NonEmptyString`                              | **Required.** Aspect ratio as `W:H` (e.g. `"16:9"`, `"2:3"`, `"1:1"`). Validated non-empty at deserialization. Stored directly as `image_size: Preset(ratio_string)`. |
| `image_size`    | Tool (resolved)  | preset name → pixels                         | Resolved from LLM's `aspect_ratio` per-provider. Hidden from LLM. |
| `size_tier`     | Config            | `"4K"`, `"2K"`, `"1K"`                        | Resolution tier for OpenRouter. Set from `default_image_size_tier`. Ignored by fal. |
| `room_id`       | Harness           | `string`                                       | Room UUID for image storage (injected if omitted). **Note:** injected at execute time, not stored in the Rust struct. |
| `webdav_dir`    | Harness           | `string`                                       | Type-prefixed room path (injected; falls back to room_id). **Note:** injected at execute time, not stored in the Rust struct; also absent from the LLM-facing tool schema. |
| `image_cache_key`| Harness          | `string`                                       | Tool call_id — used as ImageCache lookup key. **Note:** injected at execute time, not in LLM-facing schema. |
| `image_urls`    | Harness (auto)    | `[]string`                                     | Injected from 5 converging sources (see §2e): user attachments, vision/WebDAV pool, agent-provided URLs, message image URLs (auto-injected unconditionally), and `reference_image_key` (ImageCache lookup) |
| `reference_image_key` | LLM | `string` | Alternative to `image_urls` — the `image_key` from a prior `image_gen` result. Looked up in ImageCache; data URI uploaded to provider CDN. Tool-level arg — not a field on the Rust struct; resolved before `ImageGenParams` construction. |
| `model`       | LLM              | `string` (alias)                              | **Optional.** Config alias for the active image provider's `models` catalog. Exposed in the tool schema as an `enum` of valid aliases (omitted when catalog empty). Resolved via `ImageModelCatalog`; unknown alias → `ToolCallParse` error at the parse boundary. |
| `model_id`    | Tool (resolved)  | `string`                                       | Populated by the tool from the LLM's `model` alias when given (see `ImageModelCatalog`); `None` when omitted — the provider instance then uses its configured default model id (`provider.model_id()`). |
| `quality`       | Config            | `string`                                       | From `default_quality`                           |
| `output_format` | Config            | `string`                                       | From `default_output_format`                     |
| `num_images`    | Config            | `integer`                                      | From `default_num_images`                        |
| `enable_safety_checker` | Config     | `boolean`                                      | From `default_enable_safety_checker` (default `false`). Sent by `FalAiProvider` only when model contains `"seedream/v5"`. |

#### `ImageModelCatalog`

Shared type produced at startup from the active `[[image_providers]]` entry's
`models` map (alias → model id) plus the `[image_model]` default aliases.
Defined in `types.rs` — produced by `main.rs` and consumed by the tool, so
mismatches are compile-time errors.

| Field                | Type                    | Description                                      |
| -------------------- | ----------------------- | ------------------------------------------------ |
| `entries`            | `[(string, string)]`    | `(alias, model_id)` pairs, sorted by alias — stable schema `enum`. |
| `default_text_alias` | `string`                | `[image_model] default_text_model` (t2i fallback) |
| `default_edit_alias` | `string`                | `[image_model] default_edit_model` (edit fallback) |

`resolve(alias)` returns the model id or `None` — the tool rejects unknown
aliases with a `ToolCallParse` error listing the valid aliases.
`allowed_aliases()` feeds the tool schema `enum`; `model_ids()` exposes the
resolved ids for the derived tool description.
`supports_auto_aspect()` is a derived capability flag — `true` iff any entry's
model id contains the `seedream/v5` marker (same constant used by
`FalAiProvider`). It drives the `auto_1K`/`auto_2K` hint in the tool and
`aspect_ratio` parameter descriptions; when `false`, no auto-dimensional
strings are advertised.

#### `ImageGenResult`

The tool returns minimal JSON — no base64. The actual image bytes are in `ImageCache` keyed by `image_key`.

```json
{"ok": true, "webdav_path": "...", "image_key": "call_abc123def4567890", "share_url": "https://..."}
```

The `share_url` field is conditionally present — included only when a NextCloud share link was successfully created for the generated image. It is absent when share generation failed (fallback to DDP attachment path).

#### `ImageCache` Entry (GeneratedImage)

Stored in `Arc<Mutex<HashMap<String, GeneratedImage>>>` keyed by call_id.

| Field          | Type           | Description                                   |
| -------------- | -------------- | --------------------------------------------- |
| `webdav_path`  | `NonEmptyString` | WebDAV path where the image was persisted     |
| `image_bytes`  | `Vec<u8>`        | Raw image bytes (fallback for data URI)       |
| `mime_type`    | `NonEmptyString` | MIME type, e.g. `image/png`                  |
| `share_url`    | `Option<string>`| NextCloud public share link (7-day expiry)    |

After WebDAV upload, the tool calls `create_nextcloud_share_link()` on the
`WebDavClient` which POSTs to `/ocs/v2.php/apps/files_sharing/api/v1/shares`
with `shareType=3`, `permissions=1`, and `expireDate={today+7d}`. The resulting
short URL is stored in `share_url`. The agent loop (main.rs) prefers this URL
for the reply text — appending `![Generated image](share_url)` — which
RocketChat renders as an inline image preview. If share generation fails,
the agent falls back to a `data:` URI as a DDP attachment.
