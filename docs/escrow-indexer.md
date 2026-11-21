# Escrow Indexer Ops (Stellar/Soroban)

Minimal indexer strategy for `starfund_escrow`: what to subscribe to as events, what to poll as
state keys, and how to handle stale-RPC vs chainhead disagreement.

---

## 1) Minimal architecture

Use **both** channels:

- **Event stream (primary timeline):** subscribe to contract events for lifecycle transitions and
  append-only activity.
- **Storage reads (state truth):** poll selected read endpoints to reconcile derived state and
  recover from gaps/restarts.

Recommended split:

- **Subscribe** for write activity (funding, settle, admin/legal-hold controls, claims, attestation updates).
- **Poll** for canonical snapshots and optional/hint fields (`get_escrow`, `get_registry_ref`, etc.).

---

## 2) What to subscribe to (contract events)

Contract event names (`symbol_short`) emitted by `escrow/src/lib.rs`:

- `escrow_ii` - escrow initialized (payload includes `escrow`, `funding_token`, `treasury`, `registry`)
- `funded` - contribution recorded
- `escrow_sd` - settled
- `sme_wd` - SME withdraw
- `inv_claim` - investor payout claimed
- `dust_sw` - treasury dust sweep
- `legalhld` - legal hold changed
- `admin` - admin transferred
- `maturity` - maturity updated
- `fund_tgt` - funding target updated
- `coll_rec` - SME collateral commitment recorded
- `coll_clr` - SME collateral commitment cleared
- `att_bind` - primary attestation hash bound
- `att_app` - attestation append-log updated
- `al_ena` - allowlist mode enabled/disabled
- `al_set` - investor allowlist membership set
- `inv_cap` - max unique investors cap lowered (admin, open state only)

See also: `docs/EVENT_SCHEMA.md`.

### RPC event filter baseline

- Filter by `contractId` = escrow contract id.
- Filter by topics where `topic[0]` is the event symbol above.
- Persist `(ledger, txHash, eventIndex)` as your idempotency cursor.

### Horizon/websocket note

If your stack uses Horizon streams/websocket wrappers, map them to Soroban contract event sources
for the same `contractId` and enforce the same idempotency key `(ledger, txHash, eventIndex)`.

---

## 3) What to poll (storage/read API)

Read endpoints and their backing keys (`docs/escrow-read-api.md`):

### Poll every cycle (canonical state)

- `get_escrow()` -> `DataKey::Escrow`
- `get_legal_hold()` -> `DataKey::LegalHold`
- `get_funding_close_snapshot()` -> `DataKey::FundingCloseSnapshot`
- `get_unique_funder_count()` -> `DataKey::UniqueFunderCount`
- `get_version()` -> `DataKey::Version`

### Poll on-demand (investor/account views)

- `get_contribution(investor)` -> `DataKey::InvestorContribution(investor)`
- `is_investor_claimed(investor)` -> `DataKey::InvestorClaimed(investor)`
- `get_investor_yield_bps(investor)` -> `DataKey::InvestorEffectiveYield(investor)` fallback to base
- `get_investor_claim_not_before(investor)` -> `DataKey::InvestorClaimNotBefore(investor)`

### Poll at startup + periodically (config/hints)

- `get_funding_token()` -> `DataKey::FundingToken` (immutable after init; also duplicated on `escrow_ii` for indexers)
- `get_treasury()` -> `DataKey::Treasury` (immutable after init; also on `escrow_ii`)
- `get_registry_ref()` -> `DataKey::RegistryRef` (optional hint only; also on `escrow_ii`)
- `get_min_contribution_floor()` -> `DataKey::MinContributionFloor`
- `get_max_unique_investors_cap()` -> `DataKey::MaxUniqueInvestorsCap`
- `get_primary_attestation_hash()` -> `DataKey::PrimaryAttestationHash`
- `get_attestation_append_log()` -> `DataKey::AttestationAppendLog`
- `get_sme_collateral_commitment()` -> `DataKey::SmeCollateralPledge`

---

## 4) Registry hint caveat (`get_registry_ref`)

`get_registry_ref()` is a discoverability hint, not authority:

- `None` is valid and operational.
- A non-`None` address does **not** prove membership or compliance by itself.
- If you need registry guarantees, call and verify that registry contract directly.

Do not treat `RegistryRef` as a trust anchor in risk engines without independent verification.

---

## 5) Failure modes: stale RPC vs chainhead

### Symptom A: event subscription lags chainhead

- **Observation:** latest closed ledger (network) is higher than event stream cursor.
- **Risk:** UI appears stale; lifecycle transitions (e.g., `settled`) are delayed.
- **Action:** keep event cursor, backfill missing ledger range, then resume live stream.

### Symptom B: storage read is behind while events are newer

- **Observation:** event indicates change, but `get_escrow()` still returns prior snapshot.
- **Risk:** temporary read-after-write inconsistency across providers.
- **Action:** gate reconciliation on ledger number; retry read against same-or-newer ledger.

### Symptom C: reorg/rollback window

