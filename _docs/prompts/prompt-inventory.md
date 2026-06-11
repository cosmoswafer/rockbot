# Prompt Inventory — rockbot

All prompts and prompt-adjacent strings in the Rust codebase, organized by what they do.

---

## 1. System Prompt (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:17-47`
**Constant:** `DEFAULT_SYSTEM_PROMPT`
**Sent to:** AI provider as the `system` role message in `ChatRequest.messages`
**Used via:** `build_system_prompt()` (line 368) → `MemoryManager::build_context()` (memory.rs:211) → prepended as first message in context

Note: `{name}` is replaced at runtime with the bot's configured username via `build_system_prompt()`.

```
You are {name}, a helpful AI assistant running on a RocketChat server. \
You respond to DMs and @mentions concisely and helpfully. \
When you need the current date or time, use the datetime tool. \
When you need information from the web, use the web_search tool. \
When you need to fetch a URL, use web_fetch. \
When you need to analyze an image, use the vision tool. \
When you need to read, write, list, or manage files on remote storage, use the webdav tool. \
When you need to manage calendar events or todo tasks, use the calendar tool. \
When you need to generate an image, use the image_gen tool. \
When a user sends an image and asks to edit, modify, transform, or use it \
as a basis for image generation, use the image_gen tool — the attachment \
images will be automatically provided as input to the tool. \
If the user asks to edit a previously generated image (no new attachment), \
you MUST include the fal.ai CDN URL from the previous result in the \
image_urls parameter yourself. \
The image_gen tool returns both a WebDAV path and an original fal.ai CDN URL — \
always share the fal.ai CDN URL with the user so they can view or share the image directly. \
When a user says !soul or asks to save or update preferences, identity, or facts, use the edit_soul tool. \
When a user asks you to remember something, shares notes, or says !remember, !note, !save or shares important \
information worth persisting, use the save_knowledge tool. \
When a user says !forget or asks to remove something you learned, \
use the forget_knowledge tool. \
When you need to recall previously saved knowledge, use the recall_knowledge tool. \
Your display name is the first non-heading line of your soul file. \
When setting your name via edit_soul, create an Identity section with \
your name on its own line (e.g. "## Identity\n零夢"). \
Use a short name under 32 characters. \
Answer in the same language as the user. \
Keep responses clear and to the point.
```

---

## 2. User Message Template (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:156`
**Template:** `ChatMessage::user(format!("{}: {}", sender_name, clean_text))`
**Role:** `user`
**Purpose:** Wraps every incoming RocketChat message as `"SenderName: message text"` before appending to history. Preserves sender identity in group chats.

When image attachments are present (harness.rs:165-177), the user message is created as `ChatMessage::user_with_images` with the attachment images encoded as data URIs, and the prompt defaults to `"Describe this image in detail."` if the text is empty.

Example output: `"Alice: what's the weather in Tokyo?"`

---

## 3. Tool Descriptions (sent to AI provider in tool definitions)

### 3a. `datetime`
**File:** `crate-rockbot/src/tools/datetime.rs:96-99`
```
Get the current UTC date and time. Returns ISO 8601 timestamp, human-readable date with weekday, and Unix epoch seconds.
```

### 3b. `web_search`
**File:** `crate-rockbot/src/tools/web_search.rs:171-174`
```
Search the web using Exa. Returns ranked results with titles, URLs, highlights, and dates.
Supports optional type (auto/fast/deep) and num_results parameters.
```

### 3c. `web_fetch`
**File:** `crate-rockbot/src/tools/web_fetch.rs:430-434`
```
Fetch content from a URL. Supports three output formats: json (structured with metadata),
markdown (HTML converted to markdown for AI consumption), raw (unmodified response text).
Optionally cross-verifies content via web search when verify=true.
```

### 3d. `vision`
**File:** `crate-rockbot/src/tools/vision.rs:118-120`
```
Download and analyze an image from a URL using AI vision. Provide an image URL (public web or WebDAV file) and a prompt describing what to look for. User attachments are already visible to you — only use this tool to fetch images from external URLs.
```

