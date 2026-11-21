# Callback Phase Validation, Testutils Portability, and Payer Recovery Delays

## 1. Overview
This specification documents the callback execution phase validation rules, `testutils` feature compilation without `cfg(test)` `std` coupling, regression test discovery restoration, and decoupling of payer recovery timelocks from proposal expiration.

---

## 2. Callback Lifecycle Phase Validation (Issue #286)
When `execute_callback` is invoked by an external hook or registered listener:
- The handler MUST validate the current escrow lifecycle status before executing registered dispatch routines.
- Incompatible phase transitions (e.g., executing milestone or refund callbacks on an already Settled or Cancelled contract) are blocked with `EscrowError::InvalidState`.
- Callbacks must be explicitly bound to authorized phase identifiers (`Phase::Funding`, `Phase::Disbursement`, `Phase::Settlement`).

---

## 3. Testutils Feature Portability (Issue #329)
The `testutils` feature flag enables simulation harnesses and contract client wrappers:
- Previously, `testutils` code inadvertently depended on `std` imports conditioned on `cfg(test)`.
- To allow downstream consumer crates and WASM integration tests to compile with `--features testutils` in `no_std` environments:
  - All test utilities strictly use `soroban_sdk::testutils` types.
  - Zero unqualified `std::time`, `std::println`, or host filesystem symbols are referenced in non-test compilation units.

---

## 4. Contract Regression Test Module Discovery (Issue #330)
All regression test modules within `escrow/src/tests/` and top-level integration suites are explicitly declared in `lib.rs`:
- Test modules are discovered deterministically by `cargo test`.
- Prevents silent exclusion of edge-case tests or property-based invariant checks from CI test runs.

---

## 5. Payer Recovery Timelock Separation (Issue #331)
Payer recovery uses dual time-bounds to prevent front-running and indefinite limbo states:
- **Acceptance Timelock (`acceptance_delay`)**: Minimum delay (e.g. 48 hours) that must elapse after proposal submission before acceptance can be committed.
- **Proposal Expiry (`proposal_expiry`)**: Maximum horizon (e.g. 7 days) after which an unaccepted proposal expires and becomes invalid.
- Decoupling these two parameters ensures that governance has sufficient review time while unexecuted proposals automatically clear from storage.
