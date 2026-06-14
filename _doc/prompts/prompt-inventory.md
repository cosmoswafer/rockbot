# Prompt Inventory — rockbot

All prompts and prompt-adjacent strings in the Rust codebase, organized by what they do.

---

## 1. System Prompt (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:20-42`
**Constant:** `DEFAULT_SYSTEM_PROMPT`
**Sent to:** AI provider as the `system` role message in `ChatRequest.messages`
**Used via:** `build_system_prompt()` (line 619) → `MemoryManager::build_context()` → prepended as first message in context

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
When you need to generate or edit images, use the image_gen tool. \
Share image_gen results as markdown `![{description}]({image_key})`. \
Do NOT fabricate fake image references — only image_gen produces real images. \
When you need to read, write, list, or manage files on remote storage, use the webdav tool. \
When you need to manage calendar events or todo tasks, use the calendar tool. \
When you need to save or update your personality, preferences, or identity, use the edit_soul tool. \
When you need to remember something important, use the save_knowledge tool. \
When you need to remove something you learned, use the forget_knowledge tool. \
When you need to recall previously saved knowledge, use the recall_knowledge tool. \
Answer in the same language as the user. \
Keep responses clear and to the point.\
```

---

## 2. User Message Template (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:185-209`
**Template:** `ChatMessage::user(format!("{}: {}", sender_name, clean_text))`
**Role:** `user`
**Purpose:** Wraps every incoming RocketChat message as `"SenderName: message text"` before appending to history. Preserves sender identity in group chats.

When image attachments are present (harness.rs:191-209), they are downloaded via
`download_attachment_refs()` (line 189) and encoded as data URIs, then injected into the
conversation as markdown `![image_name](image_name)` labels. The user message is
created as `ChatMessage::user_with_images` (line 206) with the prompt including an
`Attached:` line listing the image labels. If the text is empty, the prompt
defaults to `"SenderName: Describe this image in detail.\nAttached: ![name](name)"`.

The harness later resolves `image_name` references in `image_gen` tool calls
by matching them against these cached data URIs.

Example output: `"Alice: what's the weather in Tokyo?"`

---

## 3. Tool Descriptions (sent to AI provider in tool definitions)

### 3a. `datetime`
**File:** `crate-rockbot/src/tools/datetime.rs:228-232`
```
Get the current UTC date and time. Returns ISO 8601 timestamp, human-readable date with weekday, Unix epoch seconds, calendar month view, week number (ISO 8601), or weekday list. Supports week_offset for prev/next week views.
```

### 3b. `web_search`
**File:** `crate-rockbot/src/tools/web_search.rs:230-233`
```
Search the web using Exa. Returns ranked results with titles, URLs, highlights, and dates.
Supports optional type (auto/fast/deep), num_results, and contents_mode (highlights/text/deep) parameters.
```

### 3c. `web_fetch`
**File:** `crate-rockbot/src/tools/web_fetch.rs:681-687`
```
Fetch or send content from/to a URL like curl. Supports GET, POST, PUT, PATCH, DELETE,
HEAD, OPTIONS with custom headers, JSON body, raw body, and file upload from WebDAV.
Response can be saved to WebDAV. Three output formats: json (structured with metadata),
markdown (HTML converted to markdown for AI consumption), raw (unmodified response text).
Optionally cross-verifies content via web search when verify=true.
```

### 3d. `vision`
**File:** `crate-rockbot/src/tools/vision.rs:125-130`
```
Fetch an image from a WebDAV file path or public URL and return it as a base64 markdown image tag.
Use this to retrieve images the user is asking about from WebDAV storage or external URLs.
Optionally provide a prompt hint for how the image should be analyzed.
User attachments are already visible to you — only use this tool for images at explicit URLs.
```

