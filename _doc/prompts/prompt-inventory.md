# Prompt Inventory — rockbot

All prompts and prompt-adjacent strings in the Rust codebase, organized by what they do.

---

## 1. System Prompt (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:27-60`
**Constant:** `DEFAULT_SYSTEM_PROMPT`
**Sent to:** AI provider as the `system` role message in `ChatRequest.messages`
**Used via:** `build_system_prompt()` (line 770) → `MemoryManager::build_context()` → prepended as first message in context

Note: `{name}`, `{max_context_mb}`, `{max_iterations}`, and `{current_utc_time}` are replaced at runtime with config values via `build_system_prompt()`.

When secrets are loaded from WebDAV (see Section 15), `build_system_prompt_with_secrets()` (line 782) appends the secret UUID listing to the base prompt.

```
You are {name}, a helpful AI assistant running on a RocketChat server. \
**Always reply in the same language as the user's most recent message.** \
Tool results, tool-call arguments, and injected image prompts may appear in \
English — ignore them when choosing your reply language; match only the \
user's language. \
You respond to DMs and @mentions concisely and helpfully. \
Context space is limited to ~{max_context_mb}MB / 1M tokens. Keep your \
reasoning brief and avoid verbose explanations. Use tools to fetch \
information rather than guessing. You have up to {max_iterations} iterations \
per task — plan your tool calls efficiently. \
Current UTC time: {current_utc_time}. Use this for all time/date questions \
and calendar calculations — do not guess or fabricate dates. \
When you need information from the web, use the web_search tool. \
When you need to fetch a URL, use web_fetch. \
When you need to describe or analyze an image, use the vision tool. \
When you need to generate or edit images, use the image_gen tool. \
Share image_gen results as markdown `![{description}]({image_key})`. \
Do NOT fabricate fake image references — only image_gen produces real images. \
When you need to read, write, list, or manage files on remote storage, use the webdav tool. \
When you need to manage calendar events or todo tasks, use the calendar tool. \
Use the edit_soul tool ONLY when the user explicitly instructs you to update your soul, \
personality, or identity (e.g. 'save this in your soul', 'update your personality', \
'remember this about yourself'). Do NOT use it for frequently changing information such as \
to-do lists, directory structures, or dynamic tasks — store those in knowledge notes or \
WebDAV files to keep the soul stable and concise. \
Before saving knowledge, ALWAYS use recall_knowledge first to check whether a related note \
already exists. If one does, update or append to the existing note instead of creating a \
duplicate. If no related note exists, you MUST ask the user for explicit permission before \
creating a new knowledge note — do NOT create new notes without user consent. Use the \
save_knowledge tool to persist entries and the forget_knowledge tool to remove them. \
When you need to recall previously saved knowledge, use the recall_knowledge tool. \
Keep responses clear and to the point.\
```

---

## 2. User Message Template (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:236-260`
**Template:** `ChatMessage::user(format!("{}: {}", sender_name, clean_text))`
**Role:** `user`
**Purpose:** Wraps every incoming RocketChat message as `"SenderName: message text"` before appending to history. Preserves sender identity in group chats.

When image attachments are present (harness.rs:242-260), they are downloaded via
`download_attachment_refs()` (line 240) and encoded as data URIs, then injected into the
conversation as markdown `![image_name](image_name)` labels. The user message is
created as `ChatMessage::user_with_images` (line 257) with the prompt including an
`Attached:` line listing the image labels. If the text is empty, the prompt
defaults to `"SenderName:\nAttached: ![name](name)"` (line 250).

The harness later resolves `image_name` references in `image_gen` tool calls
by matching them against these cached data URIs via `inject_image_urls_from_refs()`
(harness.rs:1350-1399).

Example output: `"Alice: what's the weather in Tokyo?"`

---

## 3. Tool Descriptions (sent to AI provider in tool definitions)

