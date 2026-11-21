# Settlement Readiness and Yield Tier Invariant Specifications

## 1. Overview
This specification details the contract bounds for yield tiers, pro-rata ceiling invariants, funding commitment durations, and settlement readiness logic for the StarFund escrow contract.

---

## 2. Yield Tier Count Bounds (Issue #340)
To prevent unbound state growth, linear iteration gas exhaustion, and storage denial-of-service:
- The escrow contract enforces an explicit hard ceiling on the number of configured yield tiers:
  $$\text{MAX\_YIELD\_TIERS} = 10$$
- Any configuration payload exceeding 10 tiers is rejected at entry with `EscrowError::InvalidThreshold` / `YieldTierLimitExceeded`.

---

## 3. Safe Pro-Rata Invoice Ceilings (Issue #341)
Pro-rata yield allocations calculate investor payouts using:
$$\text{Payout} = \frac{\text{Deposit} \times (10000 + \text{YieldBps})}{10000}$$
To prevent 128-bit integer overflow during the intermediate multiplication:
- The maximum supported invoice funding ceiling is bounded to:
  $$\text{MAX\_INVOICE\_CEILING} = \frac{i128\text{::MAX}}{20000} \approx 8.5 \times 10^{33}\text{ stroops}$$
- All calculations utilize `checked_mul` and `checked_div`, ensuring that no combination of valid deposit and basis points can panic or overflow.

---

## 4. Initial Duration Persistence on First Committed Deposit (Issue #342)
When an escrow accepts its first committed funding deposit:
- The contract captures and persists the immutable `original_duration_seconds`.
- Subsequent partial funding deposits cannot extend or mutate the initial duration window, preventing deadline creep and ensuring predictable investor settlement timelines.

---

## 5. Settlement Readiness Helper Conditional Closure (Issue #343)
The `get_settlement_readiness` view helper evaluates all gating predicates deterministically:
- `is_settleable`: true when `status == Funded` and no legal hold or active pause is present.
- `maturity_reached`: true when current ledger timestamp is $\ge \text{maturity}$ (or $\text{maturity} == 0$).
- `legal_hold_active`: true when compliance hold is committed.
- `ready_now`: evaluates as `is_settleable && maturity_reached && !legal_hold_active`.
All branches close explicitly without ambiguous default fallback states.