### 3e. `webdav`
**File:** `crate-rockbot/src/tools/webdav.rs:184-192`
```
Manage files on remote WebDAV storage (NextCloud). Each room has its own file space —
paths are automatically scoped. Actions: read (get file content), write (create/overwrite
a file), edit (replace oldString with newString — reads file first, fails if oldString not
found or matches multiple times, 500 KB max), list (list directory contents), mkdir
(create directory tree), delete (remove file/directory), exists (check if path exists).
```

### 3f. `calendar`
**File:** `crate-rockbot/src/tools/calendar.rs:198-212`
```
Manage calendar events on NextCloud CalDAV. Events are stored per-room — each room has its own calendar auto-created on first use. Actions: list_events (list events in a date range), get_event (fetch a single event by UID), add_event (create a new event), update_event (modify an existing event by UID), delete_event (remove an event by UID). add_event requires summary, dtstart (ISO 8601, UTC), dtend (ISO 8601, UTC). update_event uses merge semantics: specify only the fields you want to change; omitted fields keep their existing values. Optional for both: description, location, rrule (recurrence rule, RFC 5545), reminder_minutes (e.g. 15). All date/time values must be in UTC — use the Z suffix (e.g. 20260615T140000Z) or omit seconds (e.g. 20260601T000000Z). Floating times (without Z) are not supported.
```

### 3g. `image_gen`
**File:** `crate-rockbot/src/tools/image_gen.rs:148-157`
```
Generate or edit an image. For text-to-image, provide a prompt. To edit or transform an image the user sent, just describe what to do in the prompt — the user's image attachments will be automatically provided as image_urls input. The only optional parameter is image_size. Quality, output format, and number of images are pre-configured. Returns a JSON object: {"ok": true, "fal_url": "...", "webdav_path": "..."}. Always share the fal_url with the user so they can view the image directly. After a successful image_gen call, respond to the user — do not call image_gen again.
```

