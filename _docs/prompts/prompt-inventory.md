# Prompt Inventory — rockbot

All prompts and prompt-adjacent strings in the Rust codebase, organized by what they do.

---

## 1. System Prompt (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:15-35`
**Constant:** `DEFAULT_SYSTEM_PROMPT`
**Sent to:** AI provider as the `system` role message in `ChatRequest.messages`
**Used via:** `build_system_prompt()` (line 284) → `MemoryManager::build_context()` (memory.rs:208) → prepended as first message in context

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
The image_gen tool returns both a WebDAV path and an original fal.ai CDN URL — \
always share the fal.ai CDN URL with the user so they can view or share the image directly. \
When a user asks you to remember something, or you want to save permanent notes, \
preferences, or identity information, use the edit_soul tool. \
When a user says !learn, !jot, !skill, !secret, !save or shares important \
information worth persisting, use the save_knowledge tool. \
When a user says !forget or asks to remove something you learned, \
use the forget_knowledge tool. \
When you need to recall previously saved knowledge, use the recall_knowledge tool. \
Answer in the same language as the user. \
Keep responses clear and to the point.
```

---

## 2. User Message Template (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:125`
**Template:** `ChatMessage::user(format!("{}: {}", sender_name, clean_text))`
**Role:** `user`
**Purpose:** Wraps every incoming RocketChat message as `"SenderName: message text"` before appending to history. Preserves sender identity in group chats.

Example output: `"Alice: what's the weather in Tokyo?"`

---

## 3. Tool Descriptions (sent to AI provider in tool definitions)

### 3a. `datetime`
**File:** `crate-rockbot/src/tools/datetime.rs:96-99`
```
Get the current UTC date and time. Returns ISO 8601 timestamp, human-readable date with weekday, and Unix epoch seconds.
```

### 3b. `web_search`
**File:** `crate-rockbot/src/tools/web_search.rs:140-143`
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
**File:** `crate-rockbot/src/tools/vision.rs:76-78`
```
Download and describe an image. Provide an image URL and an optional prompt.
```

### 3e. `webdav`
**File:** `crate-rockbot/src/tools/webdav.rs:184-191`
```
Manage files on remote WebDAV storage (NextCloud). Each room has its own file space —
paths are automatically scoped. Actions: read (get file content), write (create/overwrite
a file), edit (replace oldString with newString — reads file first, fails if oldString not
found or matches multiple times, 500 KB max), list (list directory contents), mkdir
(create directory tree), delete (remove file/directory), exists (check if path exists).
```

### 3f. `calendar`
**File:** `crate-rockbot/src/tools/calendar.rs:162-173`
```
Manage calendar events on NextCloud CalDAV. Actions: list_events (list events in a date
range), get_event (fetch a single event by UID), add_event (create a new event),
update_event (modify an existing event by UID), delete_event (remove an event by UID).
add_event requires summary, dtstart (ISO 8601), dtend (ISO 8601). update_event uses
merge semantics: specify only the fields you want to change; omitted fields keep their
existing values. Optional for both: description, location, rrule (recurrence rule, RFC
5545), reminder_minutes (e.g. 15).
```

### 3g. `image_gen`
**File:** `crate-rockbot/src/tools/image_gen.rs:96-104`
```
Generate an image using fal.ai. Specify a prompt and optional parameters
(quality, image_size, output_format, num_images, model_id).
model_id defaults to fal-ai/flux/schnell (fast).
Use openai/gpt-image-2 for GPT Image 2 with higher quality.
For GPT Image 2, recommend quality: medium, output_format: png, and
image_size: landscape_16_9 (4K) or portrait_16_9 / square_hd / landscape_4_3.
Returns both the WebDAV path and the original fal.ai CDN URL — prefer
the fal.ai URL when sharing the image with the user.
```

### 3h. `edit_soul`
**File:** `crate-rockbot/src/tools/edit_soul.rs:143-148`
```
Edit the bot's permanent memory (soul) for this room.
Actions: append (add a new section or content),
replace (replace an existing section's content),
delete_section (remove a section entirely).
Use section_header to target the ## Section name.
```

---

## 4. Tool Parameter Descriptions (sent to AI provider in tool schema)

