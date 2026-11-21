# Lossless Contract Amounts, Invoice Identifiers, and Claimable Payout Gates

## 1. Overview
This specification addresses lossless JSON representation for 128-bit Soroban contract amounts, strict validation for invoice identifiers and lifecycle scalars, dispute/pause gating on investor payout views, and remediation of locked documentation dependency advisories.

---

## 2. Claimable Payout Pause and Dispute Gates (Issue #334)
The `get_claimable_payout` view calculates investor returns but must never mislead users into attempting claims while the contract is guarded:
- **Pause Gate**: If `is_paused()` is true for `PauseScope::Settlement` or `PauseScope::All`, claimable amounts reflect zero executable balance and indicate `PAUSED`.
- **Dispute Gate**: When an active dispute is flagged (`dispute_active: true`), claim distributions are locked until dispute resolution settles.
- **Legal Hold**: Compliance locks prevent payouts until explicitly lifted by authorized governance.

---

## 3. Lossless JSON Representation for Contract Amounts (Issue #335)
JavaScript's `Number` type loses integer precision above $2^{53} - 1$ ($9,007,199,254,740,991$), while Soroban represents monetary amounts and stroop balances as signed 128-bit integers (`i128`, up to $1.7 \times 10^{38}$):
- All contract amounts in REST requests and responses MUST support string-encoded representations (`type: string`, `pattern: "^[0-9]+$"`) alongside integer formats.
- Client SDKs must deserialize amount fields into `BigInt` (JS) or `u128`/`i128` (Rust) without IEEE 754 float cast.

---

## 4. Invoice Identifiers and Lifecycle Scalar Alignment (Issue #336)
- **Invoice Identifier Constraint**: Aligns with Soroban `Symbol` constraints (alphanumeric, underscores, hyphens, min 1 char, max 12 chars: `^[A-Za-z0-9_-]{1,12}$`).
- **Lifecycle Scalar Enum**: Aligns OpenAPI scalar definitions with on-chain `EscrowStatus`:
  - `0`: Open / Funding Active
  - `1`: Funded / Awaiting Maturity or Settlement
  - `2`: Settled / Payouts Available
  - `3`: Cancelled / Refunded

---

## 5. Documentation Dependency Security Hygiene (Issue #337)
- Documentation test dependencies in `docs/package.json` are maintained with safe semver ranges avoiding known CVE advisories in dev dependencies (e.g., prototype pollution or ReDoS in transitive schema parsers).
