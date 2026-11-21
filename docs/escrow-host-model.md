# Escrow Host Call Model (Soroban)

This note explains the escrow contract's token transfer safety model for reviewers, focused on
Soroban execution semantics and the checks implemented in `escrow/src/external_calls.rs`.

## Short model

- The escrow calls the configured SEP-41 token contract via Soroban host.
- The token call executes and returns to escrow.
- Escrow validates post-call balance deltas for both sender and recipient.
- Any mismatch (fees, rebasing side effects, non-standard behavior) fails closed with panic.

## Transfer + balance-check timeline

```
Escrow contract
  -> read from_before, treasury_before
  -> call token.transfer(from, treasury, amount)
     -> token contract executes in host
     -> returns to escrow
  -> read from_after, treasury_after
  -> assert (from_before - from_after) == amount
  -> assert (treasury_after - treasury_before) == amount
```

## Soroban vs EVM reentrancy framing (contrast only)

- In Soroban, the host call boundary is explicit: escrow logic resumes after the token call returns.
- This avoids relying on EVM-style "checks-effects-interactions" folklore as the primary argument.
- The escrow still assumes the token can be adversarial from an accounting perspective, so
  invariant checks are mandatory after external calls.

## Security assumptions and out-of-scope behavior

The escrow wrapper is intentionally strict and supports standard SEP-41-like token behavior only.

- Assumed: balance deltas exactly match transfer amount.
- Out of scope: fee-on-transfer, rebasing, or hook-driven token economics.
- Policy: unsupported behavior should be blocked by allowlisting and integration review.
- Runtime fallback: if unsupported behavior occurs, transfer checks panic (safe failure).

## Callback registration host model

Callbacks are registered via `register_callback`, which records a `CallbackContext`
(origin address, nonce, `created_at`) and returns the allocated nonce.

### Supported origin address kinds

- Only external contract addresses are supported as callback origins.
- The escrow's own address (`env.current_contract_address()`) is **not** a supported origin:
  a self-origin registration has no external origin authorization workflow and would create
  unusable state.
- Self-origin registration is rejected with the typed configuration error
  (`Error::InvalidCallbackOrigin`) **before** any nonce is allocated, so a rejected
  registration never consumes a nonce.
- Any other contract address (e.g. a real external origin fixture) registers normally.

### Callback lifetime and retirement policy

- Registrations remain valid **indefinitely** by default. `created_at` is recorded for
  observability and audit only; it does not by itself expire a registration.
- A pending callback is retired only when it is explicitly cancelled by the admin or
  consumed by execution. There is no implicit age-based expiration.
- If an expiry policy is adopted in the future, it must be specified explicitly, including
  exact-boundary behavior (e.g. whether a callback is still valid at `created_at + ttl`)
  and backward compatibility for already-registered callbacks. Until then, reviewers should
  treat old pending callbacks as still valid per this policy.

### Admin cancellation workflow

- The admin may cancel an outstanding callback registration; cancellation retires the
  pending authorization so it can no longer be consumed.
- Cancellation is the supported way to retire a callback that is no longer wanted, in place
  of relying on age-based expiration.

## Reviewer checklist

- Confirm all token transfers that matter for accounting use the wrapper in
  `escrow/src/external_calls.rs`.
- Confirm both sender and recipient deltas are asserted exactly.
- Confirm out-of-scope token behavior is documented in integration/security docs.
- Confirm `register_callback` rejects the escrow's own address before nonce allocation and
  still accepts real external contract origins.
- Confirm callback lifetime behavior matches the documented policy (indefinite unless
  cancelled or consumed).
