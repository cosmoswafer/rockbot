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
    User[User]
    RC[RocketChat Server]
    AI[AI Provider]
    NC[NextCloud WebDAV]
    Exa[Exa Search API]
    Web[Web Page]
    ImgGen[Image Generation API]
    Bot(("RockBot"))

    User -->|"chat message"| RC
    RC -->|"DM / @mention event"| Bot
    Bot -->|"bot reply"| RC
    RC -->|"bot reply"| User

    Bot -->|"chat completion request"| AI
    AI -->|"completion + tool calls"| Bot

    Bot -->|"PROPFIND / GET / PUT"| NC
    NC -->|"config, archives, images"| Bot

    Bot -->|"search query"| Exa
    Exa -->|"search results"| Bot

    Bot -->|"HTTP GET"| Web
    Web -->|"page HTML"| Bot

    Bot -->|"generation prompt"| ImgGen
    ImgGen -->|"generated image"| Bot
```