### 3a. `calendar`
**File:** `crate-rockbot/src/tools/calendar.rs:274-292`
```
Manage calendar events on NextCloud CalDAV and display calendar grids.
Events are stored per-room — each room has its own calendar auto-created on first use.
Actions: mini_calendar (display a month calendar grid),
list_events (list events in a date range),
get_event (fetch a single event by UID),
add_event (create a new event), update_event (modify an existing event by UID),
delete_event (remove an event by UID).
add_event requires summary, dtstart (ISO 8601, UTC), dtend (ISO 8601, UTC).
update_event uses merge semantics: specify only the fields you want to change;
omitted fields keep their existing values.
Optional for both: description, location, rrule (recurrence rule, RFC 5545),
reminder_minutes (e.g. 15).
mini_calendar accepts optional month_offset (0=current month, 1=next, -1=previous)
and timezone (default UTC).
All date/time values must be in UTC — use the Z suffix (e.g. 20260615T140000Z)
or omit seconds (e.g. 20260601T000000Z). Floating times (without Z) are not supported.
```

### 3b. `search_web`
**File:** `crate-rockbot/src/tools/web_search.rs:398-402`
```
Search the web using the configured search provider (Exa or Brave). Returns ranked results
with titles, URLs, highlights, and dates. Supports optional type (auto/fast/deep),
num_results, and contents_mode (highlights/text/deep) parameters.
```

Note: The tool name exposed to the LLM is `search_web` (line 395), not `web_search`. Two search providers are supported: `ExaSearchProvider` (lines 28-211) and `BraveSearchProvider` (lines 215-326). The provider is selected via config at construction time.

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
**File:** `crate-rockbot/src/tools/webdav.rs:269-278`
```
Manage files on remote WebDAV storage (NextCloud). Each room has its own file space —
paths are automatically scoped. Actions: read (get file content), write (create/overwrite
a file), edit (replace oldString with newString — reads file first, fails if oldString not
found or matches multiple times, 500 KB max), list (list directory contents), mkdir
(create directory tree), delete (remove file/directory), exists (check if path exists),
rename (move or rename a file/directory — path is source, destination is target).
```

### 3f. `image_gen`
**File:** `crate-rockbot/src/tools/image_gen.rs:140-144`
```
Generate or edit an image. Provide a prompt and optional aspect_ratio (e.g. '16:9').
User attachments are auto-provided as image_urls for editing.
Returns {"ok": true, "image_key": "..."} — share result as `![desc]({image_key})`.
```

Note: `image_size` is NOT exposed to the LLM as a tool parameter — it is derived from the `aspect_ratio` parameter at runtime (mapped to a preset like `"16:9"` → `{3840, 2160}`). `size_tier` is set from `[image_model]` config `default_image_size_tier`. The harness injects `room_id`, `webdav_dir`, and `image_cache_key` automatically. `image_urls` are auto-injected from message attachments.

### 3g. `edit_soul`
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

### 3h. `save_knowledge`
**File:** `crate-rockbot/src/tools/save_knowledge.rs:31-36`
```
Save a piece of knowledge for future reference. Use this when the user says 'remember', 'learn', or shares important information worth persisting. Each entry gets a .md file and is indexed for contextual retrieval.
```

### 3i. `forget_knowledge`
**File:** `crate-rockbot/src/tools/forget_knowledge.rs:25-29`
```
Remove a previously saved knowledge entry. Provide the topic title of the entry to delete.
The .md file is deleted and the entry is removed from the knowledge index.
```

### 3j. `recall_knowledge`
**File:** `crate-rockbot/src/tools/recall_knowledge.rs:25-29`
```
Search the knowledge index for entries matching a query. If no query is given,
returns all stored knowledge entries. Matches by topic title, when_useful description, and tags.
```

### 3k. `reset_memory`
**File:** `crate-rockbot/src/tools/reset_memory.rs:28-32`
```
Clear all conversation memory for this room instantly.
Use when the user says `!reset`, `!clearmemory`, or explicitly asks to clear/reset memory.
No LLM call or summary generation — memory is wiped immediately.
```

Note: The `reset_memory` tool's `execute()` (line 47-52) always returns an error — actual execution is intercepted by `AgentHarness::process_message()` which calls `reset_room_if_needed()` (harness.rs:959).

---

## 4. Tool Parameter Descriptions (sent to AI provider in tool schema)

