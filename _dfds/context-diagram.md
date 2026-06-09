# RockBot — Context Diagram (Level 0)

## 1. Purpose

Single-process view of RockBot: a Rust-based AI agent that connects to a
self-hosted RocketChat server, answers DMs and @mentions via configurable AI
providers, executes agentic tools (web search, URL fetch, vision, image
generation), and persists all state to a NextCloud WebDAV server — never
touching local disk.

## 2. Diagram

```mermaid
flowchart LR
    RocketChat[RocketChat Server]
    AIProvider[AI Provider]
    NextCloud[NextCloud WebDAV]
    ExaSearch[Exa Search API]
    WebPage[Web Page]
    ImageGen[Image Generation API]
    Bot(("RockBot"))

    RocketChat -->|"incoming message event"| Bot
    Bot -->|"bot reply text"| RocketChat

    Bot -->|"chat completion request"| AIProvider
    AIProvider -->|"completion result + tool calls"| Bot

    Bot -->|"file read/write/list request"| NextCloud
    NextCloud -->|"config, archives, images"| Bot

    Bot -->|"search query"| ExaSearch
    ExaSearch -->|"search results"| Bot

    Bot -->|"http GET request"| WebPage
    WebPage -->|"page html"| Bot

    Bot -->|"image generation prompt"| ImageGen
    ImageGen -->|"generated image bytes"| Bot
```
