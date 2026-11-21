# SME Milestone Withdrawal Guards, Fee Split Validation, and Structured Events

## 1. Overview
This specification details the milestone completion guards for SME withdrawals, validation constraints on fee distribution split ratios, investor claim signature verification, and structured event emissions for milestone state transitions.

---

## 2. SME Withdrawal Milestone Completion Guard (Issue #292)
To protect investor funds and enforce deliverable-based disbursement:
- The SME recipient is strictly prevented from executing withdrawals prior to milestone attestation:
  $$\text{milestone\_completed} == \text{true} \land \text{dispute\_active} == \text{false}$$
- Calling `withdraw_sme_funds` before milestone verification is recorded on-chain is rejected with `EscrowError::InvalidMilestone` / `MilestoneNotCompleted`.
- Payouts are bounded to the exact allocation budgeted for the completed milestone tranche.

---

## 3. Fee Distribution Split Ratio Validation (Issue #293)
When configuring fee split schedules across platform, referrer, and reserve pools:
- The contract validates that all constituent split basis points sum to exactly 10,000 basis points ($100.00\%$):
  $$\sum_{i=1}^{n} \text{split\_bps}_i == 10,000$$
- Any split ratio configuration with $\sum \text{split\_bps} \ne 10,000$ or negative values is rejected with `EscrowError::InvalidFee`.
- Prevents unallocated fee dust or deficit accounting during settlement disbursements.

---

## 4. Investor Signature Schema Verification During Claims (Issue #327)
Investor claim verification:
- Validates the ed25519 signature schema and message hash format submitted by claiming accounts.
- Ensures that claim payloads cannot be replayed across different contract instances or network passphrases.
- Invalid signatures immediately revert with `EscrowError::InvalidSignature`.

---

## 5. Structured Milestone Transition Events (Issue #328)
Every milestone state change emits a strongly typed Soroban event:
- Topics: `(Symbol::new(&env, "milestone"), invoice_id, milestone_id)`
- Data payload: `(prev_status, new_status, amount, ledger_timestamp)`
- Provides indexers and UI dashboards with deterministic telemetry for milestone completion and disbursement releases.