### 3e. `webdav`
**File:** `crate-rockbot/src/tools/webdav.rs:229-237`
```
Manage files on remote WebDAV storage (NextCloud). Each room has its own file space —
paths are automatically scoped. Actions: read (get file content), write (create/overwrite
a file), edit (replace oldString with newString — reads file first, fails if oldString not
found or matches multiple times, 500 KB max), list (list directory contents), mkdir
(create directory tree), delete (remove file/directory), exists (check if path exists).
```

### 3f. `calendar`
**File:** `crate-rockbot/src/tools/calendar.rs:227-242`
```
Manage calendar events on NextCloud CalDAV. Events are stored per-room — each room has its own calendar auto-created on first use. Actions: list_events (list events in a date range), get_event (fetch a single event by UID), add_event (create a new event), update_event (modify an existing event by UID), delete_event (remove an event by UID). add_event requires summary, dtstart (ISO 8601, UTC), dtend (ISO 8601, UTC). update_event uses merge semantics: specify only the fields you want to change; omitted fields keep their existing values. Optional for both: description, location, rrule (recurrence rule, RFC 5545), reminder_minutes (e.g. 15). All date/time values must be in UTC — use the Z suffix (e.g. 20260615T140000Z) or omit seconds (e.g. 20260601T000000Z). Floating times (without Z) are not supported.
```

### 3g. `image_gen`
**File:** `crate-rockbot/src/tools/image_gen.rs:137-141`
```
Generate or edit an image. Provide a prompt and optional aspect_ratio (e.g. '16:9').
User attachments are auto-provided as image_urls for editing.
Returns {"ok": true, "image_key": "..."} — share result as `![desc]({image_key})`.
```

Note: `image_size` is NOT exposed to the LLM as a tool parameter — it is derived from the `aspect_ratio` parameter at runtime (mapped to a preset like `"16:9"` → `{3840, 2160}`). `size_tier` is set from `[image_model]` config `default_image_size_tier`. The harness injects `room_id`, `webdav_dir`, and `image_cache_key` automatically. `image_urls` are auto-injected from message attachments.

### 3h. `edit_soul`
**File:** `crate-rockbot/src/tools/edit_soul.rs:48-59`
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
**File:** `crate-rockbot/src/tools/save_knowledge.rs:31-36`
```
Save a piece of knowledge for future reference. Use this when the user says 'remember', 'learn', or shares important information worth persisting. Each entry gets a .md file and is indexed for contextual retrieval.
```

### 3j. `forget_knowledge`
**File:** `crate-rockbot/src/tools/forget_knowledge.rs:25-29`
```
Remove a previously saved knowledge entry. Provide the topic title of the entry to delete.
The .md file is deleted and the entry is removed from the knowledge index.
```

### 3k. `recall_knowledge`
**File:** `crate-rockbot/src/tools/recall_knowledge.rs:25-29`
```
Search the knowledge index for entries matching a query. If no query is given,
returns all stored knowledge entries. Matches by topic title, when_useful description, and tags.
```

### 3l. `compress_memory`
**File:** `crate-rockbot/src/tools/compress_memory.rs:37-42`
```
Compress all current conversation messages into a memory summary. The LLM will distill all messages into at most 10 bullet points saved as summary.md. After compression, the chat history is cleared to zero — only the summary remains. Use when the user says !compress, !memory, or explicitly asks to save memory.
```

---

## 4. Tool Parameter Descriptions (sent to AI provider in tool schema)

