#!/usr/bin/env bash
# Reproducible local validation entrypoint (Issue #355)
set -euo pipefail

echo "=== 1. Checking Rust code formatting ==="
cargo fmt -p starfund_escrow -- --check

echo "=== 2. Running Clippy linter ==="
cargo clippy -p starfund_escrow -- -D warnings

echo "=== 3. Running unit & integration tests ==="
cargo test

echo "=== 4. Checking Markdown documentation links ==="
if [ -f "scripts/check-markdown-links.js" ]; then
  node scripts/check-markdown-links.js
fi

echo "=== Local validation passed successfully! ==="
