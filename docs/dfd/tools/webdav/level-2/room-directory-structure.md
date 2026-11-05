# WebDAV — Room Directory Structure

## 1. Purpose

Each room (channel or DM) has three subdirectories: `memory/`, `images/`, and
`workspace/`. A shared `config/` directory holds backups.

## 2. Diagram

```mermaid
flowchart TD
    ROOT[(WebDAV Root)]
    CH_ATOM[(r-atomkb)]
    CH_PROJ[(r-project-x)]
    DM_SARU[(d-saru)]
    MEM_ATOM[(r-atomkb/memory)]
    IMG_ATOM[(r-atomkb/images)]
    WSP_ATOM[(r-atomkb/workspace)]
    MEM_PROJ[(r-project-x/memory)]
    IMG_PROJ[(r-project-x/images)]
    WSP_PROJ[(r-project-x/workspace)]
    MEM_SARU[(d-saru/memory)]
    IMG_SARU[(d-saru/images)]
    WSP_SARU[(d-saru/workspace)]
    CFG_DIR[(config/)]

    ROOT --> CH_ATOM
    ROOT --> CH_PROJ
    ROOT --> DM_SARU
    ROOT --> CFG_DIR
    CH_ATOM --> MEM_ATOM
    CH_ATOM --> IMG_ATOM
    CH_ATOM --> WSP_ATOM
    CH_PROJ --> MEM_PROJ
    CH_PROJ --> IMG_PROJ
    CH_PROJ --> WSP_PROJ
    DM_SARU --> MEM_SARU
    DM_SARU --> IMG_SARU
    DM_SARU --> WSP_SARU
```

> **Note:** Calendars do **not** live under the WebDAV file storage root.
> Calendar data resides in a separate CalDAV space at
> `/remote.php/dav/calendars/{user}/{cal-name}/` (see [Calendar](../../calendar/main-path.md)).
