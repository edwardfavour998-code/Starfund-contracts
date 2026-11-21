# ADR-002: Authorization Boundaries

**Status:** Accepted  
**Date:** 2026-03-28  
**Refs:** `escrow/src/lib.rs` — `init`, `fund`, `settle`, `withdraw`, `claim_investor_payout`, `sweep_terminal_dust`, `set_legal_hold`, `propose_admin`, `accept_admin`, `rotate_beneficiary`, `recover_admin`, `consume_admin_nonce`

---

## Context

Multiple principals interact with the escrow (admin, SME, investors, treasury). Each entrypoint must enforce exactly the right `require_auth()` call so no role can act outside its boundary.

## Decision

| Entrypoint | Required signer |
|---|---|
| `init` | `admin` |
| `fund`, `fund_with_commitment` | `investor` (per-call) |
| `settle`, `withdraw` | `sme_address` |
| `claim_investor_payout` | `investor` |
| `sweep_terminal_dust` | `treasury` (immutable after init) |
| `set_legal_hold`, `clear_legal_hold` | `admin` |
| `update_funding_target`, `update_maturity`, `migrate` | `admin` |
| `propose_admin` | current `admin` |
| `accept_admin` | pending admin stored in `DataKey::PendingAdmin` |
| `record_sme_collateral_commitment` | `sme_address` |
| `batch_record_collateral` | `sme_address` |

`admin` and `treasury` are stored at `init`; `treasury` is immutable, while `admin` rotates only through a two-step handover. The current admin calls `propose_admin(new_admin)`, which stores `DataKey::PendingAdmin` without changing authority. The pending address must then call `accept_admin()`, which requires its own authorization, promotes it into `InvoiceEscrow::admin`, and clears `DataKey::PendingAdmin`. The deprecated `transfer_admin` shim must not be treated as an immediate transfer; it only creates the pending proposal.

There is no superuser that can act as all roles simultaneously unless the same key is used for multiple roles — which is a deployment concern, not a contract concern.

### Role-address separation policy (issues #430, #195, #251)

`init` enforces a deployment invariant: `admin`, `sme_address` and `treasury` must be pairwise distinct, and the escrow contract's own address cannot be the beneficiary. This is not merely an init-time check; it is a **policy that every later authorized rotation must preserve**, validated against the *current* role addresses:

- `rotate_beneficiary(new_sme)` rejects `new_sme` equal to the current SME (`NewSmeSameAsCurrent`), the current `admin`, the current `treasury`, or the escrow contract address.
- `propose_admin(new_admin)` rejects `new_admin` equal to the current admin (`NewAdminSameAsCurrent`), the current SME, or the current treasury.
- `accept_admin()` **rechecks** the pending admin against the *then-current* SME and treasury, because those roles can change between nomination and acceptance. A pending admin that now conflicts with a changed role is rejected explicitly (the pending proposal is not silently promoted); the current admin must re-nominate a non-conflicting address. The pending entry is cleared so a stale, conflicting proposal cannot be replayed.

The shared policy lives in a single helper so new governance entrypoints cannot diverge from it.

### Permissionless cleanup vs. authenticated nonce (issue #431)

`recover_admin` deliberately omits current-admin authorization so that an expired pending proposal can be garbage-collected even if the admin key is lost. It must **not** advance the shared administrative nonce: `consume_admin_nonce` is only invoked by authenticated governance paths (i.e. after the relevant `require_auth()`), never by the permissionless cleanup path. Consequently an unrelated caller can remove an expired proposal but cannot invalidate an already-prepared administrative invocation that uses that nonce. Cleanup events record the caller as the cleanup authority (distinct from governance authority) so the two are auditable separately.

## Consequences