| File | Lines | Tool | Parameter | Description |
|------|-------|------|-----------|-------------|
| `datetime.rs` | 241, 243 | `datetime` | `format` | Output format: iso (ISO 8601), human (readable with weekday), unix (epoch seconds), calendar (month grid), weekdays (list of weekdays with dates), week_number (ISO week number), full (all). Default: full |
| `datetime.rs` | 245-247 | `datetime` | `week_offset` | Offset for calendar/weekdays format: 0=current week/month, 1=next, -1=previous. Default: 0 |
| `web_search.rs` | 241 | `web_search` | `query` | The search query to execute |
| `web_search.rs` | 246 | `web_search` | `type` | Search type: auto (balanced with autoprompt), fast (quick results), deep (comprehensive). Default: auto |
| `web_search.rs` | 251 | `web_search` | `contents_mode` | Content mode: highlights returns snippets (default), text returns full page content, deep enables comprehensive search |
| `web_search.rs` | 257 | `web_search` | `num_results` | Number of results to return (default: 5, max: 20) |
| `web_fetch.rs` | 694 | `web_fetch` | `url` | The URL to fetch (required) |
| `web_fetch.rs` | 699 | `web_fetch` | `method` | HTTP method (default: GET) |
| `web_fetch.rs` | 703 | `web_fetch` | `headers` | HTTP headers as key-value pairs, e.g. {"Authorization": "token xyz", "Content-Type": "application/json"} |
| `web_fetch.rs` | 707 | `web_fetch` | `body` | Raw string body for POST/PUT/PATCH requests |
| `web_fetch.rs` | 711 | `web_fetch` | `body_json` | JSON body — serialized as request body with Content-Type: application/json |
| `web_fetch.rs` | 715 | `web_fetch` | `file_from_webdav` | WebDAV file path to read and use as request body |
| `web_fetch.rs` | 719 | `web_fetch` | `save_to_webdav` | WebDAV file path to save the response body |
| `web_fetch.rs` | 724 | `web_fetch` | `format` | Output format: json returns structured metadata, markdown converts HTML to markdown for AI, raw returns unmodified text (default: raw) |
| `web_fetch.rs` | 728 | `web_fetch` | `verify` | Perform a web search to cross-verify content (default: false) |
| `vision.rs` | 138 | `vision` | `url` | URL of the image to fetch (public web or WebDAV file) |
| `vision.rs` | 142 | `vision` | `prompt` | Optional prompt for the LLM to use when analyzing this image |
| `webdav.rs` | 245-247 | `webdav` | `action` | The WebDAV operation to perform |
| `webdav.rs` | 250 | `webdav` | `room_id` | Room ID for scoping the operation (injected automatically if omitted) |
| `webdav.rs` | 254 | `webdav` | `path` | File or directory path relative to the room root |
| `webdav.rs` | 258 | `webdav` | `content` | File content to write (required for 'write' action) |
| `webdav.rs` | 263 | `webdav` | `oldString` | Exact text to find and replace (required for 'edit' action, must be unique in the file) |
| `webdav.rs` | 267 | `webdav` | `newString` | Replacement text (required for 'edit' action) |
| `calendar.rs` | 251 | `calendar` | `action` | Calendar operation to perform |
| `calendar.rs` | 255 | `calendar` | `start` | Start of date range in ISO 8601 UTC (e.g. 20260601T000000Z). Used by list_events. |
| `calendar.rs` | 259 | `calendar` | `end` | End of date range in ISO 8601 UTC. Used by list_events. |
| `calendar.rs` | 263 | `calendar` | `uid` | Event UID. Required for update_event and delete_event. |
| `calendar.rs` | 267 | `calendar` | `summary` | Event title/summary. Required for add_event and update_event. |
| `calendar.rs` | 271 | `calendar` | `dtstart` | Event start in ISO 8601 UTC (e.g. 20260615T140000Z). Required for add_event. |
| `calendar.rs` | 275 | `calendar` | `dtend` | Event end in ISO 8601 UTC. Required for add_event. |
| `calendar.rs` | 279 | `calendar` | `description` | Optional event description/details. |
| `calendar.rs` | 283 | `calendar` | `location` | Optional event location. |
| `calendar.rs` | 287 | `calendar` | `rrule` | Optional recurrence rule in RFC 5545 format (e.g. FREQ=WEEKLY;BYDAY=MO). |
| `calendar.rs` | 291 | `calendar` | `reminder_minutes` | Optional reminder in minutes before event (e.g. 15). |
| `image_gen.rs` | 149 | `image_gen` | `prompt` | Text description of the image to generate |
| `image_gen.rs` | 153 | `image_gen` | `aspect_ratio` | Aspect ratio for the generated image as W:H (e.g. '16:9', '2:3', '1:1') |
| `image_gen.rs` | 157 | `image_gen` | `room_id` | Room ID for image storage (injected automatically if omitted) |
| `image_gen.rs` | 162 | `image_gen` | `image_urls` | Reference image URLs for editing (auto-injected from user attachments) |
| `edit_soul.rs` | 67 | `edit_soul` | `content` | Full soul.md content following the template: # Soul Memory\\n\\n- My name is Name ✨\\n- ...\\n- ... |
| `edit_soul.rs` | 71 | `edit_soul` | `webdav_dir` | Room WebDAV directory key (injected automatically) |
| `save_knowledge.rs` | 44 | `save_knowledge` | `topic` | Short title or topic for the entry (e.g. 'DB API', 'Build Commands') |
| `save_knowledge.rs` | 48 | `save_knowledge` | `content` | Markdown body of the knowledge entry |
| `save_knowledge.rs` | 52-53 | `save_knowledge` | `when_useful` | Describe the situation that makes this knowledge relevant, used for automatic retrieval (e.g. 'when calling the database API') |
| `save_knowledge.rs` | 57 | `save_knowledge` | `tags` | Comma-separated keywords for search (e.g. 'api, database, python') |
| `save_knowledge.rs` | 62 | `save_knowledge` | `priority` | Knowledge priority: P0 (highest, always recalled), P1 (high, default), P2 (medium), P3 (low). Higher priority means more frequently recalled. |
| `forget_knowledge.rs` | 37 | `forget_knowledge` | `topic` | Title or topic of the knowledge entry to delete |
| `recall_knowledge.rs` | 37-38 | `recall_knowledge` | `query` | Topic or keyword to search for in knowledge entries. Leave empty to retrieve all entries. |
| `compress_memory.rs` | 49 | `compress_memory` | `webdav_dir` | Room WebDAV directory key (injected automatically) |
| `compress_memory.rs` | 54 | `compress_memory` | `room_id` | Room UUID (injected automatically) |

