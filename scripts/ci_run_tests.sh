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

# Enforce presence of formal verification toolchain.
if ! command -v tlapm &>/dev/null; then
  echo "ERROR: tlapm (TLA+ Proof Manager) not found. Formal verification required." >&2
  exit 1
fi

# 1. TLA+ model checks (fail on any error).
echo "Running TLA+ checks..."
for spec in formal_specs/*.tla; do
  echo "Checking ${spec}"
  tlapm --toolbox "$spec"
done

# Enforce Coq availability.
if ! command -v coqc &>/dev/null; then
  echo "ERROR: coqc (Coq compiler) not found. Formal proofs must pass." >&2
  exit 1
fi

# 2. Coq proof compilation (fail on error).
echo "Building Coq proofs..."
find formal_specs -name '*.v' -print0 | xargs -0 -I{} coqc {}
# Doc pass
cargo doc --no-deps --workspace 

# 3. Apalache model checker (preferred for large state spaces). Uses Docker image if local binary missing.
if command -v apalache-mc &>/dev/null; then
  echo "Running Apalache checks..."
  for spec in formal_specs/*.tla; do
    echo "Apalache checking ${spec}"
    apalache-mc check --inv=Inv "${spec}"
  done
else
  echo "apalache-mc not found; attempting Docker execution"
  if command -v docker &>/dev/null; then
    for spec in $(ls formal_specs/*.tla); do
      echo "Docker Apalache checking ${spec}"
      docker run --rm -v "${PWD}":/workspace apalache/mc:latest check --inv=Inv "/workspace/${spec}"
    done
  else
    echo "Apalache unavailable; skipping advanced model checking"
  fi
fi
# --------------------------------------------------------------------------- 