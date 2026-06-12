# Order Processing Pipeline — Level 1 DFD Example

This file demonstrates a complete Level 1 DFD with multiple inline Level 2
diagrams (process deep-dives, UI/UX flows, non-functional concerns) and
references to shared cross-cutting diagrams — following the
[dfd-md skill](../../SKILL.md) conventions.

---

### 1. Purpose

Model the data flow through the order processing pipeline — collecting the
order, validating payment, reserving inventory, fulfilling the shipment, and
notifying the customer.

**References:**

- Upstream DFD: [`inventory-management.md`](./inventory-management.md) (stock
  reservation)
- Shared diagrams: [`error-toast.md`](./shared/error-toast.md),
  [`rate-limiting.md`](./shared/rate-limiting.md)
- Stripe API docs: https://stripe.com/docs/api

### 2. Diagram

#### 2a. Happy Flow (Main Success Path)

```mermaid
flowchart TD
    CUSTOMER["Customer"]
    PAYMENT["Payment Gateway"]
    SHIPPING["Shipping Provider"]
    ORDERDB[("Order DB
      PostgreSQL")]
    INVENTORYDB[("Inventory DB
      PostgreSQL")]

    COLLECT("Collect Order
      gather line items + payment method")
    VALIDATE("Validate Order
      check stock + pricing rules")
    CHARGE("Charge Payment
      authorize + capture funds")
    RESERVE("Reserve Inventory
      decrement available stock")
    FULFILL("Fulfill Order
      create shipment + pick items")
    NOTIFY("Notify Customer
      send confirmation email")

    CUSTOMER -->|"line items, shipping address, payment method"| COLLECT
    COLLECT -->|"raw order payload"| VALIDATE
    INVENTORYDB -->|"current stock levels"| VALIDATE
    VALIDATE -->|"validated order with totals"| CHARGE
    CHARGE -->|"charge request"| PAYMENT
    PAYMENT -->|"transaction id, status"| CHARGE
    CHARGE -->|"paid order"| RESERVE
    RESERVE -->|"reserved stock quantities"| INVENTORYDB
    RESERVE -->|"confirmed order"| FULFILL
    ORDERDB -->|"warehouse location"| FULFILL
    FULFILL -->|"shipment request"| SHIPPING
    SHIPPING -->|"tracking number, label url"| FULFILL
    FULFILL -->|"fulfilled order"| ORDERDB
    FULFILL -->|"tracking info"| NOTIFY
    NOTIFY -->|"order confirmation email"| CUSTOMER
```

- **External entities** (`[ ]`): `CUSTOMER`, `PAYMENT`, `SHIPPING`.
- **Data stores** (`[( )]`): `ORDERDB`, `INVENTORYDB` — relational databases.
- **Processes** (`( )`): six sub-processes forming the order pipeline.
- `CHARGE` communicates with the external payment gateway; `FULFILL` talks to
  the shipping provider.

#### 2b. Error Handling & Fallbacks

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
- `ERROR` is detailed in the shared [`error-toast.md`](./shared/error-toast.md)
  diagram.

#### 2c. `Charge Payment` Deep Dive (Process Deep Dive)

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
- See also shared [`rate-limiting.md`](./shared/rate-limiting.md).

#### 2d. `Fulfill Order` Deep Dive (Process Deep Dive)

```mermaid
flowchart TD
    SHIPPING["Shipping Provider"]
    ORDERDB[("Order DB")]
    INVENTORYDB[("Inventory DB")]

    PICK("Pick Items
      locate + scan SKUs")
    PACK("Pack Shipment
      select box + packing material")
    LABEL("Generate Label
      request shipping label")
    SHIP("Hand Off to Carrier
      schedule pickup / drop-off")

    PICK -->|"picked quantities"| PACK
    ORDERDB -->|"order line items"| PICK
    PACK -->|"package weight + dimensions"| LABEL
    LABEL -->|"label request"| SHIPPING
    SHIPPING -->|"tracking number, label pdf"| LABEL
    LABEL -->|"ready package"| SHIP
    SHIP -->|"carrier confirmation"| ORDERDB
    SHIP -->|"deduct from inventory"| INVENTORYDB
```

#### 2e. Order Status Lifecycle (UI/UX Flow)

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

#### 2f. Idempotency & Retry Safety (Non-Functional Concern)

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

### 3. Data Structures

#### `OrderRequest`

| Field               | Type         | Description                         |
| ------------------- | ------------ | ----------------------------------- |
| `idempotency_key`   | `string`     | Client-generated unique key         |
| `customer_id`       | `string`     | Customer identifier                 |
| `line_items`        | `LineItem[]` | Products + quantities + unit prices |
| `shipping_address`  | `Address`    | Delivery destination                |
| `payment_method_id` | `string`     | Tokenized payment method reference  |
| `currency`          | `string`     | ISO 4217 (e.g. `"USD"`)             |

#### `Order`

| Field             | Type                | Description                                                                       |
| ----------------- | ------------------- | --------------------------------------------------------------------------------- |
| `order_id`        | `string`            | System-generated unique identifier                                                |
| `status`          | `enum`              | One of: `pending`, `confirmed`, `processing`, `shipped`, `delivered`, `cancelled` |
| `transaction_id`  | `string`            | Payment gateway transaction reference                                             |
| `tracking_number` | `string` (optional) | Shipping provider tracking code                                                   |
| `total_amount`    | `integer`           | Amount in minor currency units                                                    |
| `created_at`      | `datetime`          | ISO 8601 timestamp                                                                |
| `updated_at`      | `datetime`          | ISO 8601 timestamp                                                                |

#### `ShipmentRequest`

| Field           | Type        | Description                    |
| --------------- | ----------- | ------------------------------ |
| `order_id`      | `string`    | Parent order reference         |
| `origin`        | `Address`   | Warehouse address              |
| `destination`   | `Address`   | Customer shipping address      |
| `packages`      | `Package[]` | Weight + dimensions per box    |
| `service_level` | `string`    | e.g. `"standard"`, `"express"` |