---

## 5. Default Vision Prompt (fallback for tool execution)

**File:** `crate-rockbot/src/harness.rs:199`
```
{}: Describe this image in detail.
```
**Used when:** A user sends image attachments with no text. The harness constructs a user message as `"{sender_name}: Describe this image in detail.\nAttached: ..."` with the attached image labels. Not used by the `vision` tool itself (vision tool has its own optional `prompt` parameter set by the LLM).

---

## 6. Vision Image Interception (harness internal)

**File:** `crate-rockbot/src/harness.rs:721-779`

Two methods for bridging vision tool results across turns:

### 6a. `cache_vision_images` (line 721)
Parses markdown image tags (`![name](data:mime/type;base64,...)`) from the vision tool result and caches them in `image_pool`. Called in `process_tool_response` after a `vision` tool call completes.

### 6b. `inject_vision_images` (line 753)
On the next context build, drains the `image_pool` and injects the cached data URIs into the conversation as a synthesized user message: `"The requested image(s) is/are visible below:\nAttached: ![photo1.png](photo1.png) ..."`. Called from `process_message` before each AI request.

This enables the LLM to use vision results for image editing — the vision tool fetches an image, the harness caches it, and on the next turn the image is available for `image_gen` editing via the `image_urls` auto-injection mechanism.

---

## 7. Fallback / Error Messages (returned to RocketChat user or as tool results)

### 7a. Agent loop fallbacks (harness.rs)

