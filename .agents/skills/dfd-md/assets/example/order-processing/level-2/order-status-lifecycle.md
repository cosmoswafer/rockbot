# Order Status Lifecycle — Level 2 DFD Example

## 1. Purpose

User-facing order states and their transitions — a UI/UX flow, not a
sub-process data path.

## 2. Diagram

```mermaid
flowchart TD
    CUSTOMER["Customer"]

    PENDING("Pending
      order received, awaiting payment")
    CONFIRMED("Confirmed
      payment captured, stock reserved")
    PROCESSING("Processing
      warehouse picking + packing")
    SHIPPED("Shipped
      handed to carrier, tracking live")
    DELIVERED("Delivered
      carrier confirms drop-off")
    CANCELLED("Cancelled
      refund issued, stock released")

    PENDING -->|"payment succeeded"| CONFIRMED
    PENDING -->|"payment failed / timeout"| CANCELLED
    CONFIRMED -->|"fulfillment started"| PROCESSING
    CONFIRMED -->|"customer cancels"| CANCELLED
    PROCESSING -->|"label generated"| SHIPPED
    SHIPPED -->|"carrier delivery scan"| DELIVERED
    CANCELLED -.->|"admin reopens"| PENDING
```

- Six UI states cover the entire order lifecycle.
- Dashed `-.->` from `CANCELLED` to `PENDING` represents an admin-only re-open
  path — not a standard user flow.