| File | Lines | Tool | Parameter | Description |
|------|-------|------|-----------|-------------|
| `calendar.rs` | 298-301 | `calendar` | `action` | Calendar operation: mini_calendar, list_events, get_event, add_event, update_event, delete_event |
| `calendar.rs` | 303-305 | `calendar` | `start` | Start of date range in ISO 8601 UTC (e.g. 20260601T000000Z). Used by list_events. |
| `calendar.rs` | 307-309 | `calendar` | `end` | End of date range in ISO 8601 UTC. Used by list_events. |
| `calendar.rs` | 311-313 | `calendar` | `uid` | Event UID. Required for update_event and delete_event. |
| `calendar.rs` | 315-317 | `calendar` | `summary` | Event title/summary. Required for add_event and update_event. |
| `calendar.rs` | 319-321 | `calendar` | `dtstart` | Event start in ISO 8601 UTC (e.g. 20260615T140000Z). Required for add_event. |
| `calendar.rs` | 323-325 | `calendar` | `dtend` | Event end in ISO 8601 UTC. Required for add_event. |
| `calendar.rs` | 327-329 | `calendar` | `description` | Optional event description/details. |
| `calendar.rs` | 331-333 | `calendar` | `location` | Optional event location. |
| `calendar.rs` | 335-337 | `calendar` | `rrule` | Optional recurrence rule in RFC 5545 format (e.g. FREQ=WEEKLY;BYDAY=MO). |
| `calendar.rs` | 339-341 | `calendar` | `reminder_minutes` | Optional reminder in minutes before event (e.g. 15). |
| `calendar.rs` | 343-345 | `calendar` | `timezone` | IANA timezone name (e.g. Asia/Macau, America/New_York). Default: UTC. Used by mini_calendar. |
| `calendar.rs` | 347-349 | `calendar` | `month_offset` | Month offset for mini_calendar: 0=current month, 1=next month, -1=previous. Default: 0. |
| `web_search.rs` | 408-410 | `search_web` | `query` | The search query to execute |
| `web_search.rs` | 412-415 | `search_web` | `type` | Search type: auto (balanced), fast (quick results), deep (comprehensive). Default: auto |
| `web_search.rs` | 417-420 | `search_web` | `contents_mode` | Content mode: highlights returns snippets (default), text returns full page content, deep enables comprehensive search |
| `web_search.rs` | 422-426 | `search_web` | `num_results` | Number of results to return (default: 5, max: 20) |
| `web_fetch.rs` | 693 | `web_fetch` | `url` | The URL to fetch (required) |
| `web_fetch.rs` | 697 | `web_fetch` | `method` | HTTP method (default: GET) |
| `web_fetch.rs` | 702 | `web_fetch` | `headers` | HTTP headers as key-value pairs, e.g. {"Authorization": "token xyz", "Content-Type": "application/json"} |
| `web_fetch.rs` | 706 | `web_fetch` | `body` | Raw string body for POST/PUT/PATCH requests |
| `web_fetch.rs` | 710 | `web_fetch` | `body_json` | JSON body — serialized as request body with Content-Type: application/json |
| `web_fetch.rs` | 714 | `web_fetch` | `file_from_webdav` | WebDAV file path to read and use as request body |
| `web_fetch.rs` | 718 | `web_fetch` | `save_to_webdav` | WebDAV file path to save the response body |
| `web_fetch.rs` | 722 | `web_fetch` | `format` | Output format: json returns structured metadata, markdown converts HTML to markdown for AI, raw returns unmodified text (default: raw) |
| `web_fetch.rs` | 727 | `web_fetch` | `verify` | Perform a web search to cross-verify content (default: false) |
| `vision.rs` | 138 | `vision` | `url` | URL of the image to fetch (public web or WebDAV file) |
| `vision.rs` | 142 | `vision` | `prompt` | Optional prompt for the LLM to use when analyzing this image |
| `webdav.rs` | 284-287 | `webdav` | `action` | The WebDAV operation to perform (enum: read, write, edit, list, mkdir, delete, exists, rename) |
| `webdav.rs` | 289-291 | `webdav` | `room_id` | Room ID for scoping the operation (injected automatically if omitted) |
| `webdav.rs` | 293-295 | `webdav` | `path` | File or directory path relative to the room root |
| `webdav.rs` | 297-299 | `webdav` | `content` | File content to write (required for 'write' action) |
| `webdav.rs` | 301-303 | `webdav` | `oldString` | Exact text to find and replace (required for 'edit' action, must be unique in the file) |
| `webdav.rs` | 305-307 | `webdav` | `newString` | Replacement text (required for 'edit' action) |
| `webdav.rs` | 309-311 | `webdav` | `destination` | Target path for rename/move (required for 'rename' action, relative to room root) |
| `image_gen.rs` | 149-152 | `image_gen` | `prompt` | Text description of the image to generate |
| `image_gen.rs` | 153-156 | `image_gen` | `aspect_ratio` | Aspect ratio for the generated image as W:H (e.g. '16:9', '2:3', '1:1') |
| `image_gen.rs` | 157-160 | `image_gen` | `room_id` | Room ID for image storage (injected automatically if omitted) |
| `image_gen.rs` | 161-165 | `image_gen` | `image_urls` | URLs of images to edit (e.g., share_url from a previous image_gen result). Omit to generate a new image. Auto-injected from user attachments and message images. |
| `image_gen.rs` | 166-170 | `image_gen` | `reference_image_key` | The image_key of a previously generated image to edit. Alternative to providing explicit image_urls. |
| `edit_soul.rs` | 67 | `edit_soul` | `content` | Full soul.md content following the template: # Soul Memory\\n\\n- My name is Name ✨\\n- ...\\n- ... |
| `edit_soul.rs` | 71 | `edit_soul` | `webdav_dir` | Room WebDAV directory key (injected automatically) |
| `save_knowledge.rs` | 42-45 | `save_knowledge` | `topic` | Short title or topic for the entry (e.g. 'DB API', 'Build Commands') |
| `save_knowledge.rs` | 46-49 | `save_knowledge` | `content` | Markdown body of the knowledge entry |
| `save_knowledge.rs` | 50-54 | `save_knowledge` | `when_useful` | Describe the situation that makes this knowledge relevant, used for automatic retrieval (e.g. 'when calling the database API') |
| `save_knowledge.rs` | 55-58 | `save_knowledge` | `tags` | Comma-separated keywords for search (e.g. 'api, database, python') |
| `save_knowledge.rs` | 59-63 | `save_knowledge` | `priority` | Knowledge priority: P0 (highest, always recalled), P1 (high, default), P2 (medium), P3 (low). Higher priority means more frequently recalled. |
| `forget_knowledge.rs` | 35-38 | `forget_knowledge` | `topic` | Title or topic of the knowledge entry to delete |
| `recall_knowledge.rs` | 35-38 | `recall_knowledge` | `query` | Topic or keyword to search for in knowledge entries. Leave empty to retrieve all entries. |
| `reset_memory.rs` | 38-41 | `reset_memory` | `room_id` | Room UUID (injected automatically) |