| Line | Condition | Text |
|------|-----------|------|
| 259 | Max agent iterations exceeded | `I'm sorry, I got stuck in a loop. Could you rephrase your request?` |
| 447 | compress_memory intercepted (delayed) | `Memory compression scheduled. Reply to the user first — compression will execute after your reply is sent.` |
| 522 | AI returned empty text (stored in history) | `(no response)` |
| 529 | AI returned empty text (user-facing) | `I processed your request but received an empty response.` |
| 546 | AI returned no text at all | `I received a response but it was empty. Please try again.` |
| 556-598 | Context length exceeded | Compresses history + hard-truncation retry (one attempt); if still exceeded, falls through to generic error below |
| 599-600 | AI provider error (dynamic) | `I encountered an error: {e}. Please try again.` |
| 872 | Compression completed | `Memory compressed. Summary:\n\n{summary}` |

### 7b. Tool result errors (tool.rs)

| Line | Condition | Text |
|------|-----------|------|
| 118 | Tool `execute()` returns Err | `Tool execution error: {e}` |
| 125 | LLM requests unknown tool | `Unknown tool: {tool_name}` |

### 7c. Tool-specific errors

| File | Line | Condition | Text |
|------|------|-----------|------|
| `web_search.rs` | 77-78 | No Exa API key configured | `web_search requires an Exa API key. Configure it in [tools.exa] section of config.toml.` |
| `web_search.rs` | 145 | Exa returns 401 | `Exa search failed: invalid API key (401). Check your EXA_API_KEY env var or [tools.exa] config.` |
| `web_search.rs` | 165 | Exa response missing results array | `Exa returned no results array` |
| `web_search.rs` | 168 | Exa returns empty results | `No search results found.` |
| `recall_knowledge.rs` | 56 (via knowledge.rs:362) | No knowledge entries in room | `No knowledge entries found for this room.` |
| `knowledge.rs` | 370 | Recall query matches nothing | `No knowledge entry found matching '{query}'.` |

### 7d. Main loop error (main.rs)

| Line | Condition | Text |
|------|-----------|------|
| 554-555 | process_message returns Err | `Error processing message: {e}` |

---

## 8. Image Generation Request Body (sent to image provider API)

**Files:** `crate-rockbot/src/types.rs:349-358` (ImageGenParams struct),
`crate-rockbot/src/provider/fal.rs:82` (fal.ai submit_request),
`crate-rockbot/src/provider/fal.rs:292` (fal.ai generate_image_url),
`crate-rockbot/src/provider/openrouter.rs:411` (OpenRouter generate_image)

The provider-agnostic `ImageGenParams` struct:
```json
{
  "prompt": "<user provided>",
  "quality": "<from config>",
  "image_size": "<ImageSizeValue: Preset string or Custom {width, height}>",
  "size_tier": "<4K|2K|1K from config, OpenRouter only>",
  "output_format": "<from config>",
  "num_images": "<from config>",
  "model_id": "<from config>",
  "image_urls": ["<optional URL(s) for img2img>"]
}
```

**fal.ai** (POST `{base_url}/{model_id}`): resolved pixel dimensions in submit body.
**OpenRouter** (POST `/chat/completions`): maps to `image_config.aspect_ratio` + `image_config.image_size: "4K"`.

Tool result returned to LLM (image_gen.rs:280-288):
```json
{"ok": true, "webdav_path": "<WebDAV path>", "image_key": "<call_id>", "share_url": "<NextCloud share link>/download"}
```

The LLM includes `image_key` in its reply markdown. The agent loop (main.rs:452-473) replaces it with a NextCloud share URL (`share_url`, 7-day expiry) before sending to RocketChat.

---

## 9. Memory / Context Prompts (sent to AI provider as system messages)

### 9a. Soul memory prefix
**File:** `crate-rockbot/src/memory.rs:276-279`
**Type:** Dynamic — loaded from WebDAV `memory/soul.md`
```
[Core memory — permanent preferences, identity, and facts]
{content from soul.md}
```
Injected as a second system message when soul content is non-empty. Truncated at `max_soul_chars` with a `[truncated]` marker appended.

