# REST Transaction Submission, Finality Semantics, and Authorization Guidance

## 1. Overview
This document specifies the StarFund REST API transaction submission lifecycle, ledger finality semantics, authentication separation, and request body schema policies.

---

## 2. REST Transaction Submission and Finality Semantics (Issue #369)
When clients submit invoice or escrow transactions via the REST API (`/v1/invoices`, `/v1/escrows`):
1. **Synchronous Ingestion & Validation**: The REST server performs syntax checks, OpenAPI schema validation, and signature verification.
2. **Asynchronous Stellar Network Dispatch**: The transaction envelope is submitted to Horizon / Soroban RPC.
3. **Transaction Status Lifecycle**:
   - `PENDING`: The transaction envelope has been received by the REST API and queued for submission.
   - `SUBMITTED`: The transaction has been accepted by the Stellar validator pool into the candidate transaction set.
   - `CONFIRMED`: The transaction has been included in a closed ledger and confirmed by consensus.
   - `FAILED`: Submission was rejected by Soroban RPC or failed during execution (e.g. insufficient balance, precondition check failed).
4. **Finality Guarantees**:
   - Stellar provides deterministic 1-ledger finality (~4–5 seconds) with zero possibility of reorganizations.
   - A transaction is considered final once closed in ledger $N$ and attested by Horizon ledger headers.

---

## 3. Separation of HTTP Authentication and Contract Authorization (Issue #368)
StarFund enforces a strict decoupling between transport-level HTTP authentication and on-chain contract authorization:

| Dimension | HTTP Authentication | Contract Authorization |
| :--- | :--- | :--- |
| **Mechanism** | Bearer JWT (RFC 7519) | Soroban Address Signature (`require_auth()`) |
| **Scope** | Authorizes access to off-chain REST endpoints, rate limit quotas, and telemetry | Authorizes movement of funds, state transitions, and escrow releases |
| **Verification** | Verified statelessly by REST gateway using JWT public signing keys | Verified cryptographically on-chain by the Stellar VM / Soroban host |
| **Revocation** | Short-lived token expiry (15 minutes) with refresh rotation | Sequence numbers and ed25519 signer weight thresholds |

> **Critical Security Rule**: An authenticated HTTP session **never** implies or bypasses on-chain contract authorization. All mutating transactions must include valid Soroban cryptographic authorization from the required address.

---

## 4. Unknown-Field Policy for API Requests (Issue #367)
To prevent parameter tampering, unexpected deserialization vulnerabilities, and protocol drift:
- All incoming REST mutation request payloads are evaluated under an **explicit rejection policy** (`additionalProperties: false`).
- Any request containing unrecognized or undeclared properties is immediately rejected with HTTP `400 Bad Request` and error code `ERR_UNRECOGNIZED_FIELD`.
- Clients must only submit fields explicitly documented in `docs/openapi.yaml`.

---

## 5. OpenAPI Example Conformance (Issue #366)
Every request and response example declared in `docs/openapi.yaml` is validated in CI against its corresponding component schema definition, guaranteeing that documentation fixtures never drift from production API contracts.