---

## 5. Default Vision Prompt (fallback for tool execution)

**File:** `crate-rockbot/src/harness.rs:249-250`
```
{}:\nAttached: {}
```
**Used when:** A user sends image attachments with no text. The harness constructs a user message as `"{sender_name}:\nAttached: ![name](name) ..."` with the attached image labels. Not used by the `vision` tool itself (vision tool has its own optional `prompt` parameter set by the LLM).

---

## 6. Image URL Injection for image_gen (harness internal)

**File:** `crate-rockbot/src/harness.rs:1350-1399`

The function `inject_image_urls_from_refs()` is called before each `image_gen` tool execution.
It populates the `image_urls` parameter by matching image names referenced in the tool-call
arguments against three sources:

1. **User-attached images** — data URIs from the current message's attachments whose filename appears in the prompt (lines 1363-1368)
2. **Image pool** — previously generated images (`image_gen` results) and vision-fetched images cached per-room in `AgentHarness.image_pool` (lines 1369-1379)
3. **Agent-provided URLs** — any `image_urls` already in the arguments (e.g. fal CDN from previous generation or share_url), deduplicated (lines 1380-1389)

The function also injects `room_id` and `webdav_dir` into the arguments JSON (lines 1359-1360).

---

## 7. Fallback / Error Messages (returned to RocketChat user or as tool results)