### 9b. Knowledge context (from WebDAV knowledge index)
**File:** `crate-rockbot/src/memory.rs:283-288`
**Type:** Dynamic — loaded by `AgentHarness::refresh_knowledge_context()` (harness.rs:1252)
and stored in `MemoryManager.knowledge`. Individual entries formatted as:
```
[Knowledge: {title}]
{body}
```
Entries joined with `\n---\n` separator and wrapped as:
```
[Knowledge — automatically recalled for this conversation]
{joined entries}
```
Injected as a system message when relevant knowledge entries exist for the room. Fetched on each `process_message` call before building context (harness.rs:224).

### 9c. Conversation summary
**File:** `crate-rockbot/src/memory.rs:290-296`
**Type:** Dynamic — loaded from WebDAV `memory/summary.md`
```
[Recent conversation summary]
{content from summary.md}
```
Injected as a system message when a summary exists (stored and loaded from `memory/summary.md` on WebDAV).

---

## 10. Compress-for-Summary Prompt (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:987-1017` (`compress_for_summary`)
**Role:** `user` (one-shot completion, no tools)
**Purpose:** Generates a bullet-point memory summary from archived conversation messages. Called by `compress_room_inner()` to create/update `summary.md`. Also used in tests.

```
Compress this conversation excerpt into at most 10 bullet points for a memory summary.
Focus on key facts, decisions, user preferences, and persistent information.
Output format:
# Memory Summary

- bullet point 1
- bullet point 2
...

## Used Knowledge
- filename1.md
- filename2.md

Only list knowledge entries that were actually relevant to this conversation.
{existing_block}
## Conversation
{joined message texts, each truncated to 300 chars, max 20 messages}
```

**Sub-templates:**
- Existing summary header (line 984): `\n## Existing Summary\n{summary}`
- Knowledge entries reference (lines 972-979): section heading `\n## Knowledge Entries (identify which were relevant)\n` + `- \`{filename}\` — {when_useful}\n` per entry (max 30)

**Fallback (line 1017):** If AI summarization fails or no user messages:
```
{messages.len()} messages compressed
```

---

## 11. WebDAV Storage Templates (not AI prompts)

### 11a. Summary file (write_summary_md)
**File:** `crate-rockbot/src/harness.rs:1031-1047`
**Stored at:** `{room_dir}memory/summary.md`
Content is the LLM-generated summary text (bullet-point format from section 10). Written as raw bytes via WebDAV.

### 11b. Soul memory file
**File:** `crate-rockbot/src/tools/edit_soul.rs:48-59` (description), `:39` (path)
**Stored at:** `{room_dir}memory/soul.md`
```
# Soul Memory

- My name is YourName ✨
- (optional preference)
- (optional fact)
...
```
edit_soul performs a full replace — it overwrites the entire soul.md with the content provided by the LLM.

### 11c. Knowledge entry .md file
**File:** `crate-rockbot/src/knowledge.rs:228-231`
**Stored at:** `{room_dir}knowledge/{filename}.md`
```
# {topic}

**When Useful:** {when_useful}
**Tags:** {tags}
**Created:** {timestamp}
**Updated:** {timestamp}

{content}
```

### 11d. Snapshot file (persistence)
**File:** `crate-rockbot/src/memory.rs:458` (schema version string)
**Stored at:** `{room_dir}memory/snapshot.json`
JSON snapshot of room state (messages, soul, summary, archive_seq). Schema version `rockbot-snapshot/1`. Cached read on restore, rebuilt on dirty flag.

