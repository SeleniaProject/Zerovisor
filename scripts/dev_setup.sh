#!/usr/bin/env bash
# Zerovisor developer environment bootstrap (Task 16.2)
# Installs required Rust toolchains and tooling.
set -euo pipefail

if ! command -v rustup >/dev/null; then
  echo "Rustup not found. Please install Rust from https://rustup.rs first." >&2
  exit 1
fi

echo "Installing nightly toolchain & components..."
rustup toolchain install nightly --component rust-src clippy rustfmt
rustup default nightly

echo "Adding cargo-binutils & other helpers..."
cargo install cargo-binutils --locked
rustup component add llvm-tools-preview

# Install wasm32 target for building SDK wasm bindings (optional)
rustup target add wasm32-wasi

echo "Installing pre-commit hooks (rustfmt + clippy)..."
cat <<'HOOK' > .git/hooks/pre-commit
#!/usr/bin/env bash
set -e
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
HOOK
chmod +x .git/hooks/pre-commit

echo "Developer environment ready. Run 'cargo test --all' to verify." 