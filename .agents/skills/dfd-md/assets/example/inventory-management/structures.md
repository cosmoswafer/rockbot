# Inventory Management — Shared Structures

## 1. Overview

Checking, reserving, releasing, and updating inventory stock levels —
supporting both the order pipeline and admin operations. See the dataflow
files in this directory (`reserve-stock.md`, `restock.md`, `audit.md`) and
their Level 2 diagrams in `level-2/`.

## 3. Data Structures

#### `InventoryRecord`

| Field           | Type       | Description                   |
| --------------- | ---------- | ----------------------------- |
| `sku`           | `string`   | Stock-keeping unit identifier |
| `warehouse_id`  | `string`   | Warehouse location code       |
| `available_qty` | `integer`  | Unreserved stock count        |
| `reserved_qty`  | `integer`  | Stock held for pending orders |
| `threshold`     | `integer`  | Low-stock alert trigger level |
| `updated_at`    | `datetime` | Last mutation timestamp       |

#### `Reservation`

| Field            | Type       | Description                                 |
| ---------------- | ---------- | ------------------------------------------- |
| `reservation_id` | `string`   | Unique reservation identifier               |
| `order_id`       | `string`   | Parent order (nullable for cart holds)      |
| `sku`            | `string`   | Reserved SKU                                |
| `quantity`       | `integer`  | Reserved count                              |
| `expires_at`     | `datetime` | Auto-release deadline (15 min TTL)          |
| `status`         | `enum`     | `active`, `consumed`, `released`, `expired` |
