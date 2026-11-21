# Callback Nonce Errors, External Token Address Guards, and Upgrade Test Specifications

## 1. Overview
This document specifies the resolution for callback nonce overflow error mappings, external token address validation preventing reentrant self-transfers, upgrade authorization and WASM bytecode replacement test invariants, and negative test boundaries for pause rate limiting.

---

## 2. Callback Nonce Overflow Error Mapping (Issue #285)
In `register_callback`:
- Previously, incrementing the callback registration nonce on overflow emitted a misleading `FundedAmountOverflow` error code.
- This mapping is corrected to emit `EscrowError::Overflow` (or dedicated `CallbackNonceOverflow`), ensuring precise diagnostics when the $u64$ callback counter reaches capacity.

---

## 3. External Token Address Self-Match Guard (Issue #287)
In `external_calls.rs`:
- An explicit validation guard verifies that the configured token contract address does NOT match the current escrow contract's own address (`token != env.current_contract_address()`).
- Attempting to initialize or fund an escrow where `token == current_contract_address` is rejected immediately with `EscrowError::InvalidToken`. This prevents re-entrancy anomalies and circular invocation loops.

---

## 4. Contract Upgrade Authorization & WASM Replacement Invariants (Issue #290)
Contract upgrades (`upgrade(new_wasm_hash)`):
- Require cryptographic authorization from the administrative account (`admin.require_auth()`).
- Verify that `new_wasm_hash` represents valid, uploaded Soroban contract executable bytecode.
- Replace bytecode in-place while preserving all persistent storage state, balances, and registry keys.

---

## 5. Pause Rate Limit Window Arithmetic & Bound Tests (Issue #291)
In `set_pause_rate_limit(window_seconds, max_toggles)`:
- Negative unit test coverage validates edge bounds:
  - `window_seconds == 0`: rejected with `EscrowError::InvalidPauseWindow`.
  - `window_seconds > MAX_PAUSE_WINDOW` (30 days): rejected.
  - `max_toggles == 0`: rejected with `EscrowError::InvalidThreshold`.
  - Overflow checks on sliding window timestamp arithmetic (`now.checked_sub(window_seconds)`).
