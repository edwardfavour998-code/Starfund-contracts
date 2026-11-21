# Contract Release Artifact Verification & WASM Size Budgets

## 1. Overview
This guide documents the formal procedure for building, verifying, and checking the size budgets of the StarFund smart contract release WASM artifacts.

---

## 2. Release WASM Size Budget (Issue #357)
To ensure optimal deployment fee economy, rapid ledger deserialization, and minimal footprint storage on the Stellar ledger, the release contract build is constrained to strict size budgets:

| Artifact | Optimization Target | Max Hard Limit |
| :--- | :--- | :--- |
| `starfund_escrow.wasm` | $< 50\text{ KB}$ | $64\text{ KB}$ (65,536 bytes) |

CI automatically compiles `starfund_escrow` with target `wasm32v1-none` under `--release` and enforces that the uncompressed bytecode remains strictly under the $64\text{ KB}$ ceiling.

---

## 3. Reproducible Build & Verification Procedure (Issue #361)
Community members, auditors, and node operators can verify that the on-chain WASM matches the open-source repository code by following these steps:

### Step 1: Install Pinned Toolchain
```bash
rustup target add wasm32v1-none
cargo install --locked stellar-cli --version 22.0.1
```

### Step 2: Build Release WASM
```bash
cargo build --target wasm32v1-none --release -p starfund_escrow
```

### Step 3: Compute SHA-256 Checksum
```bash
sha256sum target/wasm32v1-none/release/starfund_escrow.wasm
```

### Step 4: Verify Against On-Chain Ledger Hash
Compare the resulting 32-byte hexadecimal hash against the installed contract WASM hash obtained from Soroban RPC:
```bash
stellar contract inspect --wasm target/wasm32v1-none/release/starfund_escrow.wasm
```
If the computed checksum matches the ledger's installed hash, the artifact is cryptographically verified.

---

## 4. Documentation & Coverage Diagnostic Gates (Issues #358, #360)
- **Rustdoc Warning Gate**: All public types, functions, and invariants must be fully documented with zero warnings (`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`).
- **Machine-Readable Coverage**: CI publishes code coverage diagnostics in JSON and LCOV formats via `cargo-llvm-cov` to track regression-free test coverage.