### 11e. WebDAV tool result templates (webdav.rs)
| Line | Template | Example |
|------|----------|---------|
| 69-71 | Image read | `![{name}](data:{mime};base64,{data})` |
| 87 | Write success | `Written {bytes} bytes to {path}` |
| 137-141 | Edit success | `Edited {path}: replaced 1 occurrence ({bytes} bytes written)` |
| 158-166 | Directory listing | `Contents of '{dir}':\n\n  {DIR/FILE}  {size}  {date}  {name}` |
| 158 | Empty directory | `Directory '{dir}' is empty.` |
| 181 | Mkdir success | `Directory created: {path}` |
| 191 | Delete success | `Deleted: {path}` |
| 202-206 | Exists check | `Path '{path}': exists` / `not found` |

### 11f. Secrets sanitization (webdav.rs:52-59)
When the LLM attempts to read `secrets.toml` via the webdav tool, dummy placeholder data is returned:
```toml
[[secrets]]
host = "localhost"
key = "placeholder"
value = "abcd"
```

### 11g. Knowledge recall templates
| File | Line | Template |
|------|------|----------|
| `knowledge.rs` | 228-231 | `.md` body format (see 11c) |
| `knowledge.rs` | 355-357 | `[Knowledge: {title}]\n{body}` (recalled entry) |
| `knowledge.rs` | 362 | `No knowledge entries found for this room.` |
| `knowledge.rs` | 370 | `No knowledge entry found matching '{query}'.` |
| `save_knowledge.rs` | 89-92 | `Knowledge saved: [{topic}] {topic}` |
| `forget_knowledge.rs` | 54 | `Knowledge entry '{topic}' deleted.` |
| `knowledge.rs` | 258-259 | `Knowledge entry '{topic}' not found.` |

---

## 12. Calendar / CalDAV Result Templates (webdav.rs — not AI prompts)

| File | Lines | Template |
|------|-------|----------|
| `calendar.rs` | 107-133 | `Event: {summary}\n  UID: {uid}\n  When: {start} to {end}\n` (+ optional description, location, recurrence, reminder lines) |
| `calendar.rs` | 136-155 | `Todo: {summary}\n  UID: {uid}\n  Status: {status}\n` (+ optional description, due date, priority) |
| `calendar.rs` | 321 | `No events found between {start} and {end}.` |
| `calendar.rs` | 323-326 | `{count} event(s) between {start} and {end}:\n\n` |
| `calendar.rs` | 360 | `Event created with UID: {uid}` |
| `calendar.rs` | 378 | `Event updated: {uid}` |
| `calendar.rs` | 388 | `Event deleted: {uid}` |
| `calendar.rs` | 399 | `No todos found.` |
| `calendar.rs` | 401 | `{count} todo(s):\n\n` |

---

## 13. RocketChat Debug Binary Messages

**File:** `crate-rocketchat/src/main.rs`

| Line | Command | Reply |
|------|---------|-------|
| 39-48 | — | Console-only: `DM from {sender_name}` / `#{name} from {sender_name}` |
| 53-57 | `!ping` | `pong @{sender_name}` |
| 58-62 | `!echo <text>` | Echoes the text back |
| 63-68 | `!help` | `Commands: !ping, !echo <text>, !help` |

**File:** `crate-rocketchat/src/client.rs`

| Line | Purpose | Template |
|------|---------|----------|
| 58-61 | Code-block reply wrapper | `` ```{text}``` `` |
| 123 | Bot mention pattern for detection | `@{username}` (constructor of `RocketChatClient`, stored in `bot_name` field) |

---

## 14. Miscellaneous Prompt-Adjacent Strings

