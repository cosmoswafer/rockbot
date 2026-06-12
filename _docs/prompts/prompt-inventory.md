# Prompt Inventory — rockbot

All prompts and prompt-adjacent strings in the Rust codebase, organized by what they do.

---

## 1. System Prompt (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:20-64`
**Constant:** `DEFAULT_SYSTEM_PROMPT`
**Sent to:** AI provider as the `system` role message in `ChatRequest.messages`
**Used via:** `build_system_prompt()` (line 597) → `MemoryManager::build_context()` → prepended as first message in context

Note: `{name}`, `{max_context_mb}`, and `{max_iterations}` are replaced at runtime with config values via `build_system_prompt()`.

```
You are {name}, a helpful AI assistant running on a RocketChat server. \
You respond to DMs and @mentions concisely and helpfully. \
Context space is limited to ~{max_context_mb}MB / 1M tokens. Keep your \
reasoning brief and avoid verbose explanations. Use tools to fetch \
information rather than guessing. You have up to {max_iterations} iterations \
per task — plan your tool calls efficiently. \
When you need the current date or time, use the datetime tool. \
When you need information from the web, use the web_search tool. \
When you need to fetch a URL, use web_fetch. \
When you need to describe or analyze an image, use the vision tool. \
Do NOT use vision just to identify an image before editing — when a user \
shares an image URL and asks to modify/edit/transform it, call image_gen directly. \
The harness will automatically provide the image URL as the image_urls parameter. \
When you need to read, write, list, or manage files on remote storage, use the webdav tool. \
When you need to manage calendar events or todo tasks, use the calendar tool. \
When you need to generate an image, use the image_gen tool. \
When a user sends an image and asks to edit, modify, transform, or use it \
as a basis for image generation, use the image_gen tool — user-attached images \
appear as markdown ![image_name](image_name) in the conversation. Reference the \
image by its image_name in your prompt (e.g. \"edit image1.png to add a hat\"). \
The harness will automatically resolve image_name references and image URLs \
to the actual images. \
If the user asks to edit a previously generated image (no new attachment), \
you MUST include the image CDN URL from the previous result in the \
image_urls parameter yourself. \
The image_gen tool returns a WebDAV path and an image_key — \
always share the image with the user in markdown image format \
as `![{description}]({image_key})` so they can view the image inline. \
When a user says !soul or asks to save or update preferences, identity, or facts, use the edit_soul tool. \
edit_soul performs a full replace — it overwrites the entire soul with the content you provide. \
The soul is a flat enumeration list — each line is a \"- \" bullet item with no sub-headings. \
When setting your soul, always use this exact template: \
\"# Soul Memory\\n\\n- My name is YourName ✨\\n- (optional)\\n- (optional)\\n- (optional)\\n- (optional)\". \
Your display name is extracted by the regex \\\"My name is (.+)\\\" — \
the first item must start with \"My name is ...\" and becomes your name. \
Keep the name under 32 characters. \
When a user asks you to remember something, shares notes, or says !remember, !note, !save or shares important \
information worth persisting, use the save_knowledge tool. \
When a user says !forget or asks to remove something you learned, \
use the forget_knowledge tool. \
When you need to recall previously saved knowledge, use the recall_knowledge tool. \
Answer in the same language as the user. \
Keep responses clear and to the point.\
```

---

## 2. User Message Template (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:172-234`
**Template:** `ChatMessage::user(format!("{}: {}", sender_name, clean_text))`
**Role:** `user`
**Purpose:** Wraps every incoming RocketChat message as `"SenderName: message text"` before appending to history. Preserves sender identity in group chats.

When image attachments are present (harness.rs:214-231), they are downloaded via
`download_attachment_refs()` and encoded as data URIs, then injected into the
conversation as markdown `![image_name](image_name)` labels. The user message is
created as `ChatMessage::user_with_images` with the prompt including an
`Attached:` line listing the image labels. If the text is empty, the prompt
defaults to `"SenderName: Describe this image in detail.\nAttached: ![name](name)"`.

