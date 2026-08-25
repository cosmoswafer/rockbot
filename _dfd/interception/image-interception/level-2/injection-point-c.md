# Injection Point C — ImageGenTool::execute()

## 1. Purpose

`reference_image_key` resolved at the tool level (not in the harness). The LLM
passes the `image_key` from a prior `image_gen` result; the tool looks it up in
`ImageCache`, uploads the data URI to the provider's CDN, and appends
the resulting `https://` URL to `image_urls`:

## 2. Diagram

```mermaid
flowchart LR
    REF_KEY["reference_image_key<br/>(image_key from prev result)"]
    CACHE_LOOKUP["ImageCache lookup<br/>by call_id"]
    UPLOAD["upload_data_uri → CDN"]
    APPEND["Append https:// URL<br/>to image_urls"]

    REF_KEY --> CACHE_LOOKUP
    CACHE_LOOKUP -->|"GeneratedImage"| UPLOAD
    UPLOAD -->|"https:// URL"| APPEND
```