- A compromised investor key cannot settle or sweep funds.
- A compromised SME key cannot change the admin or sweep dust.
- Treasury auth on `sweep_terminal_dust` means the admin cannot drain the contract as "dust" unless it is also the treasury.
- Legal hold can only be set/cleared by admin, so governance controls compliance freezes.
- Admin authority changes only after both the current admin and successor have authorized. A typo in `new_admin` can be corrected by a new `propose_admin` call and does not lock admin-gated paths.
- `DataKey::PendingAdmin` is cleared after successful acceptance so stale proposals cannot be reused.
- Role separation (admin/SME/treasury distinct, escrow not a beneficiary) holds for the lifetime of the escrow, not just at `init`; rotations cannot collapse roles onto one key.
- A pending admin that conflicts with a role changed after nomination is rejected at acceptance rather than promoted, so a nomination cannot be used to bypass the separation policy.
- Permissionless `recover_admin` cleanup cannot advance the governance nonce, so an unrelated caller cannot invalidate a prepared authenticated administrative action; key-loss recovery of expired proposals remains available.
======= 

## Rejected alternatives

- **Admin can do everything:** creates a single point of failure; role separation limits blast radius.
- **No treasury auth on sweep:** would let anyone trigger dust transfers once terminal; treasury auth is a cheap extra gate.

## No-Op Guards