### 3h. `edit_soul`
**File:** `crate-rockbot/src/tools/edit_soul.rs:143-149`
```
Edit the bot's permanent memory (soul) for this room.
Actions: append (add a new section or content),
replace (replace an existing section's content),
delete_section (remove a section entirely).
Use section_header to target the ## Section name.
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
**File:** `crate-rockbot/src/tools/recall_knowledge.rs:26-30`
```
Search the knowledge index for entries matching a query. If no query is given, returns all stored knowledge entries. Matches by topic title, when_useful description, and tags.
```

---

## 4. Tool Parameter Descriptions (sent to AI provider in tool schema)

| File | Lines | Tool | Parameter | Description |
|------|-------|------|-----------|-------------|
| `datetime.rs` | 105, 108 | `datetime` | `format` | Output format: iso (ISO 8601), human (readable with weekday), unix (epoch seconds), full (all three). Default: full |
| `web_search.rs` | 182 | `web_search` | `query` | The search query to execute |
| `web_search.rs` | 187 | `web_search` | `type` | Search type: auto (balanced with autoprompt), fast (quick results), deep (comprehensive). Default: auto |
| `web_search.rs` | 193 | `web_search` | `num_results` | Number of results to return (default: 5, max: 20) |
| `web_fetch.rs` | 442 | `web_fetch` | `url` | The URL to fetch |
| `web_fetch.rs` | 447-449 | `web_fetch` | `format` | Output format: json returns structured metadata, markdown converts HTML to markdown for AI, raw returns unmodified text (default: raw) |
| `web_fetch.rs` | 453 | `web_fetch` | `verify` | Perform a web search to cross-verify content (default: false) |
| `vision.rs` | 128 | `vision` | `url` | URL of the image to analyze |
| `vision.rs` | 133 | `vision` | `prompt` | What to look for or ask about in the image |
| `webdav.rs` | 201 | `webdav` | `action` | The WebDAV operation to perform |
| `webdav.rs` | 205 | `webdav` | `room_id` | Room ID for scoping the operation (injected automatically if omitted) |
| `webdav.rs` | 209 | `webdav` | `path` | File or directory path relative to the room root |
| `webdav.rs` | 213 | `webdav` | `content` | File content to write (required for 'write' action) |
| `webdav.rs` | 217 | `webdav` | `oldString` | Exact text to find and replace (required for 'edit' action, must be unique in the file) |
| `webdav.rs` | 221 | `webdav` | `newString` | Replacement text (required for 'edit' action) |
| `calendar.rs` | 222 | `calendar` | `action` | Calendar operation to perform |
| `calendar.rs` | 226 | `calendar` | `start` | Start of date range in ISO 8601 UTC (e.g. 20260601T000000Z). Used by list_events. |
| `calendar.rs` | 230 | `calendar` | `end` | End of date range in ISO 8601 UTC. Used by list_events. |
| `calendar.rs` | 234 | `calendar` | `uid` | Event UID. Required for update_event and delete_event. |
| `calendar.rs` | 238 | `calendar` | `summary` | Event title/summary. Required for add_event and update_event. |
| `calendar.rs` | 242 | `calendar` | `dtstart` | Event start in ISO 8601 UTC (e.g. 20260615T140000Z). Required for add_event. |
| `calendar.rs` | 246 | `calendar` | `dtend` | Event end in ISO 8601 UTC. Required for add_event. |
| `calendar.rs` | 250 | `calendar` | `description` | Optional event description/details. |
| `calendar.rs` | 254 | `calendar` | `location` | Optional event location. |
| `calendar.rs` | 258 | `calendar` | `rrule` | Optional recurrence rule in RFC 5545 format (e.g. FREQ=WEEKLY;BYDAY=MO). |
| `calendar.rs` | 262 | `calendar` | `reminder_minutes` | Optional reminder in minutes before event (e.g. 15). |
| `image_gen.rs` | 165 | `image_gen` | `prompt` | Text description of the image to generate |
| `image_gen.rs` | 169 | `image_gen` | `room_id` | Room ID for image storage (injected automatically if omitted) |
| `image_gen.rs` | 173 | `image_gen` | `image_size` | Aspect ratio preset or custom {\"width\": N, \"height\": N} JSON. Presets: square_hd (1:1 2880x2880), square (512x512), landscape_16_9 (3840x2160 4K), portrait_16_9 (2160x3840), landscape_4_3 (3312x2480), portrait_4_3 (2480x3312), landscape_3_2 (3504x2336), portrait_2_3 (2336x3504), auto. Default: landscape_4_3. Max edge 3840px, multiples of 16. |
| `image_gen.rs` | 175-179 | `image_gen` | `image_urls` | Image URLs for editing/transformations. When the user sends images, they are automatically injected. Do NOT try to reference data URIs from vision context — they will be provided automatically. |
| `edit_soul.rs` | 155-157 | `edit_soul` | `action` | Soul memory operation: append (add new section/content), replace (update existing section), delete_section (remove a section) |
| `edit_soul.rs` | 162 | `edit_soul` | `section_header` | Target ## Section name (e.g. Preferences, Identity, Facts) |
| `edit_soul.rs` | 166 | `edit_soul` | `content` | New content for the section (required for append and replace) |
| `edit_soul.rs` | 170 | `edit_soul` | `webdav_dir` | Room WebDAV directory key (injected automatically) |
| `save_knowledge.rs` | 52-54 | `save_knowledge` | `category` | Knowledge category: skill (procedural/how-to), secret (credential/sensitive), note (factual info) |
| `save_knowledge.rs` | 58 | `save_knowledge` | `topic` | Short title or topic for the entry (e.g. 'DB API', 'Build Commands') |
| `save_knowledge.rs` | 62 | `save_knowledge` | `content` | Markdown body of the knowledge entry |
| `save_knowledge.rs` | 66-67 | `save_knowledge` | `when_useful` | Describe the situation that makes this knowledge relevant, used for automatic retrieval (e.g. 'when calling the database API') |
| `save_knowledge.rs` | 71 | `save_knowledge` | `tags` | Comma-separated keywords for search (e.g. 'api, database, python') |
| `forget_knowledge.rs` | 38 | `forget_knowledge` | `topic` | Title or topic of the knowledge entry to delete |
| `recall_knowledge.rs` | 38-39 | `recall_knowledge` | `query` | Topic or keyword to search for in knowledge entries. Leave empty to retrieve all entries. |

---

## 5. Default Vision Prompt (fallback for tool execution)

**File:** `crate-rockbot/src/tools/vision.rs:152`
```
Describe this image in detail.
```
**Used when:** AI calls `vision` tool without providing a `prompt` argument. Passed to `analyze_image()` as the prompt parameter.

Also used as the default prompt in `harness.rs:167` when a user sends image attachments with no text.

---

## 6. Fallback / Error Messages (returned to RocketChat user)

**File:** `crate-rockbot/src/harness.rs`

| Line | Condition | Text |
|------|-----------|------|
| 207 | Max agent iterations exceeded | `I'm sorry, I got stuck in a loop. Could you rephrase your request?` |
| 332 | AI returned empty text (stored in history) | `(no response)` |
| 339 | AI returned empty text (user-facing) | `I processed your request but received an empty response.` |
| 346 | AI returned no text at all | `I received a response but it was empty. Please try again.` |
| 352-353 | AI provider error (dynamic) | `I encountered an error: {e}. Please try again.` |

