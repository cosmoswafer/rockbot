# Conditional Tool Registration

Tools registered at startup are gated on config availability.
If the required config is absent, the tool is not registered at all
(not exposed to the LLM, cannot be called).

## Gating Rules

| Tool | Gate | Rationale |
|------|------|-----------|
| `web_search` | `[tools.exa] api_key` non-empty | Requires Exa API key; without it the tool always errors |
| `web_fetch` | Always registered (variant adapted) | Core HTTP fetch works without Exa/WebDAV; variants enable Exa verify + WebDAV save |
| `vision` | Always registered | Only depends on chat provider's vision support |
| `webdav` | `[webdav]` config present | Requires NextCloud WebDAV endpoint |
| `edit_soul` | `[webdav]` config present | Writes `soul.md` to WebDAV |
| `save_knowledge` | `[webdav]` config present | Writes knowledge `.md` files to WebDAV |
| `forget_knowledge` | `[webdav]` config present | Deletes knowledge `.md` files from WebDAV |
| `recall_knowledge` | `[webdav]` config present | Lists knowledge `.md` files from WebDAV |
| `calendar` | `[webdav]` config present | Creates/manages per-room `.ics` calendars on WebDAV |
| `image_gen` | `[webdav]` config present **and** matching `[[image_providers]]` entry with resolvable t2i + edit models | Requires WebDAV for persistence + image provider for generation |
| `compress_memory` | Always registered | Injected separately into harness; self-contained |

## Boot Logging

Each registration (or skip) emits an `INFO`-level about-info log.
The final summary (`Registered N tools: [...]`) excludes skipped tools.
