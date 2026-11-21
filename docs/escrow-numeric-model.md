# Escrow Numeric Model

This contract uses Soroban host values and Rust integer types directly. It does not emulate EVM integer wrapping, fixed-point decimals, or token-specific decimal rules.

## Amounts: `i128`

- Funding amounts, targets, contributions, dust-sweep amounts, and collateral metadata amounts are stored as signed `i128` values.
- State-changing entrypoints that accept funding-like amounts require strictly positive values before storage updates.
- Token amounts must be passed in the token's smallest unit. The escrow contract does not read token decimals to rescale user-facing amounts.
- `funded_amount` is accumulated with `checked_add`. If `funded_amount + amount` exceeds `i128::MAX`, the contract panics with `funded_amount overflow` and the Soroban invocation aborts.
- Per-investor `InvestorContribution(Address)` is accumulated with `checked_add`. If `prev_contribution + amount` exceeds `i128::MAX`, the contract panics with `investor contribution overflow` and the Soroban invocation aborts.
- The contract does not saturate, clamp, or intentionally wrap funding totals.

### Init amount upper bound: `MAX_INVOICE_AMOUNT`

`StarfundEscrow::init` rejects `amount > MAX_INVOICE_AMOUNT` with [`EscrowError::AmountExceedsMax`](../escrow/src/lib.rs) (code 14) to prevent overflow in settlement-time payout arithmetic. This is a **constructor-time guard** — no valid init can produce an escrow where `compute_investor_payout` overflows.

**Value:** `MAX_INVOICE_AMOUNT = (1 << 63) - 1 = 9_223_372_036_854_775_807` (i.e. `floor(√(i128::MAX / 2))`).

