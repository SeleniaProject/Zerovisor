#!/usr/bin/env bash
# Run Zerovisor exhaustive test suite under host `std` environment.
# Intended for CI pipelines (GitHub Actions, GitLab CI, etc.).
#
# 1. Build all crates with default features enabled.
# 2. Run cargo test including proptest with max_cases=256.
# 3. Compile docs to ensure rustdoc passes.
#
# Requires: rustup toolchain + nightly for no_std test harness.
set -euo pipefail

export RUST_BACKTRACE=1

# Build
cargo build --all --verbose

# Test (limit proptest cases for time)
PROPTEST_CASES=256 cargo test --all --verbose

# Doc pass
cargo doc --no-deps --workspace 