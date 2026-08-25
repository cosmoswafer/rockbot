# WebDAV — Write-With-Fallback Deep Dive

## 1. Purpose

Deep dive into `write_file_with_fallback()`: try a PUT with the
`X-NC-WebDAV-AutoMkcol: 1` header; on 404, extract the parent path, create
all parent directories via MKCOL, then retry the PUT. See [Main Path](../main-path.md)
for the happy flow.

## 2. Diagram

```mermaid
flowchart TD
    W(WriteInitiated)
    AMC[Try AutoMkcol PUT]
    HTTP[(HTTP Client)]
    NC[(NextCloud)]
    CHECK{Status?}
    OK(Success)
    IS_404{Is 404?}
    PARENT(ExtractParentPath)
    MKCOL_ALL(MkcolAll parent dirs)
    PUT_RETRY(PUT without mkcol header)
    FAIL(Fail)

    W --> AMC
    AMC --> HTTP
    HTTP --> NC
    NC --> CHECK
    CHECK -->|"200/201/204"| OK
    CHECK -.->|"other status"| IS_404
    IS_404 -.->|"yes"| PARENT
    IS_404 -.->|"no"| FAIL
    PARENT -.-> MKCOL_ALL
    MKCOL_ALL -.-> PUT_RETRY
    PUT_RETRY -.-> HTTP
```
