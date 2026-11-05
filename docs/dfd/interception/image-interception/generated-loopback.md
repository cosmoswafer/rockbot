# Generated Image Loopback

## 1. Purpose

Generated images can be reused for editing via two paths:

1. **Share URL**: the `image_gen` tool exposes the NextCloud `share_url` in its
   result JSON, which the LLM can pass back in `image_urls` on a subsequent call.
2. **Reference Key**: the LLM passes `reference_image_key` (the `image_key` from
   a prior `image_gen` result), and the tool looks up the cached image bytes in
   `ImageCache` and uploads them to the provider's CDN.

## 2. Diagram

```mermaid
flowchart LR
    GEN["image_gen Result<br/>{share_url, image_key}"]
    LLM[LLM sees share_url + image_key]
    URL_PATH["Next image_gen Call<br/>image_urls: share_url"]
    KEY_PATH["Next image_gen Call<br/>reference_image_key: image_key"]
    PROVIDER_URL[Provider Receives<br/>https:// URL for img2img]
    PROVIDER_KEY[Provider Receives<br/>uploaded https:// URL<br/>from ImageCache lookup]

    GEN -->|"share_url + image_key in result JSON"| LLM
    LLM -->|"passes in image_urls"| URL_PATH
    LLM -->|"passes reference_image_key"| KEY_PATH
    URL_PATH -->|"inject_image_urls_from_refs<br/>merges with agent URLs"| PROVIDER_URL
    KEY_PATH -->|"ImageCache lookup → upload to CDN"| PROVIDER_KEY
```

The loopback path: `image_gen` → `ImageCache` + tool result → LLM includes
`share_url` in next call → `inject_image_urls_from_refs` merges it →
provider receives `https://` URL (no re-upload needed).