### 7a. Agent loop fallbacks (harness.rs)

| Line | Condition | Text |
|------|-----------|------|
| 327 | Max agent iterations exceeded | `I'm sorry, I got stuck in a loop. Could you rephrase your request?` |
| 667 | AI returned empty text (stored in history) | `(no response)` |
| 674 | AI returned empty text (user-facing) | `I processed your request but received an empty response.` |
| 691 | AI returned no text at all | `I received a response but it was empty. Please try again.` |
| 700-744 | Context length exceeded | Hard memory reset + hard-truncation retry (one attempt); if still exceeded, falls through to generic error below |
| 751 | AI provider error (dynamic) | `I encountered an error: {e}. Please try again.` |

### 7b. Tool result errors (tool.rs)

| Line | Condition | Text |
|------|-----------|------|
| 118 | Tool `execute()` returns Err | `Tool execution error: {e}` |
| 125 | LLM requests unknown tool | `Unknown tool: {name}` |

### 7c. Tool-specific errors

| File | Line | Condition | Text |
|------|------|-----------|------|
| `web_search.rs` | 65-68 | Exa: no API key configured | `Exa search requires an API key. Configure it in the [search.exa] section of config.toml.` |
| `web_search.rs` | 131-135 | Exa returns 401 | `Exa search failed: invalid API key (401). Check your [search.exa] config.` |
| `web_search.rs` | 154 | Exa response missing results array | `Exa returned no results array` |
| `web_search.rs` | 157 | Exa returns empty results | `No search results found.` |
| `web_search.rs` | 253-255 | Brave: no API key configured | `Brave Search requires an API key. Configure it in the [search.brave] section of config.toml.` |
| `web_search.rs` | 278-280 | Brave returns 401 | `Brave Search failed: invalid API key (401). Check your [search.brave] config.` |
| `web_search.rs` | 282-285 | Brave returns other error | `Brave Search failed with status {status}: {error_body}` |
| `web_search.rs` | 294 | Brave response missing results array | `Brave returned no results array` |
| `web_search.rs` | 297 | Brave returns empty results | `No search results found.` |
| `recall_knowledge.rs` | 56 (via knowledge.rs:370) | No knowledge entries in room | `No knowledge entries found for this room.` |
| `knowledge.rs` | 378 | Recall query matches nothing | `No knowledge entry found matching '{query}'.` |

### 7d. Main loop error (main.rs)

| Line | Condition | Text |
|------|-----------|------|
| 575 | process_message returns Err | `Error processing message: {e}` |

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

Tool result returned to LLM (image_gen.rs:305-313):
```json
{"ok": true, "webdav_path": "<WebDAV path>", "image_key": "<call_id>"}
```
With conditional `share_url` field (line 310-312) if a NextCloud share link was created:
```json
{"ok": true, "webdav_path": "<WebDAV path>", "image_key": "<call_id>", "share_url": "<NextCloud share link>/download"}
```

The LLM includes `image_key` in its reply markdown. The agent loop (main.rs:483) replaces it with a NextCloud share URL (`share_url`, 7-day expiry) before sending to RocketChat.

---

## 9. Memory / Context Prompts (sent to AI provider as system messages)

### 9a. Soul memory prefix
**File:** `crate-rockbot/src/memory.rs:268-271`
**Type:** Dynamic — loaded from WebDAV `memory/soul.md`
```
[Core memory — permanent preferences, identity, and facts]
{content from soul.md}
```
Injected as a second system message when soul content is non-empty. Truncated at `max_soul_chars` with a `\n\n[truncated]` marker appended (line 263).