The harness later resolves `image_name` references in `image_gen` tool calls
by matching them against these cached data URIs.

Example output: `"Alice: what's the weather in Tokyo?"`

---

## 3. Tool Descriptions (sent to AI provider in tool definitions)

### 3a. `datetime`
**File:** `crate-rockbot/src/tools/datetime.rs:216-220`
```
Get the current UTC date and time. Returns ISO 8601 timestamp, human-readable date with weekday, Unix epoch seconds, calendar month view, week number (ISO 8601), or weekday list. Supports week_offset for prev/next week views.
```

### 3b. `web_search`
**File:** `crate-rockbot/src/tools/web_search.rs:188-191`
```
Search the web using Exa. Returns ranked results with titles, URLs, highlights, and dates.
Supports optional type (auto/fast/deep), num_results, and contents_mode (highlights/text/deep) parameters.
```

### 3c. `web_fetch`
**File:** `crate-rockbot/src/tools/web_fetch.rs:648-652`
```
Fetch or send content from/to a URL like curl. Supports GET, POST, PUT, PATCH, DELETE,
HEAD, OPTIONS with custom headers, JSON body, raw body, and file upload from WebDAV.
Response can be saved to WebDAV. Three output formats: json (structured with metadata),
markdown (HTML converted to markdown for AI consumption), raw (unmodified response text).
```

### 3d. `vision`
**File:** `crate-rockbot/src/tools/vision.rs:116-120`
```
Fetch an image from a WebDAV file path or public URL and return it as a base64 markdown image tag.
Use this to retrieve images the user is asking about from WebDAV storage or external URLs.
User attachments are already visible to you — only use this tool for images at explicit URLs.
```

### 3e. `webdav`
**File:** `crate-rockbot/src/tools/webdav.rs:198-205`
```
Manage files on remote WebDAV storage (NextCloud). Each room has its own file space —
paths are automatically scoped. Actions: read (get file content), write (create/overwrite
a file), edit (replace oldString with newString — reads file first, fails if oldString not
found or matches multiple times, 500 KB max), list (list directory contents), mkdir
(create directory tree), delete (remove file/directory), exists (check if path exists).
```

### 3f. `calendar`
**File:** `crate-rockbot/src/tools/calendar.rs:219-233`
```
Manage calendar events on NextCloud CalDAV. Events are stored per-room — each room has its own calendar auto-created on first use. Actions: list_events (list events in a date range), get_event (fetch a single event by UID), add_event (create a new event), update_event (modify an existing event by UID), delete_event (remove an event by UID). add_event requires summary, dtstart (ISO 8601, UTC), dtend (ISO 8601, UTC). update_event uses merge semantics: specify only the fields you want to change; omitted fields keep their existing values. Optional for both: description, location, rrule (recurrence rule, RFC 5545), reminder_minutes (e.g. 15). All date/time values must be in UTC — use the Z suffix (e.g. 20260615T140000Z) or omit seconds (e.g. 20260601T000000Z). Floating times (without Z) are not supported.
```

### 3g. `image_gen`
**File:** `crate-rockbot/src/tools/image_gen.rs:125-133`
```
Generate or edit an image. For text-to-image, provide a prompt
and optional image_size. To edit or transform an image, the user's
attachments are automatically provided as image_urls — just describe
what to do in the prompt.
Returns a JSON object: {"ok": true, "image_key": "...", "webdav_path": "..."}.
Always share the image with the user in markdown image format
as `![{description}]({image_key})` so they can view the image inline.
After a successful image_gen call, respond to the user — do not call image_gen again.
```

Note: `image_size` is NOT exposed to the LLM as a tool parameter — it is set from `[image_model]` config (`default_image_size`, `default_image_size_tier`). The harness injects `room_id` and `image_cache_key` automatically. `image_urls` are auto-injected from message attachments.

