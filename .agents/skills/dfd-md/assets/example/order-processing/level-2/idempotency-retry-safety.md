# Idempotency & Retry Safety — Level 2 DFD Example

## 1. Purpose

Non-functional safety mechanism guarding the
[order pipeline](../order-pipeline.md) against duplicate charges.

## 2. Diagram

```mermaid
flowchart TD
    ORDERDB[("Order DB")]
    PAYMENT["Payment Gateway"]

    RECEIVE("Receive Request
      attach idempotency key")
    CHECK("Check Key Store
      lookup previous outcome")
    PROCESS("Process Order
      execute pipeline")
    STORE("Store Outcome
      persist result with key")

    RECEIVE -->|"idempotency key"| CHECK
    CHECK -->|"key not found — proceed"| PROCESS
    CHECK -.->|"key found — short-circuit"| STORE
    PROCESS -->|"order result"| STORE
    STORE -->|"outcome + key"| ORDERDB
```

- Every incoming order request carries a client-generated idempotency key.
- `CHECK` short-circuits duplicate requests to the cached outcome, preventing
  double-charges.
- Dashed `-.->` marks the cache-hit path — transparent to the client.