### 9b. Knowledge context (from WebDAV knowledge index)
**File:** `crate-rockbot/src/memory.rs:275-280`
**Type:** Dynamic — loaded by `AgentHarness::load_knowledge_for_room()` (harness.rs:1087-1146)
via `refresh_knowledge_context()` (harness.rs:1148-1163) and stored in `MemoryManager.knowledge`.
Individual entries formatted as:
```
[Knowledge: {title}]
{body}
```
Entries joined with `\n---\n` separator and wrapped as:
```
[Knowledge — automatically recalled for this conversation]
{joined entries}
```
Injected as a system message when relevant knowledge entries exist for the room. Fetched on each `process_message` call before building context (harness.rs:279).

---

## 10. WebDAV Storage Templates (not AI prompts)

### 10a. Soul memory file
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

### 10b. Knowledge entry .md file
**File:** `crate-rockbot/src/knowledge.rs:227-230`
**Stored at:** `{room_dir}knowledge/{filename}.md`
```
# {topic}

**When Useful:** {when_useful}
**Tags:** {tags}
**Created:** {timestamp}
**Updated:** {timestamp}

{content}
```

### 10c. Snapshot file (persistence)
**File:** `crate-rockbot/src/memory.rs:435` (schema version string)
**Stored at:** `{room_dir}memory/snapshot.json`
JSON snapshot of room state (messages, soul, summary, archive_seq). Schema version `rockbot-snapshot/1`. Cached read on restore, rebuilt on dirty flag.

### 10d. WebDAV tool result templates (webdav.rs)
| Line | Template | Example |
|------|----------|---------|
| 69 | Image read | `![{name}](data:{mime};base64,{data})` |
| 85 | Write success | `Written {bytes} bytes to {path}` |
| 135-139 | Edit success | `Edited {path}: replaced 1 occurrence ({bytes} bytes written)` |
| 159-168 | Directory listing | `Contents of '{dir}':\n\n  {DIR/FILE}  {size}  {date}  {name}` |
| 156 | Empty directory | `Directory '{dir}' is empty.` |
| 179 | Mkdir success | `Directory created: {path}` |
| 189 | Delete success | `Deleted: {path}` |
| 200-204 | Exists check | `Path '{path}': exists` / `not found` |
| 215 | Rename success | `Renamed: {from} -> {to}` |

### 10e. Secrets sanitization (webdav.rs:218-247)
When the LLM attempts to read `secrets.toml` via the webdav tool, the `sanitize_secrets_toml()` method
preserves `host` and `key` fields but replaces every `value` field with `"abcd"` (line 228).
If the file cannot be read, a fallback placeholder is returned:
```toml
[[secrets]]
host = "unknown"
key = "placeholder"
value = "abcd"
```

### 10f. Knowledge recall templates
| File | Line | Template |
|------|------|----------|
| `knowledge.rs` | 227-230 | `.md` body format (see 10b) |
| `knowledge.rs` | 363-366 | `[Knowledge: {title}]\n{body}` (recalled entry) |
| `knowledge.rs` | 370 | `No knowledge entries found for this room.` |
| `knowledge.rs` | 378 | `No knowledge entry found matching '{query}'.` |
| `save_knowledge.rs` | 89-92 | `Knowledge saved: [{topic}] {topic}` |
| `forget_knowledge.rs` | 54 | `Knowledge entry '{topic}' deleted.` |
| `knowledge.rs` | 257-259 | `Knowledge entry '{topic}' not found.` |

---

## 11. Calendar / CalDAV Result Templates (calendar.rs — not AI prompts)

| File | Lines | Template |
|------|-------|----------|
| `calendar.rs` | 112-138 | `Event: {summary}\n  UID: {uid}\n  When: {start} to {end}\n` (+ optional description, location, recurrence, reminder lines) |
| `calendar.rs` | 379 | `No events found between {start} and {end}.` |
| `calendar.rs` | 381-386 | `{count} event(s) between {start} and {end}:\n\n` |
| `calendar.rs` | 418 | `Event created with UID: {uid}` |
| `calendar.rs` | 436 | `Event updated: {uid}` |
| `calendar.rs` | 446 | `Event deleted: {uid}` |

---

## 12. RocketChat Debug Binary Messages

**File:** `crate-rocketchat/src/main.rs`