### 3h. `edit_soul`
**File:** `crate-rockbot/src/tools/edit_soul.rs:38-48`
```
Overwrite the bot's permanent soul memory for this room.
The soul is a flat enumeration list — each line is a "- " bullet item.
Provide the full soul.md content using this template:
# Soul Memory

- My name is YourName ✨
- (optional preference)
- (optional fact)
- (optional preference)
- (optional fact)
```

### 3i. `save_knowledge`
**File:** `crate-rockbot/src/tools/save_knowledge.rs:39-44`
```
Save a piece of knowledge (skill, secret, or note) for future reference. Use this when the user says 'remember', 'learn', or shares important information worth persisting. Each entry gets a .md file and is indexed for contextual retrieval.
```

### 3j. `forget_knowledge`
**File:** `crate-rockbot/src/tools/forget_knowledge.rs:26-30`
```
Remove a previously saved knowledge entry. Provide the topic title of the entry to delete. The .md file is deleted and the entry is removed from the knowledge index.
```

### 3k. `recall_knowledge`
**File:** `crate-rockbot/src/tools/recall_knowledge.rs:26-29`
```
Search the knowledge index for entries matching a query. If no query is given, returns all stored knowledge entries. Matches by topic title, when_useful description, and tags.
```

---

## 4. Tool Parameter Descriptions (sent to AI provider in tool schema)

