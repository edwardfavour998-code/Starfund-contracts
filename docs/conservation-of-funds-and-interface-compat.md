# Conservation of Funds Invariants, OpenAPI Structural Validation, and Interface Compatibility

## 1. Overview
This specification details the property-based conservation of funds invariants for mixed funding and partial settlement, complete OpenAPI document structure validation, CI contract interface compatibility checks, and dependency emergency coverage threshold calibration.

---

## 2. Conservation of Funds Invariant (Issue #288)
Across arbitrary combinations of:
- Multiple funding deposits across distinct investors
- Platform administrative fee deductions
- Partial settlements with fractional buyer payouts
- Pro-rata claim withdrawals and refunds

The contract maintains the fundamental accounting invariant:
$$\sum \text{InvestorDeposits} + \sum \text{SettlementReceipts} = \text{ContractBalance} + \sum \text{DisbursedPayouts} + \sum \text{PlatformFees}$$
No operation can create or destroy tokens without on-chain record. In property tests with Proptest, 1,000+ randomized sequences verify that:
$$\Delta \text{LedgerBalance} = \Delta \text{Liabilities}$$
holds exactly with zero remainder.

---

## 3. Complete OpenAPI Document Structure Validation (Issue #352)
The OpenAPI test runner validates the structural integrity of the root document:
- Validates all paths (`/v1/health`, `/v1/invoices`, `/v1/escrows`) declare valid operation IDs, tags, parameters, and responses.
- Ensures all referenced components in `$ref` exist and resolve without circular loops or orphan nodes.
- Checks that all security requirements bind to declared schemes (`bearerAuth`).

---

## 4. Emergency Coverage Threshold Calibration (Issue #354)
- CI code coverage enforcement via `cargo-llvm-cov` is calibrated with a strict 90% line threshold on production code (`starfund_escrow`).
- Excludes test scaffolding and test mocks from the denominator to avoid misleading dilution.

---

## 5. Contract Interface Compatibility Verification in CI (Issue #356)
- CI verifies that newly compiled WASM exports match expected Soroban contract function signatures (`init`, `fund`, `fund_batch`, `settle`, `partial_settle`, `claim`, `cancel`, `upgrade`).
- Prevents breaking changes in contract function interfaces that would desynchronize frontend indexers and mobile client bindings.