| File | Lines | Tool | Parameter | Description |
|------|-------|------|-----------|-------------|
| `datetime.rs` | 105, 108 | `datetime` | `format` | Output format: iso (ISO 8601), human (readable with weekday), unix (epoch seconds), full (all three). Default: full |
| `web_search.rs` | 151 | `web_search` | `query` | The search query to execute |
| `web_search.rs` | 156 | `web_search` | `type` | Search type: auto (balanced with autoprompt), fast (quick results), deep (comprehensive). Default: auto |
| `web_search.rs` | 162 | `web_search` | `num_results` | Number of results to return (default: 5, max: 20) |
| `web_fetch.rs` | 442 | `web_fetch` | `url` | The URL to fetch |
| `web_fetch.rs` | 447-449 | `web_fetch` | `format` | Output format: json returns structured metadata, markdown converts HTML to markdown for AI, raw returns unmodified text (default: raw) |
| `web_fetch.rs` | 453 | `web_fetch` | `verify` | Perform a web search to cross-verify content (default: false) |
| `vision.rs` | 86 | `vision` | `url` | URL of the image to analyze |
| `vision.rs` | 90 | `vision` | `prompt` | Optional description of what to look for in the image |
| `webdav.rs` | 201 | `webdav` | `action` | The WebDAV operation to perform |
| `webdav.rs` | 205 | `webdav` | `room_id` | Room ID for scoping the operation (injected automatically if omitted) |
| `webdav.rs` | 209 | `webdav` | `path` | File or directory path relative to the room root |
| `webdav.rs` | 213 | `webdav` | `content` | File content to write (required for 'write' action) |
| `webdav.rs` | 217 | `webdav` | `oldString` | Exact text to find and replace (required for 'edit' action, must be unique in the file) |
| `webdav.rs` | 221 | `webdav` | `newString` | Replacement text (required for 'edit' action) |
| `calendar.rs` | 182 | `calendar` | `action` | Calendar operation to perform |
| `calendar.rs` | 186 | `calendar` | `start` | Start date/time in ISO 8601 (e.g. 20260601T000000Z). Used by list_events. |
| `calendar.rs` | 190 | `calendar` | `end` | End date/time in ISO 8601. Used by list_events. |
| `calendar.rs` | 194 | `calendar` | `uid` | Event UID. Required for update_event and delete_event. |
| `calendar.rs` | 198 | `calendar` | `summary` | Event title/summary. Required for add_event and update_event. |
| `calendar.rs` | 202 | `calendar` | `dtstart` | Event start in ISO 8601 (e.g. 20260615T140000Z). Required for add_event. |
| `calendar.rs` | 206 | `calendar` | `dtend` | Event end in ISO 8601. Required for add_event. |
| `calendar.rs` | 210 | `calendar` | `description` | Optional event description/details. |
| `calendar.rs` | 214 | `calendar` | `location` | Optional event location. |
| `calendar.rs` | 218 | `calendar` | `rrule` | Optional recurrence rule in RFC 5545 format (e.g. FREQ=WEEKLY;BYDAY=MO). |
| `calendar.rs` | 222 | `calendar` | `reminder_minutes` | Optional reminder in minutes before event (e.g. 15). |
| `image_gen.rs` | 113 | `image_gen` | `prompt` | Text description of the image to generate |
| `image_gen.rs` | 117 | `image_gen` | `room_id` | Room ID for image storage (injected automatically if omitted) |
| `image_gen.rs` | 121 | `image_gen` | `model_id` | fal.ai model ID (default: fal-ai/flux/schnell; use openai/gpt-image-2 for GPT Image 2) |
| `image_gen.rs` | 123-126 | `image_gen` | `quality` | Image quality / reasoning budget. Default: high. For gpt-image-2, medium recommended. |
| `image_gen.rs` | 128-130 | `image_gen` | `image_size` | Aspect ratio preset or custom {"width": N, "height": N} JSON. Presets include square_hd, landscape_16_9 (4K), portrait_16_9, landscape_4_3, etc. Default: landscape_4_3. |
| `image_gen.rs` | 132-135 | `image_gen` | `output_format` | Output image format: jpeg, png, webp. Default: png. |
| `image_gen.rs` | 137-139 | `image_gen` | `num_images` | Number of images to generate per request. Default: 1. |
| `edit_soul.rs` | 155-157 | `edit_soul` | `action` | Soul memory operation: append (add new section/content), replace (update existing section), delete_section (remove a section) |
| `edit_soul.rs` | 162 | `edit_soul` | `section_header` | Target ## Section name (e.g. Preferences, Identity, Notes) |
| `edit_soul.rs` | 166 | `edit_soul` | `content` | New content for the section (required for append and replace) |
| `edit_soul.rs` | 170 | `edit_soul` | `webdav_dir` | Room WebDAV directory key (injected automatically) |

---

## 5. Default Vision Prompt (fallback for tool execution)

**File:** `crate-rockbot/src/tools/vision.rs:110`
```
Describe this image in detail.
```
**Used when:** AI calls `vision` tool without providing a `prompt` argument. Passed to `describe_image()` as the prompt parameter.

---

## 6. Fallback / Error Messages (returned to RocketChat user)

**File:** `crate-rockbot/src/harness.rs`

| Line | Condition | Text |
|------|-----------|------|
| 154 | Max agent iterations exceeded | `I'm sorry, I got stuck in a loop. Could you rephrase your request?` |
| 248 | AI returned empty text (stored in history) | `(no response)` |
| 255 | AI returned empty text (user-facing) | `I processed your request but received an empty response.` |
| 262 | AI returned no text at all | `I received a response but it was empty. Please try again.` |
| 269 | AI provider error (dynamic) | `I encountered an error: {e}. Please try again.` |

**File:** `crate-rockbot/src/main.rs`

| Line | Condition | Text |
|------|-----------|------|
| 342 | Outer catch-all for message processing | `Error processing message: {e}` |

---

## 7. Image Generation Prompt (sent to fal.ai API)