| File | Lines | Tool | Parameter | Description |
|------|-------|------|-----------|-------------|
| `datetime.rs` | 226, 229 | `datetime` | `format` | Output format: iso (ISO 8601), human (readable with weekday), unix (epoch seconds), full (all three). Default: full |
| `web_search.rs` | 200 | `web_search` | `query` | The search query to execute |
| `web_search.rs` | 205 | `web_search` | `type` | Search type: auto (balanced with autoprompt), fast (quick results), deep (comprehensive). Default: auto |
| `web_search.rs` | 211 | `web_search` | `num_results` | Number of results to return (default: 5, max: 20) |
| `web_search.rs` | 216 | `web_search` | `contents_mode` | Content detail level: highlights (default), text (full text), deep (thorough analysis) |
| `web_fetch.rs` | 658 | `web_fetch` | `url` | The URL to fetch |
| `web_fetch.rs` | 663-665 | `web_fetch` | `format` | Output format: json returns structured metadata, markdown converts HTML to markdown for AI, raw returns unmodified text (default: raw) |
| `web_fetch.rs` | 668 | `web_fetch` | `verify` | Perform a web search to cross-verify content (default: false) |
| `web_fetch.rs` | 671 | `web_fetch` | `method` | HTTP method: GET (default), POST, PUT, PATCH, DELETE, HEAD, OPTIONS |
| `web_fetch.rs` | 676 | `web_fetch` | `headers` | JSON object of HTTP headers (e.g. {"Authorization": "Bearer ..."}) |
| `web_fetch.rs` | 680 | `web_fetch` | `body` | Request body for POST/PUT/PATCH (string or JSON) |
| `web_fetch.rs` | 684 | `web_fetch` | `save_to` | WebDAV path to save response (optional, relative to room root) |
| `web_fetch.rs` | 688 | `web_fetch` | `upload_file_path` | WebDAV file path to upload as multipart body (optional, relative to room root) |
| `vision.rs` | 130-131 | `vision` | `url` | URL of the image to fetch (public web or WebDAV file) |
| `webdav.rs` | 212-215 | `webdav` | `action` | The WebDAV operation to perform |
| `webdav.rs` | 217-219 | `webdav` | `room_id` | Room ID for scoping the operation (injected automatically if omitted) |
| `webdav.rs` | 221-223 | `webdav` | `path` | File or directory path relative to the room root |
| `webdav.rs` | 225-227 | `webdav` | `content` | File content to write (required for 'write' action) |
| `webdav.rs` | 229-231 | `webdav` | `oldString` | Exact text to find and replace (required for 'edit' action, must be unique in the file) |
| `webdav.rs` | 233-235 | `webdav` | `newString` | Replacement text (required for 'edit' action) |
| `calendar.rs` | 246 | `calendar` | `action` | Calendar operation to perform |
| `calendar.rs` | 250 | `calendar` | `start` | Start of date range in ISO 8601 UTC (e.g. 20260601T000000Z). Used by list_events. |
| `calendar.rs` | 254 | `calendar` | `end` | End of date range in ISO 8601 UTC. Used by list_events. |
| `calendar.rs` | 258 | `calendar` | `uid` | Event UID. Required for update_event and delete_event. |
| `calendar.rs` | 262 | `calendar` | `summary` | Event title/summary. Required for add_event and update_event. |
| `calendar.rs` | 266 | `calendar` | `dtstart` | Event start in ISO 8601 UTC (e.g. 20260615T140000Z). Required for add_event. |
| `calendar.rs` | 270 | `calendar` | `dtend` | Event end in ISO 8601 UTC. Required for add_event. |
| `calendar.rs` | 274 | `calendar` | `description` | Optional event description/details. |
| `calendar.rs` | 278 | `calendar` | `location` | Optional event location. |
| `calendar.rs` | 282 | `calendar` | `rrule` | Optional recurrence rule in RFC 5545 format (e.g. FREQ=WEEKLY;BYDAY=MO). |
| `calendar.rs` | 286 | `calendar` | `reminder_minutes` | Optional reminder in minutes before event (e.g. 15). |
| `image_gen.rs` | 140-142 | `image_gen` | `prompt` | Text description of the image to generate |
| `image_gen.rs` | 144-146 | `image_gen` | `room_id` | Room ID for image storage (injected automatically if omitted) |
| `image_gen.rs` | 148-151 | `image_gen` | `image_urls` | Image URLs for editing/transformations. When the user sends images, they are automatically injected. Do NOT try to reference data URIs from vision context — they will be provided automatically. |
| `edit_soul.rs` | 55-57 | `edit_soul` | `content` | Full soul.md content following the template: # Soul Memory\n\n- My name is Name ✨\n- ...\n- ..." |
| `edit_soul.rs` | 59-61 | `edit_soul` | `webdav_dir` | Room WebDAV directory key (injected automatically) |
| `save_knowledge.rs` | 50-53 | `save_knowledge` | `category` | Knowledge category: skill (procedural/how-to), secret (credential/sensitive), note (factual info) |
| `save_knowledge.rs` | 56-58 | `save_knowledge` | `topic` | Short title or topic for the entry (e.g. 'DB API', 'Build Commands') |
| `save_knowledge.rs` | 60-62 | `save_knowledge` | `content` | Markdown body of the knowledge entry |
| `save_knowledge.rs` | 64-67 | `save_knowledge` | `when_useful` | Describe the situation that makes this knowledge relevant, used for automatic retrieval (e.g. 'when calling the database API') |
| `save_knowledge.rs` | 69-71 | `save_knowledge` | `tags` | Comma-separated keywords for search (e.g. 'api, database, python') |
| `save_knowledge.rs` | 73-76 | `save_knowledge` | `priority` | Knowledge priority: P0 (highest, always recalled), P1 (high), P2 (medium, default), P3 (low). Higher priority means more frequently recalled. |
| `forget_knowledge.rs` | 36-38 | `forget_knowledge` | `topic` | Title or topic of the knowledge entry to delete |
| `recall_knowledge.rs` | 36-39 | `recall_knowledge` | `query` | Topic or keyword to search for in knowledge entries. Leave empty to retrieve all entries. |

---

## 5. Default Vision Prompt (fallback for tool execution)

