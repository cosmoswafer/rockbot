# Verify Deep Dive (Double-Check)

## 1. Purpose

Level 2 detail for the happy-path diagram in [main-path](../main-path.md): how the optional Exa cross-verification flow extracts the fetched page title, queries the Exa search API, and merges related sources into the output for fact-checking.

## 2. Diagram

```mermaid
flowchart TD
    FETCH(FetchUrl)
    MODE(SelectOutputMode)
    EXA[Exa Search API]
    PARSE_TITLE(ExtractPageTitle)
    SEARCH(SearchRelated)
    MERGE(MergeResults)
    OUTPUT(OutputWithSources)

    FETCH -->|"response html"| PARSE_TITLE
    PARSE_TITLE -->|"page title / domain"| SEARCH
    SEARCH -->|"POST /search (query=title)"| EXA
    EXA -->|"related results"| MERGE
    FETCH -->|"primary content"| MERGE
    MERGE -->|"content + related sources"| OUTPUT
    OUTPUT -->|"verified output"| MODE
```

When `verify` is `true` and the tool holds a valid Exa API key, the fetched
page title is extracted and used as a query to the Exa search API. The resulting
related sources are bundled alongside the primary content, giving the AI provider
cross-referenced information for fact-checking.