| Line | Command | Reply |
|------|---------|-------|
| 39-48 | — | Console-only: `DM from {sender_name}` / `#{name} from {sender_name}` |
| 53-57 | `!ping` | `pong @{sender_name}` |
| 58-62 | `!echo <text>` | Echoes the text back |
| 63-67 | `!help` | `Commands: !ping, !echo <text>, !help` |

**File:** `crate-rocketchat/src/client.rs`

| Line | Purpose | Template |
|------|---------|----------|
| 66-68 | Code-block reply wrapper | `` ```{text}``` `` |
| 149, 155 | Bot mention pattern for detection | `@{username}` (stored in `bot_name` field at line 149, constructed at line 155) |

---

## 13. Miscellaneous Prompt-Adjacent Strings

| File | Line | Purpose | Text |
|------|------|---------|------|
| `memory.rs` | 263 | Soul truncation marker | `\n\n[truncated]` (appended to soul memory when over char limit) |
| `memory.rs` | 524, 533 | Context truncation marker | `\n\n[...truncated]` (appended to truncated message texts) |
| `memory.rs` | 554 | Image stripping placeholder | `[image]` (replaces image parts in non-latest messages) |
| `provider/deepseek.rs` | 71 | DeepSeek image placeholder | `[image]` (replaces image data URIs, DeepSeek lacks vision) |
| `tools/web_fetch.rs` | 526 | Web fetch truncation | `\n\n... (truncated)` (appended when content exceeds 10000 chars) |
| `tools/web_fetch.rs` | 362, 373 | Saved-to note | `\n\nSaved to WebDAV: {path}` |
| `tools/web_fetch.rs` | 365, 376 | Related sources header | `\n\n## Related Sources\n\n` |
| `tools/web_fetch.rs` | 458-474 | Related source entry | `{idx}. **{title}**\n   URL: {url}\n   {snippet}\n\n` |
| `image_cache.rs` | 70 | Cached image markdown | `![{description}]({url})` |
| `main.rs` | 483 | Generated image attachment | `\n\n![Generated image]({share_url})` |
| `provider/fal.rs` | 307 | Upload filename | `rockbot-{timestamp}.{ext}` |
| `harness.rs` | 1535-1546 | WebDAV dir naming | `d-{room_id}` (DM) or `r-{room_id}` (channel) |
| `edit_soul.rs` | 34 | Soul update confirmation | `Soul memory updated.` |
| `knowledge.rs` | 152 | Knowledge index version | `rockbot-knowledge/1` |
| `memory.rs` | 435 | Snapshot schema version | `rockbot-snapshot/1` |

---

## 14. Secret UUID Resolution (harness internal)

**File:** `crate-rockbot/src/harness.rs:1300-1533`

### 14a. Types

| Type | Line | Fields |
|------|------|--------|
| `SecretEntry` | 1301-1305 | `host`, `key`, `value` — raw TOML row from `secrets.toml` |
| `ResolvedSecret` | 1307-1313 | `uuid`, `host`, `key`, `value` — entry with deterministic UUID |
| `SecretsToml` | 1315-1319 | `secrets: Vec<SecretEntry>` — TOML deserialization wrapper |

### 14b. Constants

**Line 1321-1324:** `SECRET_UUID_NAMESPACE` — fixed UUID v5 namespace used to derive deterministic UUIDs from `(host, key)` pairs.

### 14c. Functions

| Line | Function | Purpose |
|------|----------|---------|
| 1326-1329 | `generate_secret_uuid(host, key)` | Deterministic v5 UUID from `"{host}:{key}"` |
| 1331-1340 | `build_secret_uuids_prompt(secrets)` | Builds prompt lines listing available secrets as `secret:<UUID> ({key})` |
| 1342-1348 | `inject_room_context(arguments, room_id, webdav_dir)` | Injects `room_id` and `webdav_dir` into tool-call arguments |
| 1350-1399 | `inject_image_urls_from_refs(arguments, room_id, webdav_dir, refs, image_pool)` | Matches image names to data URIs and injects into `image_urls` |
| 1401-1433 | `replace_secret_refs(value, secrets)` | Replaces inline `secret:<UUID>` strings with actual secret values |
| 1435-1467 | `filter_secrets_by_host(entries, args_json)` | Parses `url` from tool-call args, matches scheme+host+port, returns UUID→value map |
| 1469-1476 | `resolve_secret_refs_deep(args_json, secrets)` | Walks a JSON value tree applying `replace_secret_refs` to every string |
| 1478-1497 | `resolve_json_value(value, secrets)` | Recursive walker used by `resolve_secret_refs_deep` |
| 1499-1533 | `load_secrets_from_webdav(webdav, room_dir)` | Reads `{room_dir}/secrets.toml`, parses as `SecretsToml`, returns `Vec<ResolvedSecret>` |
| 782-786 | `build_system_prompt_with_secrets(secrets)` | Appends secret UUID listing to base system prompt |

