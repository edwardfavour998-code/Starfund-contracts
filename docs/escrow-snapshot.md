# Funding Close Snapshot

The `FundingCloseSnapshot` is a critical piece of the StarFund Escrow contract's audit trail. It captures the exact state of the escrow at the moment it transitions from `Open` (0) to `Funded` (1).

## Purpose

This snapshot serves as the **immutable source of truth** for off-chain pro-rata calculations. When an invoice is over-funded (which is allowed by the contract), the full `funded_amount` at the threshold-crossing deposit becomes the denominator for investor share calculations, even when it is greater than `funding_target`.

By capturing this state once and making it immutable, the contract ensures that subsequent actions (like SME withdrawals or settlements) do not shift the relative weight of investor contributions.

## Structure

The snapshot is stored under `DataKey::FundingCloseSnapshot` and contains:

- `total_principal`: The sum of all principal contributed at the moment the funding target was met or exceeded. This equals `InvoiceEscrow.funded_amount` at close and can be greater than `funding_target`.
- `funding_target`: The original target for the invoice.
- `closed_at_ledger_timestamp`: The ledger timestamp when the snapshot was captured.
- `closed_at_ledger_sequence`: The ledger sequence number when the snapshot was captured.

## Lifecycle and Immutability

1. **Before close**: `get_funding_close_snapshot()` returns `None` while the escrow is still open and below target.
2. **Creation**: The snapshot is created by one of four production writers in `escrow/src/lib.rs` (see [Snapshot Writers](#snapshot-writers)). Two are deposit-triggered closes and two are administrative closes; not every snapshot is produced by a threshold-crossing deposit.
3. **Over-funding capture**: If the threshold-crossing deposit overshoots the target, `total_principal` records the full over-funded close amount.
4. **Write-Once**: Once the snapshot is written, the contract's logic prevents it from being updated or overwritten. Later funding attempts are rejected because the escrow is no longer open, and later lifecycle writes do not touch `DataKey::FundingCloseSnapshot`.
5. **Persistence**: The snapshot survives all state transitions, including `settle` and `withdraw`.

## Snapshot Writers

Every production write to `DataKey::FundingCloseSnapshot` in `escrow/src/lib.rs` goes through the same helper, which records `total_principal` from the current `funded_amount`, `funding_target` from the current `funding_target`, and the current ledger timestamp and sequence. The four writers differ in *why* the escrow closes and therefore in what those recorded values mean:

| Writer | Close type | Trigger | Recorded `funding_target` | Recorded `total_principal` |
| --- | --- | --- | --- | --- |
| `fund` | Deposit-triggered | `status == 0` and new `funded_amount >= funding_target` | The target in force at the deposit | Full `funded_amount` at close (may exceed the target) |
| `fund_with_commitment` | Deposit-triggered | `status == 0` and new `funded_amount >= funding_target` | The target in force at the deposit | Full `funded_amount` at close (may exceed the target) |
| `partial_settle` | Administrative (early close) | SME closes the escrow before the target is reached | The target in force at the early close (not met) | `funded_amount` at the early close, which is **below** `funding_target` |
| `update_funding_target` | Administrative (target lowering) | SME lowers `funding_target` to or below the already-raised `funded_amount` | The **new, lowered** target | `funded_amount` already raised, which is now `>=` the new target |

Notes for auditors:

- **Deposit-triggered closes** (`fund`, `fund_with_commitment`) are the only paths where a single deposit crosses the threshold. The `EscrowFunded` event is emitted alongside the snapshot on these paths.
- **Administrative closes** (`partial_settle`, `update_funding_target`) produce a snapshot **without a threshold-crossing deposit**. In `partial_settle` the escrow closes under-funded; in `update_funding_target` the threshold moves down to meet funds that were already present. Neither path involves a new contribution at close time.
- Because `partial_settle` can record `total_principal < funding_target`, off-chain consumers must not assume `total_principal >= funding_target` when using the snapshot as a pro-rata denominator. Always read the recorded `funding_target` and `total_principal` together.
- The write-once guard means only the first of these writers to fire captures the snapshot; subsequent writers observe the escrow as no longer open and do not overwrite it.

### Examples

**Deposit-triggered close (`fund` / `fund_with_commitment`)**

- Target: 1,000. Prior `funded_amount`: 900. A deposit of 200 brings `funded_amount` to 1,100, crossing the threshold.
- Snapshot: `funding_target = 1,000`, `total_principal = 1,100`. The over-funded close amount is the denominator.

**Administrative early close (`partial_settle`)**

- Target: 1,000. `funded_amount`: 600. The SME calls `partial_settle` before the target is reached.
- Snapshot: `funding_target = 1,000`, `total_principal = 600`. No threshold-crossing deposit occurred; the snapshot records an under-funded close.

**Administrative target lowering (`update_funding_target`)**

- Target: 1,000. `funded_amount`: 600. The SME lowers `funding_target` to 500.
- Snapshot: `funding_target = 500` (the new, lowered target), `total_principal = 600`. The escrow closes because the lowered target is now met by funds already raised, not by a new deposit.

## Auditing

Integrators can use the `get_funding_close_snapshot` getter to retrieve this metadata. For historical auditing, the `EscrowFunded` event emitted during deposit-triggered snapshot creation (`fund`, `fund_with_commitment`) contains the `funded_amount` and `status: 1`, allowing off-chain systems to reconcile the snapshot with the event stream. Administrative closes (`partial_settle`, `update_funding_target`) do not emit `EscrowFunded`; auditors must reconcile those snapshots against the settlement and target-update events instead, and should not assume every snapshot has a matching `EscrowFunded` event.

## Paginated Funding Records

`get_funding_records` scans the contribution index and filters out records whose balance has been reduced to zero (for example after a withdrawal). Because of this filtering, a page can be **empty before the end of the index**: the returned vector length alone is not a safe continuation signal. A client that stops on an empty page can miss later live investors.

To make continuation explicit, the contract exposes a companion view, `get_funding_records_page(start, limit)`, which returns a `FundingRecordsPage`:

- `records`: The live funding records found in the scanned range, in index order. This may be empty even when more records remain.
- `next_offset`: The index offset at which the next page should start. Pass it as `start` to the next call.
- `done`: `true` when the scan has reached the end of the contribution index; `false` when more slots remain to be scanned.

### Continuation Semantics

- Always continue while `done == false`, even if `records` is empty. An empty page with `done == false` still carries a valid `next_offset`.
- Stop only when `done == true`. At that point `next_offset` equals the end of the index and no further live records exist.
- Starting from `start = 0` and repeatedly calling `get_funding_records_page(next_offset, limit)` until `done == true` visits every live record exactly once on fixed state, regardless of how many zero-balance slots are interleaved.

### Compatibility

The existing `get_funding_records` getter is unchanged and remains compatible: it still returns the filtered vector of live records. New integrations should prefer `get_funding_records_page` so that sparse, empty pages do not truncate traversal.

The `closed_at_ledger_timestamp` and `closed_at_ledger_sequence` fields are captured from the same ledger as the threshold-crossing funding call. Off-chain indexers should use those fields as the canonical close boundary for pro-rata reporting.

## Security Considerations

1. **Time and Sequence Bounds**: The snapshot captures `env.ledger().timestamp()` and `env.ledger().sequence()`. In Soroban, these are provided by the host environment and are reliable for on-chain time-based logic. Off-chain systems should treat these as the canonical boundaries for the "funded" state transition.
2. **Write-Once Denominator**: `DataKey::FundingCloseSnapshot` is only set if it does not already exist. All four writers (`fund`, `fund_with_commitment`, `partial_settle`, `update_funding_target`) respect this guard, and state transitions such as `settle` and `withdraw` do not recompute the denominator, which prevents later writes from changing investor weights.
3. **State-Machine Misuse**: Funding after close is rejected by the `status == 0` funding guard before contribution or snapshot state can be mutated.
4. **Overflow and Amount Guards**: Funding uses positive amount checks and checked arithmetic before writing `funded_amount` or contribution records.
5. **Token Economics and Assumptions**: As detailed in `escrow/src/external_calls.rs`, this contract strictly assumes standard SEP-41 token mechanics. Malicious, rebasing, or fee-on-transfer (FOT) tokens are **explicitly out of scope** and will trigger safe-failure panics at the balance-check boundaries. This ensures that the `total_principal` captured in the snapshot matches standard token accounting assumptions, preserving the integrity of off-chain payout calculations.
