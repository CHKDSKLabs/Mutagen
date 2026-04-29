# Implied Systems Contract

## Orders to Billing

- [ISC-001] `OrderCreated` must include `order_id`, `customer_id`, and `total_cents`.
- Detection: contract tests fail if any required field disappears.
