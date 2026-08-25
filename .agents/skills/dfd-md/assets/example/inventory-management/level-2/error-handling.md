# Error Handling & Fallbacks — Level 2 DFD Example

## 1. Purpose

Exceptional paths diverging from the
[reserve-stock](../reserve-stock.md) and [restock](../restock.md) data paths —
stock shortages, data integrity issues, and the compensating release on
cancellation. The user-facing `ERROR` surface is shared:
see [`../../shared/error-toast.md`](../../shared/error-toast.md).

## 2. Diagram

```mermaid
flowchart TD
    ADMIN["Admin"]
    ERROR("Error Handler
      see shared/error-toast.md")

    CHECK("Check Stock")
    RESERVE("Reserve Items")
    RESTOCK("Restock")
    AUDIT("Audit Stock")
    RELEASE("Release Items
      return stock on cancellation")

    CHECK -->|"sku not found"| ERROR
    RESERVE -->|"insufficient stock"| ERROR
    RESTOCK -->|"invalid supplier reference"| ERROR
    AUDIT -->|"discrepancy > threshold"| ERROR
    RESERVE -->|"checkout abandoned / cancelled"| RELEASE
    RELEASE -->|"released quantity"| ERROR
    ERROR -->|"error toast / alert"| ADMIN
```

- Stock shortages and data integrity issues flow to `ERROR` and surface to
  `ADMIN`.
- `RELEASE` is the compensating action triggered by order cancellations or
  payment failures.
- `ERROR` is the shared [`error-toast.md`](../../shared/error-toast.md) component.
