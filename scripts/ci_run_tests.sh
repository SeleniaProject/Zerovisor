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
# Run tests with formal verification and Coq proof stubs enabled as a separate pass
PROPTEST_CASES=256 cargo test --all --features "formal_verification coq_proofs" --verbose

# ---------------------------------------------------------------------------
# Formal verification stage
# ---------------------------------------------------------------------------

# 1. TLA+ model checks – requires `tlapm` tool (TLAPS). Skipped if not present.
if command -v tlapm &>/dev/null; then
  echo "Running TLA+ checks..."
  for spec in formal_specs/*.tla; do
    echo "Checking ${spec}"
    tlapm --toolbox "$spec"
  done
else
  echo "tlapm not found; skipping TLA+ checks"
fi

# 2. Coq proofs – compile any .v files when Coq is available.
if command -v coqc &>/dev/null; then
  echo "Building Coq proofs..."
  find formal_specs -name '*.v' -print0 | xargs -0 -I{} coqc {}
else
  echo "coqc not found; skipping Coq proof compilation"
fi
# Doc pass
cargo doc --no-deps --workspace 