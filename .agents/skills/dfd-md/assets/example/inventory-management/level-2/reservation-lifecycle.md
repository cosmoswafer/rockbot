# Reservation Lifecycle — Level 2 DFD Example

## 1. Purpose

UI-visible states of a `Reservation` (see
[`structures.md`](../structures.md)) and their transitions.

## 2. Diagram

```mermaid
flowchart TD
    AVAILABLE("Available
      stock ready for orders")
    RESERVED("Reserved
      held for pending checkout")
    CONSUMED("Consumed
      deducted on order fulfillment")
    RELEASED("Released
      returned to available pool")
    EXPIRED("Expired
      reservation timeout > 15 min")

    AVAILABLE -->|"customer adds to cart"| RESERVED
    RESERVED -->|"order confirmed + paid"| CONSUMED
    RESERVED -->|"checkout abandoned / cancelled"| RELEASED
    RESERVED -->|"timer expires"| EXPIRED
    EXPIRED -->|"auto-release"| AVAILABLE
    RELEASED -->|"return to pool"| AVAILABLE
```

- Reservations time out after 15 minutes to prevent stock hoarding.
- `EXPIRED` auto-releases back to `AVAILABLE` without user interaction.