**File:** `crate-rockbot/src/harness.rs:224`
```
{}: Describe this image in detail.
```
**Used when:** A user sends image attachments with no text. The harness constructs a user message as `"{sender_name}: Describe this image in detail.\nAttached: ..."` with the attached image labels. Not used by the `vision` tool itself (vision tool has no `prompt` parameter).

---

## 6. Fallback / Error Messages (returned to RocketChat user)

**File:** `crate-rockbot/src/harness.rs`

| Line | Condition | Text |
|------|-----------|------|
| 280 | Max agent iterations exceeded | `I'm sorry, I got stuck in a loop. Could you rephrase your request?` |
| 476 | AI returned empty text (stored in history) | `(no response)` |
| 483 | AI returned empty text (user-facing) | `I processed your request but received an empty response.` |
| 496 | AI returned no text at all | `I received a response but it was empty. Please try again.` |
| 552 | AI provider error (dynamic) | `I encountered an error: {e}. Please try again.` |

**File:** `crate-rockbot/src/main.rs`

| Line | Condition | Text |
|------|-----------|------|
| 549 | Outer catch-all for message processing | `Error processing message: {e}` |

---

## 7. Image Generation Request Body (sent to image provider API)

**Files:** `crate-rockbot/src/types.rs:349-357` (ImageGenParams struct),
`crate-rockbot/src/provider/fal.rs:80-219` (fal.ai submit_request),
`crate-rockbot/src/provider/openrouter.rs:779-840` (OpenRouter build_request_body)

The provider-agnostic `ImageGenParams` struct:
```json
{
  "prompt": "<user provided>",
  "quality": "<from config>",
  "output_format": "<from config>",
  "num_images": <from config>,
  "image_size": "<preset from config, hidden from LLM>",
  "size_tier": "<4K|2K|1K from config, OpenRouter only>",
  "image_urls": ["<optional URL(s) for img2img>"]
}
```

**fal.ai** (POST `{base_url}/{model_id}`): resolved pixel dimensions in submit body.
**OpenRouter** (POST `/chat/completions`): maps to `image_config.aspect_ratio` + `image_config.image_size: "4K"`.

Tool result returned to LLM (image_gen.rs):
```json
{"ok": true, "webdav_path": "<WebDAV path>", "image_key": "<call_id>"}
```

The LLM includes `image_key` in its reply markdown. The agent loop (main.rs)
replaces it with a NextCloud share URL (`/download` suffix, 7-day expiry)
before sending to RocketChat.

---

## 8. Memory / Context Prompts (sent to AI provider as system messages)

### 8a. Soul memory prefix
**File:** `crate-rockbot/src/memory.rs:239-242`
**Type:** Dynamic — loaded from WebDAV `memory/soul.md`
```
[Core memory — permanent preferences, identity, and facts]
{content from soul.md}
```
Injected as a second system message when soul content is non-empty.

### 8b. Knowledge context (from WebDAV knowledge index)
**File:** `crate-rockbot/src/memory.rs:246-250`
**Type:** Dynamic — loaded by `AgentHarness::refresh_knowledge_context()` and stored in `MemoryManager.knowledge`
Injected as a system message when relevant knowledge entries exist for the room. Fetched on each `process_message` call before building context (harness.rs:249).

### 8c. Daily summaries header
**File:** `crate-rockbot/src/memory.rs:253-267`
**Type:** Static prefix + dynamic per-summary lines
```
[Recent conversation summaries]
## {date} ({msg_count} messages)
{summary}
...
```
Injected as a system message when daily summaries exist (loaded from `memory/summaries/` on WebDAV).

---

## 9. Summarize-for-Archive Prompt (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:1021-1074`
**Role:** `user` (one-shot completion, no tools)
**Purpose:** Generates a short summary of archived messages for daily summaries. Also used inline by `truncate_and_summarize()` (line 768) for context overflow.
```
Summarize this conversation excerpt in 1-3 concise sentences. Focus on key topics,
decisions, and factual information shared:

<joined message texts, each truncated to 300 chars, max 20 messages>
```

