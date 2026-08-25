# Happy Flow (Main Success Path)

## 1. Purpose

The agent's chat request is built into a provider request, formatted per
provider (OpenRouter, DeepSeek, or llama.cpp), sent as an HTTP POST to the
provider API, parsed, and returned to the agent as a `CompletionResult`.

References: [Shared structures](structures.md)

## 2. Diagram

```mermaid
flowchart TD
    AGENT[Agent]
    BUILD(BuildContext)
    FORMAT(FormatProviderRequest)
    OPENROUTER(OpenRouterProvider)
    DEEPSEEK(DeepSeekProvider)
    LLAMA(LlamaCppProvider)
    HTTP(SendHttpRequest)
    PARSE(ParseResponse)
    PROVIDER_API[Provider HTTP API]

    AGENT -->|"chat request"| BUILD
    BUILD -->|"provider request"| FORMAT
    FORMAT -->|"openrouter request"| OPENROUTER
    FORMAT -->|"deepseek request"| DEEPSEEK
    FORMAT -->|"llama.cpp request<br/>(native tools field)"| LLAMA
    OPENROUTER -->|"http request"| HTTP
    DEEPSEEK -->|"http request"| HTTP
    LLAMA -->|"http request<br/>(Bearer auth header when key set)"| HTTP
    HTTP -->|"http post"| PROVIDER_API
    PROVIDER_API -->|"json response body"| HTTP
    HTTP -->|"raw bytes"| PARSE
    PARSE -->|"completion result"| AGENT
```

## 3. Data Structures

See [structures.md](structures.md).