**File:** `crate-rockbot/src/main.rs`

| Line | Condition | Text |
|------|-----------|------|
| 429 | Outer catch-all for message processing | `Error processing message: {e}` |

---

## 7. Image Generation Request Body (sent to fal.ai API)

**File:** `crate-rockbot/src/provider/fal.rs:129-181` (`submit_request`)

The user-provided `prompt` string plus optional parameters are sent as a JSON body:
```json
{
  "prompt": "<user provided>",
  "quality": "<optional>",
  "output_format": "<optional>",
  "num_images": <optional>,
  "image_size": "<optional preset or object>",
  "image_urls": ["<optional URL(s) for img2img>"]
}
```
Sent as a POST to `{base_url}/{model_id}` with `Authorization: Key {api_key}` header. Returns a `request_id` that is polled until completion via `poll_status()`.

Result message returned to AI (image_gen.rs:262-267):
```json
{"ok": true, "fal_url": "<CDN URL>", "webdav_path": "<WebDAV path>"}
```

---

## 8. Memory / Context Prompts (sent to AI provider as system messages)

### 8a. Soul memory prefix
**File:** `crate-rockbot/src/memory.rs:233-236`
**Type:** Dynamic — loaded from WebDAV `memory/soul.md`
```
[Core memory — permanent preferences, identity, and facts]
{content from soul.md}
```
Injected as a second system message when soul content is non-empty.

### 8b. Daily summaries header
**File:** `crate-rockbot/src/memory.rs:246-259`
**Type:** Static prefix + dynamic per-summary lines
```
[Recent conversation summaries]
## {date} ({msg_count} messages)
{summary}
...
```
Injected as a system message when daily summaries exist (loaded from `memory/summaries/` on WebDAV).

### 8c. Knowledge context (from WebDAV knowledge index)
**File:** `crate-rockbot/src/memory.rs:240-244`
**Type:** Dynamic — loaded by `AgentHarness::refresh_knowledge_context()` (harness.rs:878) and stored in `MemoryManager.knowledge`
```
[Knowledge — automatically recalled for this conversation]
{formatted entries from knowledge index}
```
Injected as a system message when relevant knowledge entries exist for the room. Fetched on each `process_message` call before building context (harness.rs:192).

---

