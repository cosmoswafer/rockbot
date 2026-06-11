# fal.ai Integration Notes

## API endpoint patterns

The fal.ai queue API uses the following URL patterns:

| Operation | Method | URL pattern |
|---|---|---|
| Submit request | `POST` | `{base_url}/{model_id}` |
| Poll status | `GET` | Use `status_url` from submit response |
| Fetch result | `GET` | Use `response_url` from submit response |

## Critical: Use API-returned URLs, do NOT reconstruct

The submit response includes `status_url` and `response_url` fields. These must be used **as-is** — do not construct URLs from `base_url + model_id + request_id`.

Example submit response for `openai/gpt-image-2/edit`:

```json
{
  "request_id": "019eb448-b6c5-7fe2-bc14-2b2498ae92d0",
  "status_url": "https://queue.fal.run/openai/gpt-image-2/requests/019eb448-.../status",
  "response_url": "https://queue.fal.run/openai/gpt-image-2/requests/019eb448-..."
}
```

Note the model portion is `openai/gpt-image-2` (without `/edit`). The `/edit` suffix is an action on the base model, and the API strips it from the status/response URLs. Reconstructing with `{model_id}/requests/{id}/status` would produce `openai/gpt-image-2/edit/requests/{id}/status`, which returns an empty **405** response, causing `response.json()` to fail with "EOF while parsing a value".

## Status polling

- Polling interval: 2 seconds
- Max attempts: 90 (3 minutes total)
- Exit conditions: `COMPLETED`, `FAILED`, or timeout

## Real-image edit flow

1. Upload source image to fal storage via `POST {storage_url}/storage/upload/initiate` → `PUT {upload_url}`
2. Submit edit request with `image_urls` pointing to uploaded file
3. Poll `status_url` until `COMPLETED`
4. Fetch result from `response_url` — extracts `images[0].url`
5. Download result from fal CDN, re-upload to WebDAV

## Test

`crate-rockbot/tests/fal_real.rs` (`#[ignore]`) exercises the full flow against the real API using `_docs/ref_img/p1.png`:
- Uploads p1.png to fal CDN (~1.3 MB)
- Submits edit: "Change the girl's red sweater outfit to Rei Ayanami's iconic blue plugsuit cosplay from Neon Genesis Evangelion"
- Polls every 2s (typical queue wait ~2–3 minutes)
- Asserts result is an HTTPS URL

## Turnaround times

| Step | Typical time |
|---|---|
| Upload | <1s |
| Queue wait | 30s–180s |
| Inference | ~30ms |
| Download + cleanup | <1s |
