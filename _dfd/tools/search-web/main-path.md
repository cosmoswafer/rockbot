# Search Web

## 1. Purpose

Performs internet searches via a configurable search provider. Currently
supports **Exa** (`POST https://api.exa.ai/search`) and **Brave Search**
(`GET https://api.search.brave.com/res/v1/web/search`), returning
token-efficient highlights or page descriptions from search results.

- Upstream: [Configuration Management](../../infra/config/main-path.md) provides `SearchConfig`
  with `provider` selection ("exa" | "brave") and per-provider API keys
- Upstream: [Agent Harness](../../agent/agent-harness/tool-dispatch.md) invokes search as a tool during
  the agent loop, passing a natural-language query
- Downstream: [AI Provider](../../ai/ai-provider/main-path.md) consumes returned context for chat
  completions

## 2. Diagram

```mermaid
flowchart TD
    AGENT[Agent Harness]
    CFG[(SearchConfig)]
    SELECT[SelectProvider]
    BUILD(BuildRequest)
    EXA[Exa Search API]
    BRAVE[Brave Search API]
    PARSE(ParseResults)
    FORMAT(FormatContext)
    AI[AiProvider]

    AGENT -->|"query + search params"| BUILD
    CFG -->|"provider type + api_key"| SELECT
    SELECT -->|"provider selected"| BUILD
    BUILD -->|"POST /search (Exa) or GET /web/search (Brave)"| EXA
    BUILD -->|"GET /web/search"| BRAVE
    EXA -->|"results: title, url, highlights, text"| PARSE
    BRAVE -->|"results: title, url, description"| PARSE
    PARSE -->|"typed results"| FORMAT
    FORMAT -->|"formatted context"| AI
```

## 3. Data Structures

Shared with [structures.md](structures.md).
