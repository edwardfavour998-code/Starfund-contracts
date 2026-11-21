# Funding Window Extensions, Invoice Cancellation Tests, and Emergency Circuit Breaker Procedures

## 1. Overview
This specification details contract validation for funding window extensions, negative test coverage for expired invoice cancellations, fee basis points arithmetic documentation, and operational recovery runbooks for emergency circuit breakers.

---

## 2. Funding Window Extension Validation (Issue #344)
When an authorized administrative caller extends the funding deadline:
- The contract enforces strict non-zero validation on the proposed extension:
  $$\text{extension\_seconds} > 0 \land \text{new\_deadline} > \text{current\_ledger\_timestamp}$$
- Attempting to pass `extension_seconds == 0` or setting a timestamp in the past is rejected with `EscrowError::InvalidDeadline`.
- Deadlines must not exceed the global maximum maturity horizon (`MAX_MATURITY_HORIZON_SECONDS = 31,536,000`).

---

## 3. Negative Unit Tests for Expired Invoice Cancellation (Issue #345)
Unit test coverage validates boundary states when cancelling invoices:
- **Prior to Expiry**: Calling `cancel_expired_invoice` before the deadline elapses (`ledger_timestamp < deadline`) fails with `EscrowError::DeadlineNotReached`.
- **Funded State**: Invoices that reached full funding cannot be cancelled via expiry, failing with `EscrowError::InvalidState`.
- **Post-Expiry Cancellation**: Once `ledger_timestamp >= deadline` and `status == Open`, cancellation succeeds, transitioning status to Cancelled and freeing locked liquidity.

---

## 4. Fee Basis Points Guidance & Denominator Clarification (Issue #346)
All administrative fee calculations specify basis points ($1\text{ bps} = 0.01\%$):
- Mathematical formula:
  $$\text{Fee} = \frac{\text{Amount} \times \text{fee\_bps}}{10,000}$$
- Maximum allowable fee basis points is bounded to $\text{MAX\_FEE\_BPS} = 1,000$ ($10.00\%$). Configurations exceeding this bound are rejected with `EscrowError::FeeTooHigh`.

---

## 5. Emergency Circuit Breaker Recovery Procedures (Issue #347)
When anomalous network events, oracle failure, or security incidents trigger the global emergency pause (`PauseScope::All`):
1. **Immediate Quarantine**: All fundings, withdrawals, settlements, and claims are halted atomically.
2. **Investigation Window**: Auditors and admin signers inspect on-chain state and ledger history.
3. **Selective Scoped Unpausing**:
   - `PauseScope::Withdrawal` can be lifted independently once collateral integrity is verified.
   - `PauseScope::Settlement` remains engaged until oracle verification completes.
4. **Full Recovery Protocol**: Admin invokes `unpause_all()` with valid signature weight, resuming normal operation and emitting a `CircuitBreakerResumed` event.