Certain administrative entrypoints enforce a no-op guard to reject updates that do not change the existing state, preventing noisy events and wasted storage writes:
- **`update_maturity`**: Rejects `new_maturity == old_maturity` (`MaturityUnchanged`).
- **`propose_admin`**: Rejects `new_admin == current_admin` (`NewAdminSameAsCurrent`).
- **`rotate_beneficiary`**: Rejects `new_sme == current_sme` (`NewSmeSameAsCurrent`), and also rejects `new_sme` equal to the current admin, treasury, or the escrow address (role-separation policy, issue #430).
- **`propose_admin` / `accept_admin`**: `propose_admin` rejects a `new_admin` equal to the current SME or treasury; `accept_admin` rechecks the pending admin against the current SME and treasury and rejects a conflicting pending proposal (issue #430).

## Shared gate helpers (issue #626)

The repeated pre-`require_auth` checks are centralised in private helpers so a new
risk-bearing entrypoint cannot accidentally omit or diverge from them:

| Helper | Signature | Replaces |
|---|---|---|
| `guard_not_legal_hold` | `fn(env, error)` | inline `ensure(!legal_hold_active, LegalHoldBlocks*)` in `sweep_terminal_dust`, `rotate_beneficiary`, `fund_impl`, `partial_settle`, `settle`, `withdraw`, `claim_investor_payout`, `cancel_funding` |
| `guard_status_eq` | `fn(env, status, expected, error)` | inline `ensure(status == expected, ...)` checks |
| `guard_status_in` | `fn(env, status, &[allowed], error)` | inline `ensure(matches!(status, ...), ...)` membership checks |
| `is_terminal_status` | predicate | inline `(status == 2 || status == 3 || status == 4)`, used by `sweep_terminal_dust` |
| `is_pre_settlement_status` | predicate | inline `(status == 0 || status == 1)`, used by `rotate_beneficiary` |
| `require_funding_open` | `fn(env, status)` | inline `ensure(status == 0, EscrowNotOpenForFunding)`, used by `fund_impl` |

All helpers are `#[inline(always)]` `pub(crate) fn` (or `const`) at the top of `escrow/src/lib.rs`
so they are inlined at call sites with no measurable cost; they preserve ADR-002's
canonical sequence (read-only preconditions → `Address::require_auth()` → storage writes
and token transfers) and the exact `EscrowError` variant at every call site. The
predicates (`is_terminal_status`, `is_pre_settlement_status`) are deliberately
separable from the `ensure`-side guards so that view helpers and tests can reuse
the same status-set definition without hiding a panic.

## Retry-safe admin nonce handling (issue #405)

Administrative setters consume a shared, monotonically increasing admin nonce. A
transaction that is *simulated* against nonce `N` is only valid if `N` is still the
current nonce when it is *submitted*. Concurrent admin operations (or a retry after a
dropped transaction) can invalidate a simulated nonce, so integrations must treat the
nonce as a resource that is read, signed, and re-read on failure — never cached across
attempts.

### The nonce contract surface

- `get_admin_nonce()` — read-only view returning the next nonce the contract will accept.
- `consume_admin_nonce(env, expected)` — the internal guard invoked by every
  nonce-consuming admin setter. It compares `expected` against the stored nonce and
  either advances it (success) or rejects the call (stale/incorrect nonce). The nonce
  is advanced **only** when the whole transaction commits; a rejected call leaves the
  stored nonce untouched.

The public setters that consume the admin nonce are:

| Setter | Signature (argument order) |
|---|---|
| `update_funding_target` | `update_funding_target(env, admin, nonce, new_target)` |
| `update_maturity` | `update_maturity(env, admin, nonce, new_maturity)` |
| `set_legal_hold` | `set_legal_hold(env, admin, nonce, active)` |
| `propose_admin` | `propose_admin(env, admin, nonce, new_admin)` |

Each takes the `admin` address (which must `require_auth()`), the `nonce` the caller
believes is current, and the setter-specific argument(s). The `nonce` argument is
always the value returned by `get_admin_nonce()` at simulation time — never a value
incremented locally from a previous attempt.

### Recipe

1. **Read the nonce.** Call `get_admin_nonce()` immediately before building the
   transaction. Do not reuse a nonce read in an earlier attempt or from a cached
   dashboard value.
2. **Sign the exact arguments.** Build the invocation with the exact `admin`, `nonce`,
   and setter arguments you intend to submit. Sign and submit that serialized payload
   unchanged — do not re-serialize, re-order, or re-encode arguments between simulation
   and submission, or the signed bytes will not match the simulated ones.
3. **Classify the outcome.** A transaction is either:
   - **Rejected** — the call failed (e.g. stale-nonce error, or any other `EscrowError`).
     The nonce was **not** consumed; the stored nonce is unchanged. The operation did
     not happen and is safe to retry after re-reading the nonce.
   - **Committed** — the transaction succeeded and the nonce was consumed (advanced by
     one). The operation happened; do **not** retry it. Any further admin call must
     re-read `get_admin_nonce()` to obtain the new value.
4. **Handle stale-nonce rejection.** If the rejection is specifically a stale/incorrect
   nonce, another admin operation committed first and advanced the nonce. Do not
   blindly resubmit the same payload — it will keep failing. Go back to step 1.
5. **Re-simulate before retry.** After re-reading the nonce, re-run simulation with the
   fresh nonce and the same setter arguments, then sign and submit the newly simulated
   payload. Never patch the nonce into an already-signed transaction.

### Concurrent admin operation example

Two operators (or two automated jobs) both hold admin authority and both want to run
an admin setter. Suppose the current nonce is `7`.

1. Operator A reads `get_admin_nonce() == 7`, simulates
   `update_funding_target(admin, 7, new_target)`, and submits it.
2. Operator B also read `get_admin_nonce() == 7` earlier, simulates
   `set_legal_hold(admin, 7, true)`, and submits it.
3. A's transaction commits first: the nonce advances to `8`.
4. B's transaction is **rejected** — its `nonce = 7` is now stale. B's nonce was not
   consumed and `set_legal_hold` did not take effect.
5. B re-reads `get_admin_nonce() == 8`, re-simulates
   `set_legal_hold(admin, 8, true)`, signs the fresh payload, and submits. It commits
   and the nonce advances to `9`.

The same pattern applies when a single operator retries after a dropped transaction:
if the original never committed, the nonce is unchanged and the retry with the same
nonce succeeds; if it did commit, the retry is rejected as stale and must re-read the
nonce before re-simulating. In both cases the rule is identical — **rejected means
nonce intact and retryable; committed means nonce consumed and must be re-read.**
