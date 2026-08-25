# Error Handling & Fallbacks

## 1. Purpose

Level 2 detail for the happy-path diagram in [main-path](../main-path.md): how search failures are handled — missing API key, 401 auth errors, 429/5xx retries with exponential backoff, and empty results fallback.

## 2. Diagram

```mermaid
flowchart TD
    BUILD(BuildRequest)
    API[Search API]
    RETRY(RetryWithBackoff)
    PARSE(ParseResults)
    ERR_KEY[Error: Missing Api Key]
    ERR_AUTH[Error: 401 Unauthorized]
    ERR_RATE[Error: 429 Rate Limited]
    ERR_SVR[Error: 500 Internal]
    ERR_EMPTY[Warning: No Results]
    WARN_FALLBACK(WarnEmptyResults)
    AGENT[Agent Harness]

    BUILD -.->|"missing key"| ERR_KEY
    ERR_KEY -->|"skip search"| AGENT
    API -.->|"401"| ERR_AUTH
    API -.->|"429"| RETRY
    RETRY -.->|"max retries"| ERR_RATE
    API -.->|"500"| RETRY
    RETRY -.->|"all attempts fail"| ERR_SVR
    PARSE -.->|"zero results"| ERR_EMPTY
    ERR_EMPTY -->|"return empty context"| WARN_FALLBACK
    WARN_FALLBACK -->|"empty search results"| AGENT
```

Note: Exa HTTP errors (429, 5xx) are retried with up to 3 attempts using exponential backoff before returning a failure.

Note: 401 errors for Exa return a specific message: `"Exa search failed: invalid API key (401). Check your [search.exa] config."`

Note: 401 errors for Brave return a specific message: `"Brave Search failed: invalid API key (401). Check your [search.brave] config."`
