# Charge Payment Deep Dive — Level 2 DFD Example

## 1. Purpose

Internal transformation logic inside the
[`Charge Payment` process](../order-pipeline.md) — too complex for Level 1.

## 2. Diagram

```mermaid
flowchart TD
    PAYMENT["Payment Gateway"]
    ORDERDB[("Order DB")]

    TOKENIZE("Tokenize Payment Method
      replace raw card with token")
    AUTHORIZE("Authorize Hold
      reserve funds on card")
    CAPTURE("Capture Funds
      finalize the charge")
    IDEMPOTENCY("Idempotency Check
      prevent duplicate charges")

    TOKENIZE -->|"payment token"| AUTHORIZE
    AUTHORIZE -->|"authorization code"| CAPTURE
    CAPTURE -->|"transaction id"| ORDERDB
    CAPTURE -->|"idempotency key"| IDEMPOTENCY
    IDEMPOTENCY -.->|"duplicate detected"| CAPTURE
```

- `TOKENIZE` replaces raw PCI-sensitive card data with a gateway token.
- The dashed `-.->` from `IDEMPOTENCY` back to `CAPTURE` represents a silent
  short-circuit — if the same idempotency key is replayed, the previous result
  is returned instead of charging again.
