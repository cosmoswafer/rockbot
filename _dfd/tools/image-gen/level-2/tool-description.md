# Tool Description Generation

## 1. Purpose

Level 2 detail for the happy-path diagram in [main-path](../main-path.md): how
the `image_gen` tool's description and parameter schema are **generated from
config at registry time** — the `[image_providers]` `models` map plus
`[image_model]` defaults are the single source of truth (issue #95). Nothing
model-specific is hardcoded: if a model is added, renamed, or removed from
config, the LLM-visible description follows automatically.

## 2. Diagram

```mermaid
flowchart TD
    CFG[("ConfigStore<br/>[image_providers] models + edit_models<br/>[image_model] default alias")]
    BUILD(ImageModelCatalog::new<br/>main.rs)
    GEN_DESC(Generate tool description<br/>ImageGenTool::new)
    GEN_SCHEMA(Generate parameter schema<br/>ImageGenTool.parameters)
    REG[(Tool Registry)]
    LOOP[Agent Loop]

    CFG -->|"alias to (model_id, edit_model_id?, provider_name) map + default alias"| BUILD
    CFG -->|"defaults (quality, format, num_images, size_tier)"| GEN_DESC
    BUILD -->|"ImageModelCatalog (sorted entries, every provider)"| GEN_DESC
    GEN_DESC -->|"derived description String"| REG
    GEN_DESC -->|"catalog (supports_auto_aspect predicate)"| GEN_SCHEMA
    GEN_SCHEMA -->|"model enum + derived aspect_ratio hint"| REG
    REG -->|"schema + description at list time"| LOOP
```

## 3. Derived text

| Output           | Derived from                                                    | Drift risk (if hardcoded) |
| ---------------- | --------------------------------------------------------------- | ------------------------- |
| Tool description | alias→(model_id, edit_model_id?, provider_name) list from **every** provider entry, default alias, `supports_auto_aspect` | stale model names (e.g. `seedream5`), wrong provider routing hint |
| `model` param description | valid alias list, default alias | enum/description mismatch |
| `model` param `enum` | `allowed_aliases()` (sorted; omitted when catalog empty) | LLM picks a model id that no longer exists |
| `aspect_ratio` param description | `supports_auto_aspect` (any model id containing `seedream/v5`) | auto-dimensional hint absent/present wrongly |

Role-scoped default tables keep chat-only aliases (`gpt`, `qwen`, …) out of
the advertised list by construction — image defaults and chat defaults are
separate maps merged into their respective provider roles at config load
(issue #99). Edit companions are not separate enum options; the description
notes that editing reuses the same alias with a dedicated endpoint where one
exists (issue #100).

The `supports_auto_aspect` predicate reuses the same `seedream/v5` model-id
marker as `FalAiProvider` (see [AI Provider](../../../ai/ai-provider/main-path.md) /
`fal.rs` request builder) so the description hint and the provider's body
generation can never disagree about which models accept `auto_1K`/`auto_2K`.
