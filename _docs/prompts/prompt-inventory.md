# Prompt Inventory — rockbot

All prompts and prompt-adjacent strings in the Rust codebase, organized by what they do.

---

## 1. System Prompt (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:13-22`
**Constant:** `DEFAULT_SYSTEM_PROMPT`
**Sent to:** AI provider as the `system` role message in `ChatRequest.messages`
**Used via:** `build_system_prompt()` (line 214) → `MemoryManager::build_context()` (line 108) → prepended as first message in context

```
You are RockBot, a helpful AI assistant running on a RocketChat server.
You respond to DMs and @mentions concisely and helpfully.
When you need information from the web, use the web_search tool.
When you need to fetch a URL, use web_fetch.
When you need to analyze an image, use the vision tool.
When you need to read, write, list, or manage files on remote storage, use the webdav tool.
Answer in the same language as the user.
Keep responses clear and to the point.
```

---

## 2. User Message Template (sent to AI provider)

**File:** `crate-rockbot/src/harness.rs:97`
**Template:** `format!("{}: {}", sender_name, clean_text)`
**Role:** `user`
**Purpose:** Wraps every incoming RocketChat message as `"SenderName: message text"` before appending to history. Preserves sender identity in group chats.

Example output: `"Alice: what's the weather in Tokyo?"`

---

## 3. Tool Descriptions (sent to AI provider in tool definitions)

### 3a. `vision`
**File:** `crate-rockbot/src/tools/vision.rs:77`
```
Download and describe an image. Provide an image URL and an optional prompt.
```

### 3b. `web_search`
**File:** `crate-rockbot/src/tools/web_search.rs:100`
```
Search the web using Exa. Returns ranked results with titles, URLs, and snippets.
```

### 3c. `web_fetch`
**File:** `crate-rockbot/src/tools/web_fetch.rs:423-426`
```
Fetch content from a URL. Supports three output formats: json (structured with metadata),
markdown (HTML converted to markdown for AI consumption), raw (unmodified response text).
Optionally cross-verifies content via web search when verify=true.
```

### 3d. `webdav`
**File:** `crate-rockbot/src/tools/webdav.rs:131-136`
```
Manage files on remote WebDAV storage (NextCloud).
Actions: read (get file content), write (create/overwrite a file),
list (list directory contents), mkdir (create directory tree),
delete (remove file/directory), exists (check if path exists).
All paths are scoped to the given room_id.
```

---

## 4. Tool Parameter Descriptions (sent to AI provider in tool schema)

| File | Line | Parameter | Description |
|------|------|-----------|-------------|
| `vision.rs` | 90 | `prompt` | `Optional description of what to look for in the image` |
| `web_search.rs` | 108 | `query` | (no explicit description string — name is self-describing) |

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
| 119 | Max agent iterations exceeded | `I'm sorry, I got stuck in a loop. Could you rephrase your request?` |
| 178 | AI returned empty text (stored in history) | `(no response)` |
| 185 | AI returned empty text (user-facing) | `I processed your request but received an empty response.` |
| 192 | AI returned no text at all | `I received a response but it was empty. Please try again.` |
| 199 | AI provider error | `I encountered an error: {e}. Please try again.` |

---

## 7. Image Generation Prompt (sent to Replicate API)

**File:** `crate-rockbot/src/provider/replicate.rs:55-64`

The user-provided `prompt` string is placed in the JSON body:
```json
{
  "version": "<model>",
  "input": {
    "prompt": "<user provided>",
    "output_format": "png",
    "aspect_ratio": "1:1",
    "output_quality": 80
  }
}
```
The prompt content is dynamic (supplied by the AI model's tool call). Fixed parameters (`output_format`, `aspect_ratio`, `output_quality`) are also part of the Replicate API request.

---

## 8. Conversation Archive Header (not an AI prompt)

**File:** `crate-rockbot/src/harness.rs:270-296`
```
# Conversation Archive
```
Used as a markdown header when archiving conversation history to WebDAV. Not sent to any AI model.

---

## Summary Table

| # | What | Where | Sent To | Dynamic? |
|---|------|-------|---------|----------|
| 1 | **System prompt** — defines persona & capabilities | `harness.rs:13` | AI provider (`system` role) | Static |
| 2 | **User message template** — wraps chat text | `harness.rs:97` | AI provider (`user` role) | Per-message |
| 3a-d | **Tool descriptions** — teach AI what tools do | 4 files in `tools/` | AI provider (tool definitions) | Static |
| 4 | **Tool param descriptions** — describe JSON fields | `vision.rs:90` | AI provider (tool schema) | Static |
| 5 | **Default vision prompt** — fallback | `vision.rs:110` | Downstream tool code | Static |
| 6 | **Fallback messages** — error/loop handling | `harness.rs:119-199` | RocketChat user | Partially |
| 7 | **Image gen prompt** — Replicate API | `replicate.rs:59` | Replicate API | Dynamic |
