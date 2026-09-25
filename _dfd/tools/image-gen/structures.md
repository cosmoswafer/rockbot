# Image Generation — Shared Structures

## 1. Overview

Generates images via an `ImageProvider` (fal.ai queue API or OpenRouter
Image API / chat-completions fallback), stores them on WebDAV for persistence, and caches the
raw image bytes in the shared `ImageCache`. The agent loop calls `image_gen`
with a prompt and optional parameters; the tool delegates to the provider,
writes to WebDAV, stores to the cache, and returns a minimal result
(`{ok, webdav_path, image_key}`) so the LLM context stays lightweight.

The LLM can select any model from **all configured `[[image_providers]]`**
per call via an optional `model` alias arg; omitting it falls back to
`[image_model] default_text_model` of the default provider. The catalog
(alias → model ids + provider name) is a shared type (`ImageModelCatalog`)
produced from config at startup and consumed by the tool — every model is
routed to its own provider's backend (issue #96), so the concatenated catalog
spans the fal and openrouter entries (the only supported backend kinds; other
provider names are skipped at registry with a warning). Default model tables
are **role-scoped** (issue #99): image providers inherit only image-capable
models — chat aliases (`gpt`, `qwen`, `minimax`, …) exist solely in the chat
role's table and can never leak into the image catalog.
Editing is **unified per alias** (issue #100): an alias may carry an optional
edit companion id for providers that genuinely use separate endpoints
(currently only fal); otherwise the same model id serves both modes and no
`*_edit` alias is exposed.

Per-model **wire-level traits** (issue #104, `ImageModelTraits`) cover quirks
the OpenRouter image catalog does not advertise: *self-sized* models
(`ming` → `inclusionai/ming-image-0.1-design`, Ming Image 0.1 Design) choose
their own output dimensions, so the request builder drops `resolution` /
`aspect_ratio`, and *text-to-image-only* models reject edit calls at the tool
parse boundary with a clear error. Neither trait affects catalog routing — the
alias is registered once in `models` (never in `edit_models`) and every call
still resolves through `ImageModelCatalog`.

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
| `image_size`    | Tool (resolved)  | preset name → pixels                         | Resolved from LLM's `aspect_ratio` per-provider. Hidden from LLM. Dropped on the wire by self-sized models (#104). |
| `size_tier`     | Config            | `"4K"`, `"2K"`, `"1K"`                        | Resolution tier for OpenRouter. Set from `default_image_size_tier`. Ignored by fal and by self-sized OpenRouter models (#104). |
| `room_id`       | Harness           | `string`                                       | Room UUID for image storage (injected if omitted). **Note:** injected at execute time, not stored in the Rust struct. |
| `webdav_dir`    | Harness           | `string`                                       | Type-prefixed room path (injected; falls back to room_id). **Note:** injected at execute time, not stored in the Rust struct; also absent from the LLM-facing tool schema. |
| `image_cache_key`| Harness          | `string`                                       | Tool call_id — used as ImageCache lookup key. **Note:** injected at execute time, not in LLM-facing schema. |
| `image_urls`    | Harness (auto)    | `[]string`                                     | Injected from 5 converging sources (see §2e): user attachments, vision/WebDAV pool, agent-provided URLs, message image URLs (auto-injected unconditionally), and `reference_image_key` (ImageCache lookup) |
| `reference_image_key` | LLM | `string` | Alternative to `image_urls` — the `image_key` from a prior `image_gen` result. Looked up in ImageCache; data URI uploaded to provider CDN. Tool-level arg — not a field on the Rust struct; resolved before `ImageGenParams` construction. |
| `model`       | LLM              | `string` (alias)                              | **Optional.** Config alias for the active image provider's `models` catalog. Exposed in the tool schema as an `enum` of valid aliases (omitted when catalog empty). Resolved via `ImageModelCatalog`; unknown alias → `ToolCallParse` error at the parse boundary. |
| `model_id`    | Tool (resolved)  | `string`                                       | Mode-aware id from the resolved alias's entry (`resolve_for_mode`): edit companion when editing and paired (#100), else the plain id. Stays `None` only when no alias was given and the default alias is unresolvable — the provider instance then uses its configured default (`provider.model_id()`). |
| `quality`       | Config            | `string`                                       | From `default_quality`                           |
| `output_format` | Config            | `string`                                       | From `default_output_format`                     |
| `num_images`    | Config            | `integer`                                      | From `default_num_images`                        |
| `enable_safety_checker` | Config     | `boolean`                                      | From `default_enable_safety_checker` (default `false`). Sent by `FalAiProvider` only when model contains `"seedream/v5"`. |

#### `ImageModelCatalog`

Shared type produced at startup by joining the `models` map of **every**
configured `[[image_providers]]` entry backed by a supported provider kind
(fal / openrouter), tagging each alias with its provider name and optional
edit companion, plus the `[image_model]` default alias. Defined in `types.rs`
— produced by `main.rs` and consumed by the tool, so mismatches are
compile-time errors.

| Field                | Type                    | Description                                      |
| -------------------- | ----------------------- | ------------------------------------------------ |
| `entries`            | `[ImageModelEntry]`     | One per model family, sorted by alias — stable schema `enum`. See below. |
| `default_alias`      | `string`                | `[image_model] default_text_model` (fallback alias) |

`ImageModelEntry` fields:

| Field             | Type             | Description                                                                 |
| ----------------- | ---------------- | --------------------------------------------------------------------------- |
| `alias`           | `string`         | LLM-facing selection name (e.g. `seedream5`).                                |
| `model_id`        | `string`         | Text-to-image model id sent to the provider.                                 |
| `edit_model_id`   | `Option<string>` | Dedicated edit-endpoint id. `None` ⇒ edits reuse `model_id` (issue #100).    |
| `provider_name`   | `string`         | Owning `[[image_providers]]` entry name — selects the backend.               |

Config shape: `models` maps alias → t2i id; the sibling `edit_models` map
(serde default: empty) maps alias → edit companion id for providers with
genuinely separate endpoints (fal only). Role-scoped default tables in
`config.rs` guarantee chat-only aliases never enter image-provider defaults
(#99); they are probed against the fal model registry when the fal map changes.
The fal defaults carry the GPT Image 2.5 pair (issue #102): `gptimage` →
`openai/gpt-image-2.5/flare/{text-to-image,edit}` (**Flare**, the
`[image_model]` default) and `sunburst` →
`openai/gpt-image-2.5/sunburst/{text-to-image,edit}` (Sunburst).

`resolve(alias)` returns `(model_id, provider_name)` or `None` — the tool
rejects unknown aliases with a `ToolCallParse` error listing the valid aliases;
each resolved pair selects that provider's backend.
`resolve_for_mode(alias, is_edit)` swaps in the edit companion id when the
call carries input images (`Option::unwrap_or(model_id)`), so edit requests
always hit the correct endpoint id without the LLM choosing a separate alias.
`allowed_aliases()` feeds the tool schema `enum`; `model_ids()` /
`provider_names()` expose the resolved ids and provider tags (index-aligned
with `allowed_aliases()`) for the derived tool description. Duplicate aliases
across provider entries are dropped with a warning (first defined wins).
`supports_auto_aspect()` is a derived capability flag — `true` iff any entry's
model id contains the `seedream/v5` marker (same constant used by
`FalAiProvider`). It drives the `auto_1K`/`auto_2K` hint in the tool and
`aspect_ratio` parameter descriptions; when `false`, no auto-dimensional
strings are advertised.

#### `ImageModelTraits`

Per-model wire-level quirks **not** advertised by the OpenRouter image catalog,
derived from the model id once (`ImageModelTraits::for_model_id` in `types.rs`,
issue #104) — the canonical type consumed by both `ImageGenTool` and
`OpenRouterImageProvider`, so tool behaviour and wire body can never disagree.
The only marked model today is `ming` (`inclusionai/ming-image-0.1-design`,
Ming Image 0.1 Design — text-to-image only, model-chosen dimensions, promo
`$0.00` pricing on NovitaAI). Both traits default to `false`.

| Field                 | Type   | Description                                                                 |
| --------------------- | ------ | --------------------------------------------------------------------------- |
| `self_sized`          | `bool` | Model picks its own dimensions — the request builder omits `resolution` / `aspect_ratio`; the tool appends a one-line `note` to the result instead of the params. |
| `text_to_image_only`  | `bool` | Model accepts no input references — `image_gen` rejects edit calls at the parse boundary with a `ToolCallParse` text-to-image-only error naming the model id. |

`ImageModelEntry::traits()` exposes the flags for an entry; the provider
derives them from the effective model id it is about to send
(`params.model_id` or its configured default), so a direct provider call
without the tool is guarded too.

#### `ImageGenResult`

The tool returns minimal JSON — no base64. The actual image bytes are in `ImageCache` keyed by `image_key`.

```json
{"ok": true, "webdav_path": "...", "image_key": "call_abc123def4567890", "share_url": "https://..."}
```

The `share_url` field is conditionally present — included only when a NextCloud share link was successfully created for the generated image. It is absent when share generation failed (fallback to DDP attachment path).

The `note` field is conditionally present — included only when the resolved
model is self-sized (`ImageModelTraits.self_sized`, issue #104). It is a
one-line hint that the model chose the output dimensions and the
`aspect_ratio` / size-tier params were omitted (preferred behaviour per the
issue: silent drop + hint, rather than a hard error).

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
short URL with a `/download` suffix is stored in `share_url` — probed in
production (issue #97): the `/preview` endpoint serves better headers (HTTP
200, real MIME, `Content-Disposition: inline`) but RocketChat fails to render
it inline, while `/download` renders fine. The agent loop (main.rs) prefers
this URL for the reply text — appending `![Generated image](share_url)` —
which RocketChat renders as an inline image preview. If share generation
fails, the agent falls back to a `data:` URI as a DDP attachment.
