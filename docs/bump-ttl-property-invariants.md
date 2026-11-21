# Property-Based Testing Invariants for bump_ttl Thresholds

## 1. Overview
This document specifies the property-based testing invariants governing Soroban storage TTL extension (`bump_ttl`, Issue #289).

---

## 2. Invariant Specifications
When contract entrypoints or administrative operators invoke `bump_ttl(threshold, extend_to)` on persistent or instance storage:
1. **Monotonicity**: The remaining TTL must never decrease as a result of a `bump_ttl` invocation:
   $$\text{TTL}_{\text{post}} \ge \text{TTL}_{\text{pre}}$$
2. **Threshold Boundary Invariant**:
   If the current remaining TTL is strictly greater than `threshold`, no storage mutation or fee consumption occurs:
   $$\text{TTL}_{\text{pre}} > \text{threshold} \implies \text{TTL}_{\text{post}} = \text{TTL}_{\text{pre}}$$
3. **Extension Boundary Invariant**:
   If $\text{TTL}_{\text{pre}} \le \text{threshold}$, the storage entry TTL is extended to at least `extend_to`:
   $$\text{TTL}_{\text{pre}} \le \text{threshold} \implies \text{TTL}_{\text{post}} \ge \text{extend_to}$$
4. **Ordering Constraint**:
   `extend_to` must strictly exceed `threshold` (`extend_to > threshold`). Violations are rejected with `EscrowError::InvalidThreshold`.

---

## 3. Property Test Generator
A proptest harness generates arbitrary triples:
$$(\text{current\_ttl}, \text{threshold}, \text{extend\_to}) \in [0, 2^{32}-1]^3$$
verifying that the contract maintains state consistency without panic across all boundary conditions.