- **Observation:** previously seen event disappears from canonical chain.
- **Risk:** derived tables contain orphaned transitions.
- **Action:** store ledger/tx/event provenance; support rollback of non-finalized ledgers and replay.

### Symptom D: provider disagreement (RPC A vs RPC B)

- **Observation:** different latest ledgers or inconsistent event availability.
- **Risk:** duplicated or missing state transitions.
- **Action:** prefer a single writer provider per environment, periodically cross-check, and use
  deterministic idempotency keys to avoid double application.

---

## 6) Recommended reconciliation loop

1. Read latest finalized/accepted ledger checkpoint.
2. Consume events from last stored cursor to checkpoint.
3. Apply event-driven projections idempotently.
4. Poll canonical read endpoints (`get_escrow`, hold, snapshot, version).
5. If projection != polled canonical state, mark drift and re-sync from last good ledger.

---

## 7) Bounded retry recipe: recovering from a stale RPC simulation

This section is the write-side companion to the read-side failure modes in
[section 5](#5-failure-modes-stale-rpc-vs-chainhead). A simulation is only valid against the ledger
state it was run on; if that state moves (concurrent admin update, lifecycle transition, nonce
consumption), the signed transaction is stale and must not be blindly resubmitted.

### 7.1 Decision points (in order)

1. **Detect staleness.** Compare the simulation's reported ledger with the latest accepted ledger
   checkpoint, and compare simulated reads with fresh canonical reads. Treat the simulation as
   stale if **any** of the following hold:
   - simulated ledger < latest accepted ledger checkpoint;
   - simulated `get_escrow()` snapshot != freshly polled `get_escrow()`;
   - simulated `get_admin_nonce()` != freshly polled `get_admin_nonce()`;
   - simulated `get_legal_hold()` or `get_version()` != freshly polled values.
2. **Refresh ledger state.** Re-read the latest accepted ledger checkpoint, then re-poll the
   canonical read endpoints used by the invocation: `get_escrow`, `get_admin_nonce`,
   `get_legal_hold`, `get_version` (plus any endpoint the call depends on, e.g.
   `get_funding_close_snapshot`).
3. **Check for an already-committed transaction.** Before retrying, query by tx hash (and your
   event cursor) to confirm the previous attempt did **not** land. If it is committed, stop: apply
   the result and do not retry. A stale simulation does not imply the submission failed.
4. **Re-simulate** against the refreshed state. Never reuse a simulation result from a different
   ledger.
5. **Diff invocation arguments.** Compare the new invocation arguments (nonce, target, amounts,
   addresses, and any lifecycle-dependent fields) with the arguments that were previously signed.
   - If **nothing** changed, the existing signatures remain valid; resubmit.
   - If **anything** changed, **discard the old signatures** and recollect fresh signatures from
     all required signers. Never reuse a signature over changed arguments.
6. **Bound retries.** Cap the loop (e.g. **max 3 attempts**). On exhaustion, stop retrying and
   surface persistent staleness as an alert (include escrow id, tx hash, attempt count, and the
   ledger numbers observed) rather than looping indefinitely.

### 7.2 Worked example: concurrent admin update + lifecycle transition

Scenario: an admin submits `settle`, while a second admin concurrently transfers admin
(`admin` event) and toggles legal hold (`legalhld` event). The `settle` simulation was run before
both landed.

- **Attempt 1 (stale simulation).** Simulated ledger `L`; latest accepted ledger is `L+2`.
  Fresh `get_admin_nonce()` is higher than simulated (admin transfer consumed the nonce), and
  `get_legal_hold()` / `get_version()` differ from the simulation.
  - *Decision point 1:* stale -> proceed to refresh.
  - *Decision point 2:* re-poll `get_escrow`, `get_admin_nonce`, `get_legal_hold`, `get_version`.
  - *Decision point 3:* query by tx hash -> not committed, so a retry is allowed.
  - *Decision point 4:* re-simulate against `L+2`.
  - *Decision point 5:* nonce changed (and possibly the admin signer set) -> **discard old
    signatures**, recollect from the current admin signers.
  - *Decision point 6:* attempt 1 of 3.
- **Attempt 2.** If a further admin update lands between re-simulation and submission, the same
  checks repeat. If the nonce/arguments are unchanged, resubmit with existing signatures; if
  changed, discard and recollect again.
- **Attempt 3 (bound reached).** If staleness persists, stop and raise an alert. Do not keep
  resubmitting stale nonce or stale-state transactions.

For lifecycle transitions, the same recipe applies: if `settle` races with a legal-hold change,
the refreshed `get_legal_hold()` / `get_escrow()` state may make the call revert or change its
preconditions. Re-simulate, diff arguments, and only resubmit with signatures that match the
final arguments.

---

## 8) Security notes for indexers

- Unsupported token economics (fee-on-transfer/rebasing/hook tokens) are out of scope for escrow
  accounting and are expected to fail closed at transfer balance checks in
  `escrow/src/external_calls.rs`.
- Indexers should surface these failures as integration alerts, not silently smooth them over.
- Treat attestation and collateral entries as metadata records unless/until on-chain enforcement is
  explicitly introduced by contract APIs.
