# Order Pipeline — Level 1 DFD Example

## 1. Purpose

Model the happy-path data flow through the order pipeline — collecting the
order, validating payment, reserving inventory, fulfilling the shipment, and
notifying the customer.

**References:**

- Upstream DFD: [`../inventory-management/reserve-stock.md`](../inventory-management/reserve-stock.md)
  (stock reservation, `InventoryRecord`)
- Shared diagrams: [`../shared/error-toast.md`](../shared/error-toast.md),
  [`../shared/rate-limiting.md`](../shared/rate-limiting.md)
- Stripe API docs: https://stripe.com/docs/api

## 2. Diagram

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

## 3. Data Structures

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
