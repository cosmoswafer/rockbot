# Restock — Level 1 DFD Example

## 1. Purpose

Admin-driven data path adding new inventory from a supplier delivery.

## 2. Diagram

```mermaid
flowchart TD
    ADMIN["Admin"]
    INVENTORYDB[("Inventory DB
      PostgreSQL")]

    RESTOCK("Restock
      add new inventory from supplier")

    ADMIN -->|"supplier delivery manifest"| RESTOCK
    RESTOCK -->|"added stock levels"| INVENTORYDB
```

## 3. Data Structures

- `InventoryRecord` — see [`structures.md`](structures.md)
