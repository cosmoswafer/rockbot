# Order Processing System — Context Diagram Example

### 1. Purpose

Model the top-level data flows between the order processing system, its users,
and external dependencies.

### 2. Diagram

```mermaid
flowchart LR
    CUSTOMER["Customer
      web / mobile end-user"]
    S(("Order Processing System
      e-commerce backend"))
    PAYMENT["Payment Gateway
      Stripe / external PSP"]
    SHIPPING["Shipping Provider
      3rd-party logistics API"]
    ADMIN["Admin
      warehouse / support staff"]

    CUSTOMER -->|"order request, payment method"| S
    S -->|"order confirmation, tracking info"| CUSTOMER
    S -->|"charge request"| PAYMENT
    PAYMENT -->|"payment result, transaction id"| S
    S -->|"shipment request, package details"| SHIPPING
    SHIPPING -->|"tracking number, label url"| S
    ADMIN -->|"stock updates, refund approvals"| S
    S -->|"order dashboard, fulfillment queue"| ADMIN
```

- Single system process `(( ))`: the entire order processing backend.
- Four external entities `[ ]`: `CUSTOMER`, `PAYMENT`, `SHIPPING`, `ADMIN`.
- No data stores — those appear at Level 1.
- Every external entity has ≥1 flow to/from the system.
