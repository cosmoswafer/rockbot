# Audit Stock — Level 1 DFD Example

## 1. Purpose

Admin-driven reconciliation of physical counts against system-reported stock.

## 2. Diagram

```mermaid
flowchart TD
    ADMIN["Admin"]
    INVENTORYDB[("Inventory DB
      PostgreSQL")]

    AUDIT("Audit Stock
      reconcile physical vs. system counts")

    ADMIN -->|"physical count data"| AUDIT
    INVENTORYDB -->|"system-reported stock"| AUDIT
    AUDIT -->|"reconciled stock levels"| INVENTORYDB
    AUDIT -->|"discrepancy report"| ADMIN
```

## 3. Data Structures

- `InventoryRecord` — see [`structures.md`](structures.md)
