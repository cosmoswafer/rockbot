# Provider Selection Deep Dive

## 1. Purpose

Level 2 detail for the happy-path diagram in [main-path](../main-path.md): how `SearchConfig` selects the concrete `SearchProvider` (Exa or Brave) and registers the tool, or leaves it unregistered when no provider is configured.

## 2. Diagram

```mermaid
flowchart TD
    CFG[(SearchConfig)]
    SELECT{Select Provider}
    EXA(ExaSearchProvider)
    BRAVE(BraveSearchProvider)
    NO_PROV[No Provider Configured]
    TOOL[WebSearchTool]

    CFG -->|"provider = exa, apikey present"| SELECT
    CFG -->|"provider = brave, apikey present"| SELECT
    CFG -->|"no apikey"| SELECT
    SELECT -->|"exa"| EXA
    SELECT -->|"brave"| BRAVE
    SELECT -->|"none"| NO_PROV
    EXA -->|"Box<dyn SearchProvider>"| TOOL
    BRAVE -->|"Box<dyn SearchProvider>"| TOOL
    NO_PROV -->|"tool not registered"| TOOL
```