## 9. Summarize-for-Archive Prompt (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:616-620`
**Role:** `user` (one-shot completion, no tools)
**Purpose:** Generates a short summary of archived messages for daily summaries.
```
Summarize this conversation excerpt in 1-3 concise sentences. Focus on key topics,
decisions, and factual information shared:

<joined message texts, each truncated to 300 chars, max 20 messages>
```

**Fallback (line 638-652):** If AI summarization fails:
```
{messages.len()} messages: {preview of up to 5 message snippets truncated to 80 chars each}
```
**Minimal fallback (line 646-648):** If no previewable messages:
```
{messages.len()} messages archived
```

---

## 10. WebDAV Storage Templates (not AI prompts)

### 10a. Daily summary file (upsert_daily_summary)
**File:** `crate-rockbot/src/harness.rs:522-559`
**Stored at:** `{room_dir}memory/summaries/{date}.md`
```
# Daily Summaries — {webdav_dir}

## {today_date} ({msg_count} messages, {char_count} chars)
{ai_generated_summary}

## {date2} ({msg_count2} messages)
{summary2}
...
```

### 10b. Soul memory file
**File:** `crate-rockbot/src/tools/edit_soul.rs:133-135`
**Stored at:** `{room_dir}memory/soul.md`
```
# Soul Memory

## {section_header}
{content}
```
Append appends another `## Section\ncontent` block to the existing file.

---

## 11. RocketChat Debug Binary Messages

**File:** `crate-rocketchat/src/main.rs`

| Line | Command | Reply |
|------|---------|-------|
| 39-50 | — | Console-only: `DM from {sender_name}` / `#{room_name} from {sender_name}` |
| 53 | `!ping` | `pong @{sender_name}` |
| 58-62 | `!echo <text>` | Echoes the text back |
| 63 | `!help` | `Commands: !ping, !echo <text>, !help` |

**File:** `crate-rocketchat/src/client.rs`

| Line | Purpose | Template |
|------|---------|----------|
| 46-49 | Code-block reply wrapper | `` ```\n{text}\n``` `` |
| 104 | Bot mention pattern for detection | `@{username}` (constructor of `RocketChatClient`, stored in `bot_name` field) |

---

## Summary Table

| # | What | Where | Sent To | Dynamic? |
|---|------|-------|---------|----------|
| 1 | **System prompt** — defines persona & capabilities | `harness.rs:17-47` | AI provider (`system` role) | Static (with `{name}` template) |
| 2 | **User message template** — wraps chat text | `harness.rs:156` | AI provider (`user` role) | Per-message |
| 3a-k | **Tool descriptions** — teach AI what tools do | 11 files in `tools/` | AI provider (tool definitions) | Static |
| 4 | **Tool param descriptions** — describe JSON fields | 11 files in `tools/` | AI provider (tool schema) | Static |
| 5 | **Default vision prompt** — fallback | `vision.rs:152`, `harness.rs:167` | Downstream tool code | Static |
| 6 | **Fallback messages** — error/loop handling | `harness.rs:207-353`, `main.rs:429` | RocketChat user | Partially |
| 7 | **Image gen request body** — fal.ai API | `fal.rs:129-181` | fal.ai API | Dynamic |
| 8a-b | **Memory/context prompts** — soul, summaries | `memory.rs:233-259` | AI provider (`system` role) | Dynamic |
| 8c | **Knowledge context** — matched entries | `memory.rs:240-244` | AI provider (`system` role) | Dynamic |
| 9 | **Summarize-for-archive** — daily summary | `harness.rs:616-620` | AI provider (one-shot) | Dynamic |
| 10a-b | **WebDAV storage templates** — summaries, soul | `harness.rs:522-559`, `edit_soul.rs:133-135` | WebDAV storage | Dynamic |
| 11 | **Debug binary messages** | `rocketchat/main.rs:39-64`, `client.rs:46-49` | RocketChat / console | Dynamic |

(End of file - total lines may vary)
