# Escrow Lifecycle Checks on Recovery, Allowlist Coordination, and Test Guards

## 1. Overview
This specification hardens escrow lifecycle verification during pending payer recovery, establishes coordinated allowlist state recovery, details public initialization guard invariants, and defines token contract failure assertions in test suites.

---

## 2. Escrow Lifecycle Recheck on Payer Recovery Acceptance (Issue #332)
When accepting a pending payer recovery proposal (`accept_payer_recovery`):
- Payer recovery can only be proposed and accepted while the escrow remains in a valid **pre-settlement** state (`status == 0` [Open] or `status == 1` [Funded]).
- If an escrow transitions to `Settled` or `Cancelled` while a proposal is pending, `accept_payer_recovery` MUST re-evaluate the escrow lifecycle state before committing the new payer address.
- If the escrow is no longer pre-settlement, the recovery execution is rejected with `EscrowError::InvalidState` or `PayerRotationNotAllowed`.

---

## 3. Coordinated Recovery for Allowlist Pages and Flags (Issue #333)
The allowlist subsystem partitions large investor registries into paginated chunks with status bitmaps:
- If an allowlist page is missing from storage (e.g. archived entry or failed migration) while the corresponding page flag indicates occupancy:
  - The contract does NOT leave storage in a split-brain condition.
  - A coordinated recovery path treats the page as unverified, requiring explicit administrative reconciliation before investor funding or claim operations can proceed against that segment.

---

## 4. Initialization Guard Enforcement (Issue #338)
- The public `init` entrypoint is strictly single-use.
- Invoking `init` on an already-initialized contract immediately returns `EscrowError::AlreadyInitialized`.
- All operational entrypoints verify that the contract has been initialized (`EscrowError::NotInitialized`) before accessing storage.

---

## 5. Explicit Token Failure Handling in Test Flows (Issue #339)
In test suites and simulation harnesses:
- When a mocked or real Soroban token contract fails (e.g. insufficient allowance, frozen trustline, or host transfer rejection), test fixtures must NOT catch and silently substitute a working token stub.
- All transfer failures must propagate deterministically as expected `EscrowError` variants, ensuring that negative test cases assert actual contract error handling.