**Fallback (line 1060-1072):** If AI summarization fails:
```
{messages.len()} messages: {preview of up to 5 message snippets truncated to 80 chars each}
```
**Minimal fallback (line 1069):** If no previewable messages:
```
{messages.len()} messages archived
```

---

## 10. WebDAV Storage Templates (not AI prompts)

### 10a. Daily summary file (upsert_daily_summary)
**File:** `crate-rockbot/src/harness.rs:922-978`
**Stored at:** `{room_dir}memory/summaries/{date}.md`
```
# Daily Summaries — {webdav_dir}

## {today_date} ({msg_count} messages, {char_count} chars)
{merged_summary}
```
Merges today's new summary into existing content using `extract_latest_summary()` and `parse_summary_header()` helpers.

### 10b. Soul memory file
**File:** `crate-rockbot/src/tools/edit_soul.rs:38-48` (description), `:74` (execute)
**Stored at:** `{room_dir}memory/soul.md`
```
# Soul Memory

- My name is YourName ✨
- (optional preference)
- (optional fact)
...
```
edit_soul performs a full replace — it overwrites the entire soul.md with the content provided by the LLM.

---

## 11. RocketChat Debug Binary Messages

**File:** `crate-rocketchat/src/main.rs`

| Line | Command | Reply |
|------|---------|-------|
| 39-50 | — | Console-only: `DM from {sender_name}` / `#{name} from {sender_name}` |
| 53-57 | `!ping` | `pong @{sender_name}` |
| 58-62 | `!echo <text>` | Echoes the text back |
| 63-68 | `!help` | `Commands: !ping, !echo <text>, !help` |

**File:** `crate-rocketchat/src/client.rs`

| Line | Purpose | Template |
|------|---------|----------|
| 58-61 | Code-block reply wrapper | `` ```\n{text}\n``` `` |
| 123 | Bot mention pattern for detection | `@{username}` (constructor of `RocketChatClient`, stored in `bot_name` field) |

---

## Summary Table

| # | What | Where | Sent To | Dynamic? |
|---|------|-------|---------|----------|
| 1 | **System prompt** — defines persona & capabilities | `harness.rs:20-64` | AI provider (`system` role) | Dynamic (`{name}`, `{max_context_mb}`, `{max_iterations}` from config) |
| 2 | **User message template** — wraps chat text | `harness.rs:172-234` | AI provider (`user` role) | Per-message |
| 3a-k | **Tool descriptions** — teach AI what tools do | 11 files in `tools/` | AI provider (tool definitions) | Static |
| 4 | **Tool param descriptions** — describe JSON fields | 11 files in `tools/` | AI provider (tool schema) | Static |
| 5 | **Default vision prompt** — fallback | `harness.rs:224` | Downstream tool code | Static |
| 6 | **Fallback messages** — error/loop handling | `harness.rs:280-552`, `main.rs:549` | RocketChat user | Partially |
| 7 | **Image gen request body** — image provider API | `types.rs:349-357`, `fal.rs:80-219` | image provider API | Dynamic |
| 8a | **Soul memory prefix** | `memory.rs:239-242` | AI provider (`system` role) | Dynamic |
| 8b | **Knowledge context** | `memory.rs:246-250` | AI provider (`system` role) | Dynamic |
| 8c | **Daily summaries** | `memory.rs:253-267` | AI provider (`system` role) | Dynamic |
| 9 | **Summarize-for-archive** — daily summary | `harness.rs:1021-1074` | AI provider (one-shot) | Dynamic |
| 10a-b | **WebDAV storage templates** — summaries, soul | `harness.rs:922-978`, `edit_soul.rs:38-48` | WebDAV storage | Dynamic |
| 11 | **Debug binary messages** | `rocketchat/main.rs:39-68`, `client.rs:58-123` | RocketChat / console | Dynamic |