**File:** `crate-rockbot/src/provider/fal.rs:110-147` (`submit_request`)

The user-provided `prompt` string plus optional parameters are sent as a JSON body:
```json
{
  "prompt": "<user provided>",
  "quality": "<optional>",
  "output_format": "<optional>",
  "num_images": <optional>,
  "image_size": "<optional preset or object>"
}
```
Sent as a POST to `{base_url}/{model_id}` with `Authorization: Key {api_key}` header. The model defaults to `fal-ai/flux/schnell`.

Result message returned to AI (image_gen.rs:197):
```
Image generated and stored at {webdav_path}. Original fal.ai URL: {image_url}
```

---

## 8. Memory / Context Prompts (sent to AI provider as system messages)

### 8a. Soul memory prefix
**File:** `crate-rockbot/src/memory.rs:230-233`
**Type:** Dynamic — loaded from WebDAV `memory/soul.md`
```
[Core memory — permanent preferences, identity, and notes]
{content from soul.md}
```
Injected as a second system message when soul content is non-empty.

### 8b. Daily summaries header
**File:** `crate-rockbot/src/memory.rs:244-255`
**Type:** Static prefix + dynamic per-summary lines
```
[Recent conversation summaries]
## {date} ({msg_count} messages)
{summary}
...
```
Injected as a system message when daily summaries exist (loaded from `memory/summaries/` on WebDAV).

### 8c. Knowledge context (from WebDAV knowledge index)
**File:** `crate-rockbot/src/memory.rs:237-240`
**Type:** Dynamic — loaded by `AgentHarness::refresh_knowledge_context()` (harness.rs:711) and stored in `MemoryManager.knowledge`
```
{knowledge text from matched entries}
```
Injected as a system message when relevant knowledge entries exist for the room. Fetched on each `process_message` call before building context (harness.rs:139).

---

## 9. Summarize-for-Archive Prompt (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:449-453`
**Role:** `user` (one-shot completion, no tools)
**Purpose:** Generates a short summary of archived messages for daily summaries.
```
Summarize this conversation excerpt in 1-3 concise sentences. Focus on key topics,
decisions, and factual information shared:

<joined message texts, each truncated to 300 chars, max 20 messages>
```

**Fallback (line 472-484):** If AI summarization fails:
```
{messages.len()} messages: {preview of up to 5 message snippets truncated to 80 chars each}
```
**Minimal fallback (line 479-480):** If no previewable messages:
```
{messages.len()} messages archived
```

---

## 10. WebDAV Storage Templates (not AI prompts)

### 10a. Daily summary file (upsert_daily_summary)
**File:** `crate-rockbot/src/harness.rs:355-391`
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
**File:** `crate-rockbot/src/tools/edit_soul.rs:133-134`
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
| 39-47 | — | Console-only: `DM from {sender_name}` / `#{room_name} from {sender_name}` |
| 53 | `!ping` | `pong @{sender_name}` |
| 63 | `!help` | `Commands: !ping, !echo <text>, !help` |

**File:** `crate-rocketchat/src/client.rs`

| Line | Purpose | Template |
|------|---------|----------|
| 38-41 | Code-block reply wrapper | `` ```\n{text}\n``` `` |
| 77 | Bot mention pattern for detection | `@{username}` (constructor of `RocketChatClient`, stored in `bot_name` field) |

---

## Summary Table

| # | What | Where | Sent To | Dynamic? |
|---|------|-------|---------|----------|
| 1 | **System prompt** — defines persona & capabilities | `harness.rs:15-35` | AI provider (`system` role) | Static (with `{name}` template) |
| 2 | **User message template** — wraps chat text | `harness.rs:125` | AI provider (`user` role) | Per-message |
| 3a-h | **Tool descriptions** — teach AI what tools do | 8 files in `tools/` | AI provider (tool definitions) | Static |
| 4 | **Tool param descriptions** — describe JSON fields | 8 files in `tools/` | AI provider (tool schema) | Static |
| 5 | **Default vision prompt** — fallback | `vision.rs:110` | Downstream tool code | Static |
| 6 | **Fallback messages** — error/loop handling | `harness.rs:154-269`, `main.rs:342` | RocketChat user | Partially |
| 7 | **Image gen prompt** — fal.ai API | `fal.rs:110-147` | fal.ai API | Dynamic |
| 8a-b | **Memory/context prompts** — soul, summaries | `memory.rs:230-255` | AI provider (`system` role) | Dynamic |
| 8c | **Knowledge context** — matched entries | `memory.rs:237-240` | AI provider (`system` role) | Dynamic |
| 9 | **Summarize-for-archive** — daily summary | `harness.rs:449-453` | AI provider (one-shot) | Dynamic |
| 10a-b | **WebDAV storage templates** — summaries, soul | `harness.rs:355-391`, `edit_soul.rs:133-134` | WebDAV storage | Dynamic |
| 11 | **Debug binary messages** | `rocketchat/main.rs:39-64`, `client.rs:38-41` | RocketChat / console | Dynamic |

(End of file - total lines may vary)
