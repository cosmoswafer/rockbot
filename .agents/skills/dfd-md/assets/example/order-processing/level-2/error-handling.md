# Error Handling & Fallbacks — Level 2 DFD Example

## 1. Purpose

Exceptional paths that diverge from the
[order pipeline](../order-pipeline.md) happy path — failures, compensating
refunds, and admin escalation. The user-visible `ERROR` surface is shared:
see [`../shared/error-toast.md`](../../shared/error-toast.md).

## 2. Diagram

```mermaid
flowchart TD
    CUSTOMER["Customer"]
    ADMIN["Admin"]

    VALIDATE("Validate Order")
    CHARGE("Charge Payment")
    RESERVE("Reserve Inventory")
    FULFILL("Fulfill Order")
    ERROR("Error Handler
      see shared/error-toast.md")
    REFUND("Issue Refund
      reverse captured payment")

    VALIDATE -->|"out of stock / invalid address"| ERROR
    CHARGE -->|"payment declined / timeout"| ERROR
    CHARGE -->|"charge succeeded, reservation failed"| REFUND
    RESERVE -->|"insufficient stock after charge"| REFUND
    FULFILL -->|"shipping provider error"| ERROR
    ERROR -->|"error toast / modal"| CUSTOMER
    REFUND -->|"refund confirmation"| CUSTOMER
    FULFILL -->|"fulfillment stalled > 24h"| ADMIN
```

- Payment failures flow to `ERROR` and surface to the customer.
- `REFUND` handles the compensating action when payment succeeds but downstream
  steps fail.
- Stalled fulfillments escalate to `ADMIN` for manual intervention.
- Rate limiting is documented in the shared
  [`rate-limiting.md`](../../shared/rate-limiting.md) diagram.