| File | Line | Purpose | Text |
|------|------|---------|------|
| `harness.rs` | 769 | Vision image injection | `The requested image{s are/is} visible below:\nAttached: {labels}` |
| `harness.rs` | 872 | Compression result | `Memory compressed. Summary:\n\n{summary}` |
| `memory.rs` | 271 | Soul truncation marker | `[truncated]` (appended to soul memory when over char limit) |
| `memory.rs` | 550-552 | Context truncation marker | `[...truncated]` (appended to truncated message texts) |
| `memory.rs` | 581 | Image stripping placeholder | `[image]` (replaces image parts in non-latest messages) |
| `provider/deepseek.rs` | 71 | DeepSeek image placeholder | `[image]` (replaces image data URIs, DeepSeek lacks vision) |
| `tools/web_fetch.rs` | 526 | Web fetch truncation | `...(truncated)` (appended when content exceeds 10000 chars) |
| `tools/web_fetch.rs` | 373 | Saved-to note | `\n\nSaved to WebDAV: {path}` |
| `tools/web_fetch.rs` | 365,377 | Related sources header | `\n\n## Related Sources\n\n` |
| `tools/web_fetch.rs` | 460-473 | Related source entry | `{idx}. **{title}**\n   URL: {url}\n   {snippet}\n\n` |
| `image_cache.rs` | 65 | Cached image markdown | `![{description}]({url})` |
| `main.rs` | 460-464 | Generated image attachment | `\n\n![Generated image]({share_url})` |
| `provider/fal.rs` | 307 | Upload filename | `rockbot-{timestamp}.{ext}` |
| `harness.rs` | 1561-1572 | WebDAV dir naming | `d-{room_id}` (DM) or `r-{room_id}` (channel) |
| `edit_soul.rs` | 34 | Soul update confirmation | `Soul memory updated.` |
| `knowledge.rs` | 153 | Knowledge index version | `rockbot-knowledge/1` |
| `memory.rs` | 458 | Snapshot schema version | `rockbot-snapshot/1` |

---

## Summary Table

| # | What | Where | Sent To | Dynamic? |
|---|------|-------|---------|----------|
| 1 | **System prompt** — defines persona & capabilities | `harness.rs:20-42` | AI provider (`system` role) | Dynamic (`{name}`, `{max_context_mb}`, `{max_iterations}` from config) |
| 2 | **User message template** — wraps chat text | `harness.rs:185-209` | AI provider (`user` role) | Per-message |
| 3a-l | **Tool descriptions** — teach AI what tools do | 12 files in `tools/` | AI provider (tool definitions) | Static |
| 4 | **Tool param descriptions** — describe JSON fields | 12 files in `tools/` | AI provider (tool schema) | Static |
| 5 | **Default vision prompt** — fallback | `harness.rs:199` | Harness internal | Static |
| 6 | **Vision image interception** — bridge vision→image_gen | `harness.rs:721-779` | Harness internal | Dynamic |
| 7 | **Fallback/error messages** — loop/tool/API errors | `harness.rs:259-600`, `tool.rs:118-125`, `main.rs:555`, 6 tool files | RocketChat user / tool results | Partially |
| 8 | **Image gen request body** — image provider API | `types.rs:349-358`, `fal.rs:82,292`, `openrouter.rs:411` | image provider API | Dynamic |
| 9a | **Soul memory prefix** | `memory.rs:276-279` | AI provider (`system` role) | Dynamic |
| 9b | **Knowledge context** | `memory.rs:283-288` | AI provider (`system` role) | Dynamic |
| 9c | **Conversation summary** | `memory.rs:290-296` | AI provider (`system` role) | Dynamic |
| 10 | **Compress-for-summary** — creates bullet-point memory summary | `harness.rs:987-1017` | AI provider (one-shot) | Dynamic |
| 11a-g | **WebDAV / knowledge storage templates** | `harness.rs`, `edit_soul.rs`, `knowledge.rs`, `memory.rs`, `webdav.rs` | WebDAV storage / tool results | Dynamic |
| 12 | **Calendar result templates** | `calendar.rs` | AI provider (tool results) | Dynamic |
| 13 | **Debug binary messages** | `rocketchat/main.rs:39-68`, `client.rs:58-123` | RocketChat / console | Dynamic |
| 14 | **Miscellaneous** — placeholders, markers, path templates | 18 entries across 8 files | Various | Mixed |

(End of file)
