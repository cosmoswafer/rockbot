# Content Mode Selection (Exa-specific)

## 1. Purpose

Level 2 detail for the happy-path diagram in [main-path](../main-path.md): how query complexity selects the Exa content mode (`highlights`, `text`, `deep`) and maps it onto `BuildRequest`.

## 2. Diagram

```mermaid
flowchart TD
    AGENT[Agent Harness]
    SELECT(SelectContentMode)
    HIGHLIGHTS[(Highlights Mode)]
    TEXT[(Text Mode)]
    DEEP[(Deep Mode)]

    AGENT -->|"query complexity"| SELECT
    SELECT -->|"simple factual query"| HIGHLIGHTS
    SELECT -->|"needs full page context"| TEXT
    SELECT -->|"multi-step research"| DEEP
    HIGHLIGHTS -->|"contents: {highlights: true}"| BUILD[BuildRequest]
    TEXT -->|"contents: {text: {maxCharacters: 15000}}"| BUILD
    DEEP -->|"type: deep"| BUILD
```

`highlights` mode is the default — it returns excerpts relevant to the query.
`text` mode returns full page content up to 15K characters. `deep` mode
enables comprehensive search with up to 15K character content.
Brave returns `description` for all results regardless of content mode.
