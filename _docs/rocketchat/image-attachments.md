# Image Attachment Reception via DDP WebSocket

How the RocketChat DDP WebSocket client receives messages with image/file attachments.

## DDP event structure

When a user sends a message with an image, the `stream-room-messages` subscription emits a `"changed"` event with the full message object in `fields.args[0]`.

### Raw payload (example: message "B78" with a PNG screenshot)

```json
{
  "msg": "changed",
  "collection": "stream-room-messages",
  "id": "id",
  "fields": {
    "eventName": "__my_messages__",
    "args": [
      {
        "_id": "3YABZRsrguXtKSSRL",
        "rid": "KkMWxgv32j6m6n2Ce",
        "msg": "B78",
        "ts": { "$date": 1781113235311 },
        "u": { "_id": "viGqbf8p33xMqHik6", "username": "saru", "name": "🐵 猴一隻" },
        "file": {
          "_id": "6a29a19267d3d1722cebb263",
          "name": "Clipboard - June 11th, 2026 1:40 AM.png",
          "type": "image/png",
          "size": 2799744,
          "format": "png",
          "typeGroup": "image"
        },
        "files": [
          { "_id": "6a29a19267d3d1722cebb263", "name": "Clipboard...png", "type": "image/png", "size": 2799744, "format": "png", "typeGroup": "image" },
          { "_id": "6a29a19567d3d1722cebb264", "name": "thumb-Clipboard...png", "type": "image/png", "size": 237037, "format": "png", "typeGroup": "thumb" }
        ],
        "attachments": [
          {
            "title": "Clipboard - June 11th, 2026 1:40 AM.png",
            "title_link": "/file-upload/6a29a19267d3d1722cebb263/Clipboard%20-%20June%2011th,%202026%201:40%20AM.png",
            "title_link_download": true,
            "image_url": "/file-upload/6a29a19567d3d1722cebb264/Clipboard%20-%20June%2011th,%202026%201:40%20AM.png",
            "image_type": "image/png",
            "image_size": 2799744,
            "image_dimensions": { "width": 240, "height": 360 },
            "image_preview": "/9j/2wBDAAYEBQYFBAYGBQ...",
            "type": "file",
            "fileId": "6a29a19267d3d1722cebb263"
          }
        ],
        "groupable": false,
        "mentions": [],
        "channels": [],
        "urls": [],
        "md": [{ "type": "PARAGRAPH", "value": [{ "type": "PLAIN_TEXT", "value": "B78" }] }],
        "_updatedAt": { "$date": 1781113238441 }
      },
      {
        "roomParticipant": true,
        "roomType": "p",
        "roomName": "atomkb"
      }
    ]
  }
}
```

## Key fields

### `args[0].file` — single file metadata

Single object with the primary uploaded file:

| Field | Type | Description |
|-------|------|-------------|
| `_id` | string | File ID on the RocketChat server |
| `name` | string | Original filename |
| `type` | string | MIME type (e.g. `image/png`) |
| `size` | int | File size in bytes |
| `format` | string | File extension (e.g. `png`) |
| `typeGroup` | string | Group: `"image"`, `"video"`, `"audio"`, `"document"` |

### `args[0].files` — array of all file variants

Contains multiple entries — typically the original file (`typeGroup: "image"`) plus a thumbnail variant (`typeGroup: "thumb"`).

### `args[0].attachments` — array of attachment objects

| Field | Value | Notes |
|-------|-------|-------|
| `image_url` | `/file-upload/{thumb_file_id}/{name}` | **Thumbnail variant** — lower resolution |
| `title_link` | `/file-upload/{orig_file_id}/{name}` | **Original file** — use this for full quality |
| `title_link_download` | bool | `true` means clicking title triggers download |
| `image_preview` | base64 string | Small inline preview (low-res data URI) |
| `image_dimensions` | `{width, height}` | Pixel dimensions of the preview image |
| `image_type` | string | MIME type |
| `image_size` | int | Original file size in bytes |
| `type` | string | Always `"file"` for uploads |
| `fileId` | string | Back-reference to the original `file._id` |

## Download URL construction

RocketChat serves files at the server's base URL with the `/file-upload/` path prefix.

For the original file, use `attachments[0].title_link`:
```
{server_base_url}{title_link}
```

For the thumbnail, use `attachments[0].image_url`:
```
{server_base_url}{image_url}
```

## Verified download results

Both endpoints were tested against a real RocketChat v8.4 server with auth headers:

| Field | Source | Size | Dimensions | Auth |
|-------|--------|------|------------|------|
| `image_url` (thumbnail) | `args[0].attachments[0].image_url` | 237 KB | 240×360 | Required |
| `title_link` (original) | `args[0].attachments[0].title_link` | 2.8 MB | 1024×1536 | Required |

- The `image_dimensions` field in the DDP payload (`{width: 240, height: 360}`) describes the **thumbnail**, not the original file.
- Both endpoints require `X-Auth-Token` + `X-User-Id` HTTP headers — returns **403** without auth.
- Auth tokens come from the DDP `login` method response: `result.token` and `result.id`.

### curl example

```bash
curl -o image.png \
  -H "X-Auth-Token: <token>" \
  -H "X-User-Id: <user_id>" \
  "https://rc.example.com/file-upload/<file_id>/<filename>"
```

## Current parser gap

The current `MessageFilter::parse_message()` in `crate-rocketchat/src/types.rs` only extracts:
- `msg`, `rid`, `u._id`, `u.username`, `alias`, `ts`, `roomName`, `fname`

It does **not** extract `file`, `files`, or `attachments`. The `IncomingMessage` struct has no fields to carry them.
