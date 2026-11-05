# Image Generation — Shared Structures

## 1. Overview

Generates images via an `ImageProvider` (fal.ai queue API or OpenRouter
Image API / chat-completions fallback), stores them on WebDAV for persistence, and caches the
raw image bytes in the shared `ImageCache`. The agent loop calls `image_gen`
with a prompt and optional parameters; the tool delegates to the provider,
writes to WebDAV, stores to the cache, and returns a minimal result
(`{ok, webdav_path, image_key}`) so the LLM context stays lightweight.

The LLM can select any model from the manifest (`models/image.toml`, issues
#111/#112) per call via an optional `model` alias arg; omitting it falls back to
the manifest's `default = true` alias. The catalog (alias → ordered provider
routes) is a shared type (`ImageModelCatalog`) produced from the manifest at
startup and consumed by the tool — every route names its provider backend
(fal / openrouter, #96). `[[image_providers]]` only supplies credentials and
endpoints; aliases are never declared in config. Routes also carry a tier
(`standard`/`pro`/`turbo`/`flash`), selectable via the optional `tier` arg, and
an optional `provider` arg pins one route. Default model tables in `config.rs`
remain only as a fallback for code paths that predate the manifest; the
manifest always wins.
Editing is **mode-aware per route** (issue #100/#112): a route may carry an
optional edit companion id for providers that genuinely use separate endpoints
(fal); when no candidate route declares one, the generate slug serves edits.

Per-model **wire-level traits** (issue #104, `ImageModelTraits`) cover quirks
the OpenRouter image catalog does not advertise: the Ming Design model id
(`inclusionai/ming-image-0.1-design`) is *self-sized* (the request builder drops
`resolution` / `aspect_ratio`) and *text-to-image-only* (edit calls fail at the
tool parse boundary). The `ming` alias itself is no longer in the eight-alias
roster (#114), but the model-id-driven traits remain available. `FalSizeParam`
covers the fal request-shape split (image_size object vs `aspect_ratio` ±
`resolution`).

The tool's description and `parameters()` schema are **generated from the
catalog at registry time** (issue #95, see
[level-2/tool-description.md](level-2/tool-description.md)) — the manifest is the
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
| `model`       | LLM              | `string` (alias)                              | **Optional.** Manifest alias (`models/image.toml`). Exposed in the tool schema as an `enum` of valid aliases, deprecated ones included (omitted when catalog empty). Resolved via `ImageModelCatalog`; unknown alias → `ToolCallParse` error at the parse boundary. |
| `provider`    | LLM              | `string`                                       | **Optional.** Pins one route provider for the alias (`fal`, `openrouter`). Omitted ⇒ the alias's route order (preferred first) is used, with one failover step. A pin suppresses failover to other providers. |
| `tier`        | LLM              | `ImageTier` (`standard`/`pro`/`turbo`/`flash`) | **Optional.** Selects a route by tier (e.g. `seedream5flash` + `flash`). Omitted ⇒ the alias default route. No matching route → `ToolCallParse` error listing the alias's tiers. |
| `model_id`    | Tool (resolved)  | `string`                                       | Mode-aware slug of the attempt's route (`generate_model_id` or its `edit_model_id`). Set per attempt; failover updates it. `None` only when the catalog has no entry for the default alias (config leniency) — the provider instance then uses its configured default (`provider.model_id()`). |
| `quality`       | Config            | `string`                                       | From `default_quality`                           |
| `output_format` | Config            | `string`                                       | From `default_output_format`                     |
| `num_images`    | Config            | `integer`                                      | From `default_num_images`                        |
| `enable_safety_checker` | Provider   | `Option<boolean>`                              | No config knob since #114 (the roster has no fal seedream/v5 route). Direct callers may set it; `FalAiProvider` sends it only when the model contains `"seedream/v5"`. |

#### `ImageModelCatalog`

Shared type produced at startup from the canonical manifest
(`models/image.toml`, embedded via `include_str!`; issues #111/#112), validated
and converted by `image_manifest.rs`, pruned to providers with usable backends
by `main.rs`, and consumed by the tool — mismatches are compile-time errors.

| Field                | Type                    | Description                                      |
| -------------------- | ----------------------- | ------------------------------------------------ |
| `entries`            | `[ImageModelEntry]`     | One per alias, sorted by alias — stable schema `enum`. See below. |
| `default_alias`      | `string`                | Manifest alias marked `default = true`.          |

`ImageModelEntry` fields:

| Field             | Type             | Description                                                                 |
| ----------------- | ---------------- | --------------------------------------------------------------------------- |
| `alias`           | `string`         | LLM-facing selection name (e.g. `seedream5pro`).                             |
| `label`           | `string`         | Human-facing model name (`GPT Image 2.6 · Flare`) — description text.        |
| `family`          | `string`         | Model family (`openai`, `google`, `qwen`, …).                                |
| `version`         | `string`         | Family version (`2.6`, `2.1`, …).                                            |
| `deprecated`      | `bool`           | Still selectable for one release; marked in the description.                 |
| `routes`          | `[ImageRoute]`   | Ordered provider routes, preferred (`is_default`) first.                     |

`ImageRoute` fields (issues #100/#112):

| Field               | Type             | Description                                                              |
| ------------------- | ---------------- | ------------------------------------------------------------------------ |
| `provider_name`     | `string`         | Owning `[[image_providers]]` entry name — selects the backend.            |
| `generate_model_id` | `string`         | Text-to-image slug sent to the provider.                                  |
| `edit_model_id`     | `Option<string>` | Dedicated edit slug. `None` ⇒ generate-only route (edits reuse the generate slug when no route declares one). |
| `tier`              | `ImageTier`      | `standard` / `pro` / `turbo` / `flash`.                                   |
| `is_default`        | `bool`           | Preferred route of the alias (first route when none is flagged).          |

Manifest lints (hard boot errors, `RockBotError::Config`): non-empty roster,
≥1 route per alias, unique aliases, exactly one default alias, ≤1 default route
per alias, deprecated ≠ default, non-empty labels. Empty `edit = ""` means a
generate-only route and is explicitly allowed — not a broken reference.

`route_candidates(provider?, tier?, is_edit)` filters by the optional LLM pin
and tier, orders the preferred route first, and — for edit calls — stable-sorts
edit-capable routes ahead of generate-only ones. An empty list is a
`ToolCallParse` error naming the alias's providers/tiers.
`resolve(alias)` / `resolve_for_mode(alias, is_edit)` return the preferred
route's `(slug, provider_name)`; `allowed_aliases()` feeds the schema `enum`
(deprecated aliases included); `supports_auto_aspect()` is `true` iff any
route's generate slug contains the `seedream/v5` marker (same constant used by
`FalAiProvider`) and drives the `auto_1K`/`auto_2K` hints.

Execution fails over once (issue #112): a failed route logs the failure and the
next candidate is attempted; the result reports the serving provider/model/tier.
Pro-tier routes are charged to `ProTierUsage` (per-room, UTC day) before the
call and capped by `[image_model] pro_daily_cap` (default 20, `0` disables).

#### `ImageModelTraits`

Per-model wire-level quirks **not** advertised by the OpenRouter image catalog,
derived from the model id once (`ImageModelTraits::for_model_id` in `types.rs`,
issue #104) — the canonical type consumed by both `ImageGenTool` and
`OpenRouterImageProvider`, so tool behaviour and wire body can never disagree.
The only marked model today is the Ming Design model id
(`inclusionai/ming-image-0.1-design` — text-to-image only, model-chosen
dimensions). The `ming` alias is not part of the eight-alias #114 roster. Both
traits default to `false`.

| Field                 | Type   | Description                                                                 |
| --------------------- | ------ | --------------------------------------------------------------------------- |
| `self_sized`          | `bool` | Model picks its own dimensions — the request builder omits `resolution` / `aspect_ratio`; the tool appends a one-line `note` to the result instead of the params. |
| `text_to_image_only`  | `bool` | Model accepts no input references — `image_gen` rejects edit calls at the parse boundary with a `ToolCallParse` text-to-image-only error naming the model id. |

`ImageModelEntry::traits()` exposes the flags for the preferred route; the
provider derives them from the effective model id it is about to send
(`params.model_id` or its configured default), so a direct provider call
without the tool is guarded too.

#### `FalSizeParam`

fal endpoints disagree on how output size is expressed; `FalSizeParam::for_model_id`
in `types.rs` selects the request shape from the wire slug (verified against the
fal OpenAPI schemas, 2026-11-05):

| Variant                       | Models                                                  | Wire fields                        |
| ----------------------------- | ------------------------------------------------------- | ---------------------------------- |
| `ImageSizeObject` (default)   | gpt-image, seedream, moss, …                            | `image_size: {width, height}`      |
| `AspectRatio`                 | `bria/fibo-2/*`                                          | `aspect_ratio: "2:3"`              |
| `AspectRatioWithResolution`   | `google/nano-banana-2.1*`                                 | `aspect_ratio` + `resolution` tier |

#### `ImageGenResult`

The tool returns minimal JSON — no base64. The actual image bytes are in `ImageCache` keyed by `image_key`.

```json
{"ok": true, "webdav_path": "...", "image_key": "call_abc123def4567890", "provider": "fal", "model": "openai/gpt-image-2.6/flare/text-to-image", "tier": "standard", "share_url": "https://..."}
```

`provider`, `model`, and `tier` name the route that actually served the image —
failover is never silent (issue #112).

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
