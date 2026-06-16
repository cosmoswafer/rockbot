# RockBot — Context Diagram (Level 0)

## 1. Purpose

Single-process view of RockBot: a Rust-based AI agent that connects to a
messaging platform (RocketChat **or** Matrix), answers DMs and @mentions via
configurable AI providers (cloud or local llama.cpp), executes agentic tools
(web search, URL fetch, vision, image generation), and persists all state to a
NextCloud WebDAV server — never touching local disk.

The messaging platform is selected at startup via `[platform]` config. Only
one platform is active per process — RocketChat (DDP over WebSocket) or Matrix
(via matrix-rust-sdk sync). Both produce the same `IncomingMessage` type.

## 2. Diagram

```mermaid
flowchart LR
    RocketChat[RocketChat Server]
    Matrix[Matrix Homeserver]
    AIProvider[AI Provider<br/>OpenRouter / DeepSeek / llama.cpp]
    NextCloud[NextCloud WebDAV]
    ExaSearch[Exa Search API]
    WebPage[Web Page]
    ImageGen[Image Generation<br/>OpenRouter / fal.ai]
    Bot(("RockBot"))

    RocketChat -.->|"incoming message event<br/>(if platform = rocketchat)"| Bot
    Bot -.->|"bot reply text"| RocketChat

    Matrix -.->|"sync event<br/>(if platform = matrix)"| Bot
    Bot -.->|"bot reply text"| Matrix

    Bot -->|"chat completion request"| AIProvider
    AIProvider -->|"completion result + tool calls"| Bot

    Bot -->|"file read/write/list request"| NextCloud
    NextCloud -->|"config, archives, images"| Bot

    Bot -->|"search query"| ExaSearch
    ExaSearch -->|"search results"| Bot

    Bot -->|"http GET request"| WebPage
    WebPage -->|"page html"| Bot

    Bot -->|"image generation prompt"| ImageGen
    ImageGen -->|"image bytes"| Bot
    Bot -->|"upload + create share"| NextCloud
    NextCloud -->|"share URL"| Bot
```

Only one of RocketChat / Matrix is connected per process. The inactive
platform's edges are dashed to indicate they are configuration-selected,
not simultaneously active.