**Derivation** (see the constant's doc comment in [`escrow/src/lib.rs`](../escrow/src/lib.rs)):

```text
coupon       = total_principal × yield_bps / 10_000  (floor)   (1)
settle_pool  = total_principal + coupon                        (2)
gross_payout = contribution × settle_pool / total_principal    (3)
```

The tightest constraint is step (3): with worst-case `yield_bps = 10_000` and a single investor (`contribution = total_principal`), the intermediate product is `total_principal × 2 × total_principal = 2 × total_principal²`. Requiring this to stay within `i128` yields `total_principal ≤ floor(√(i128::MAX / 2)) = 2⁶³ − 1`. This is stricter than both the step (1) bound (`i128::MAX / 10_000`) and the step (2) bound (`i128::MAX / 2`).

**Tests:**
- `test_cost_baseline_init_max_amount` — accepting exactly `MAX_INVOICE_AMOUNT`
- `test_init_amount_exceeds_max_rejected` — rejecting `MAX_INVOICE_AMOUNT + 1` with `AmountExceedsMax`
- `test_max_bound_funded_escrow_compute_investor_payout_no_overflow` — full funding + settlement with `yield_bps = 10_000` at the bound

## Commitment locks: `u64`

- Ledger timestamps and lock durations use `u64` seconds from `Env::ledger().timestamp()`.
- `fund_with_commitment` stores `InvestorClaimNotBefore` as `now + committed_lock_secs` when the commitment is non-zero.
- That addition uses `checked_add`. If the result would exceed `u64::MAX`, the contract panics with `investor claim time overflow` and the Soroban invocation aborts.
- A zero commitment stores `0`, meaning no additional investor claim-time gate.
- Boundary values are inclusive: a timestamp plus commitment that equals `u64::MAX` is representable; only values above `u64::MAX` fail.

## Funding invariants (property-based)

This contract’s funding accounting and state transitions are intended to obey these invariants for all orderings of `fund` / `fund_with_commitment` calls.

- **Conservation (principal accounting):** while the escrow is open, `escrow.funded_amount` must equal the sum of every investor’s stored `get_contribution(addr)`.
- **Unique funder count:** `get_unique_funder_count()` must equal the number of distinct investor addresses whose `get_contribution(addr) > 0`.
- **Cap enforcement (never exceeded):**
  - When `max_per_investor` is configured, each investor’s running contribution must never exceed the configured cap.
  - When `max_unique_investors` is configured, the contract must never allow more distinct funders than the configured cap.
- **Status transition:** `escrow.status` must flip from `0` (open) to `1` (funded) **exactly at the first call** where `funded_amount >= funding_target` becomes true.
- **FundingCloseSnapshot semantics:** on the funded transition, `FundingCloseSnapshot` is written once with `total_principal == escrow.funded_amount` (including over-funding), and it must remain immutable across later reads.

These invariants are validated with randomized property tests in `escrow/src/tests/properties.rs`.

## Integration Guidance

- Off-chain callers should validate amount and lock-duration inputs before submitting transactions, especially when simulating near integer limits.
- Risk and accounting systems should use integer arithmetic for base-unit amounts and rational math for pro-rata ratios; avoid floating-point rounding when reconciling on-chain state.
- Maturity and claim-lock checks are ledger-time checks, not wall-clock oracle checks.
- Unsupported token economics remain out of scope. Fee-on-transfer, rebasing, malicious, or callback-heavy tokens are covered separately in [`escrow/src/external_calls.rs`](../escrow/src/external_calls.rs) and [`ESCROW_TOKEN_INTEGRATION_CHECKLIST.md`](ESCROW_TOKEN_INTEGRATION_CHECKLIST.md).

## Refund conservation (cancelled escrows)

In status **cancelled** (4), the following invariants hold for all refund orderings:

- Each `refund(investor)` returns at most that investor's recorded contribution.
- `DistributedPrincipal` increases atomically per refund and never exceeds `funded_amount`.
- Once every investor has refunded, `DistributedPrincipal == funded_amount`.
- Double-refund is impossible: contribution is zeroed before the token transfer.

These properties are validated in `escrow/src/tests/properties.rs` (`prop_refund_conservation_never_exceeds_funded_principal`).

## Worked payout examples

`calculate_investor_payout` (see [`escrow/src/lib.rs`](../escrow/src/lib.rs)) computes an investor's settlement payout with integer arithmetic only, in this order:

```text
coupon       = total_principal × yield_bps / 10_000   (floor division)   (1)
settle_pool  = total_principal + coupon                                  (2)
gross_payout = contribution × settle_pool / total_principal (floor)      (3)
```

Both divisions are integer floor divisions: any fractional remainder is discarded, never rounded up. There is **no minimum-coupon rule** — a coupon that floors to `0` is a valid result and yields a payout equal to the investor's principal. There is also **no annualization or time-based accrual**: `yield_bps` is applied once as a flat rate over the whole principal, regardless of how long funds were locked or when settlement occurs. The implementation does not read timestamps, lock durations, or any period length when computing the coupon.

Each example below states the principal, the bps, the intermediate product and division result, and the final payout. All values are in the token's smallest unit. Each result was cross-checked with independent integer arithmetic.

### Example (a): zero configured yield

- `total_principal = 1_000_000`
- `yield_bps = 0`
- `contribution = 1_000_000` (single investor funding the whole escrow)
- Step (1): `coupon = 1_000_000 × 0 / 10_000 = 0 / 10_000 = 0`
- Step (2): `settle_pool = 1_000_000 + 0 = 1_000_000`
- Step (3): `gross_payout = 1_000_000 × 1_000_000 / 1_000_000 = 1_000_000`
- **Final payout: `1_000_000`** (equal to principal; coupon is exactly zero)

### Example (b): positive yield rounding below one base unit

- `total_principal = 1_000_000`
- `yield_bps = 1` (0.01%)
- `contribution = 1_000_000` (single investor funding the whole escrow)
- Step (1): `coupon = 1_000_000 × 1 / 10_000 = 1_000_000 / 10_000 = 100` — here the coupon is a whole number, so use a smaller principal to show the floor-to-zero case:
- `total_principal = 9_999`
- `yield_bps = 1`
- `contribution = 9_999`
- Step (1): intermediate product `9_999 × 1 = 9_999`; `coupon = 9_999 / 10_000 = 0` (floor division discards the remainder `9_999`)
- Step (2): `settle_pool = 9_999 + 0 = 9_999`
- Step (3): `gross_payout = 9_999 × 9_999 / 9_999 = 9_999`
- **Final payout: `9_999`** (coupon floors to `0`, so payout equals principal — no minimum coupon is added)

### Example (c): exact whole-unit coupon

- `total_principal = 1_000_000`
- `yield_bps = 500` (5%)
- `contribution = 1_000_000` (single investor funding the whole escrow)
- Step (1): intermediate product `1_000_000 × 500 = 500_000_000`; `coupon = 500_000_000 / 10_000 = 50_000` (exact, no remainder)
- Step (2): `settle_pool = 1_000_000 + 50_000 = 1_050_000`
- Step (3): `gross_payout = 1_000_000 × 1_050_000 / 1_000_000 = 1_050_000`
- **Final payout: `1_050_000`** (principal `1_000_000` + coupon `50_000`)

For a partial investor in example (c), e.g. `contribution = 250_000`, step (3) gives `250_000 × 1_050_000 / 1_000_000 = 262_500_000_000 / 1_000_000 = 262_500`, i.e. a pro-rata share of the same `settle_pool` with floor rounding applied per investor.

## Integration Guidance

- Off-chain callers should validate amount and lock-duration inputs before submitting transactions, especially when simulating near integer limits.
- Risk and accounting systems should use integer arithmetic for base-unit amounts and rational math for pro-rata ratios; avoid floating-point rounding when reconciling on-chain state.
- Maturity and claim-lock checks are ledger-time checks, not wall-clock oracle checks.
- Unsupported token economics remain out of scope. Fee-on-transfer, rebasing, malicious, or callback-heavy tokens are covered separately in [`escrow/src/external_calls.rs`](../escrow/src/external_calls.rs) and [`ESCROW_TOKEN_INTEGRATION_CHECKLIST.md`](ESCROW_TOKEN_INTEGRATION_CHECKLIST.md).