### 14d. System prompt extension

When secrets are loaded, the following text is appended to the system prompt (line 1335-1338):
```
Available API secrets (use secret:<UUID> to authenticate):
- secret:{uuid1} ({key1})
- secret:{uuid2} ({key2})
```

### 14e. secrets.toml storage format

Stored at `{room_dir}/secrets.toml`:
```toml
[[secrets]]
host = "https://api.example.com:443"
key  = "X-API-Key"
value = "actual-secret-value"
```
Multiple `[[secrets]]` entries are supported. Host matching uses scheme+hostname+port from the URL.

### 14f. Tool-call integration

Before a `web_fetch` tool call executes, `filter_secrets_by_host()` extracts the `url` field from the
tool-call arguments and filters secrets by matching host. Then `resolve_secret_refs_deep()` walks
the entire arguments JSON, replacing any `secret:<UUID>` references with the actual secret values.
This lets the LLM write `Authorization: secret:abc-123-...` and have it transparently resolved at
the call site, while the LLM only ever sees opaque UUIDs.

---

## Summary Table

| # | What | Where | Sent To | Dynamic? |
|---|------|-------|---------|----------|
| 1 | **System prompt** — defines persona & capabilities | `harness.rs:27-60` | AI provider (`system` role) | Dynamic (`{name}`, `{max_context_mb}`, `{max_iterations}`, `{current_utc_time}` from config; secret UUID listing appended when available) |
| 2 | **User message template** — wraps chat text | `harness.rs:236-260` | AI provider (`user` role) | Per-message |
| 3a-k | **Tool descriptions** — teach AI what tools do | 11 files in `tools/` | AI provider (tool definitions) | Static |
| 4 | **Tool param descriptions** — describe JSON fields | 11 files in `tools/` | AI provider (tool schema) | Static |
| 5 | **Default vision prompt** — fallback | `harness.rs:249-250` | Harness internal | Static |
| 6 | **Image URL injection for image_gen** — matches refs to data URIs | `harness.rs:1350-1399` | Harness internal (tool-call args) | Dynamic |
| 7 | **Fallback/error messages** — loop/tool/API errors | `harness.rs:327-751`, `tool.rs:118-125`, `main.rs:575`, 4 tool files | RocketChat user / tool results | Partially |
| 8 | **Image gen request body** — image provider API | `types.rs:349-358`, `fal.rs:82,292`, `openrouter.rs:411` | image provider API | Dynamic |
| 9a | **Soul memory prefix** | `memory.rs:268-271` | AI provider (`system` role) | Dynamic |
| 9b | **Knowledge context** | `memory.rs:275-280`, `harness.rs:1087-1163` | AI provider (`system` role) | Dynamic |
| 10a-f | **WebDAV / knowledge storage templates** | `edit_soul.rs`, `knowledge.rs`, `memory.rs`, `webdav.rs` | WebDAV storage / tool results | Dynamic |
| 11 | **Calendar result templates** | `calendar.rs:112-446` | AI provider (tool results) | Dynamic |
| 12 | **Debug binary messages** | `rocketchat/main.rs:39-67`, `client.rs:66-155` | RocketChat / console | Dynamic |
| 13 | **Miscellaneous** — placeholders, markers, path templates | 15 entries across 8 files | Various | Mixed |
| 14 | **Secret UUID resolution** — deterministic UUIDs for API secrets | `harness.rs:1300-1533` | AI provider (system prompt extension) + tool-call args | Dynamic |

(End of file)
