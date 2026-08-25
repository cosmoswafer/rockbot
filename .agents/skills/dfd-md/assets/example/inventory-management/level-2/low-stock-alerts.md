# Low Stock Alerts — Level 2 DFD Example

## 1. Purpose

Non-functional monitoring concern: compare stock levels against configured
thresholds and trigger reorder suggestions.

## 2. Diagram

```mermaid
flowchart TD
    INVENTORYDB[("Inventory DB")]
    ADMIN["Admin"]

    MONITOR("Monitor Thresholds
      compare vs. configured minimums")
    ALERT("Trigger Alert
      push notification + email")
    REORDER("Generate Reorder
      suggest supplier purchase order")

    INVENTORYDB -->|"sku, qty, threshold"| MONITOR
    MONITOR -->|"below threshold"| ALERT
    ALERT -->|"low-stock notification"| ADMIN
    ADMIN -->|"approve reorder"| REORDER
    REORDER -->|"suggested po line items"| ADMIN
```

- `MONITOR` runs periodically (cron) and compares stock levels against
  configured thresholds per SKU.
- `ALERT` pushes to admin dashboard and optionally sends email/Slack.
- `REORDER` suggests purchase order line items based on historical demand, but
  requires admin approval.
