#!/usr/bin/env bash
# QEMU/KVM launch helper for Zerovisor (Task 15.2)
# Builds the bootloader binary and runs it under QEMU with KVM acceleration.

set -euo pipefail

PROJECT_ROOT=$(cd "$(dirname "$0")/.." && pwd)
TARGET_DIR="$PROJECT_ROOT/target/x86_64-unknown-none/debug"

# Build Zerovisor bootloader
cargo build --workspace --package zerovisor-bootloader --target x86_64-unknown-none

KERNEL_BIN="$TARGET_DIR/zerovisor-bootloader"
if [ ! -f "$KERNEL_BIN" ]; then
  echo "Bootloader binary not found: $KERNEL_BIN" >&2
  exit 1
fi

# Launch QEMU with minimal devices; serial output mapped to stdio.
exec qemu-system-x86_64 \
  -cpu host \
  -machine q35,accel=kvm \
  -smp 4 \
  -m 1024M \
  -kernel "$KERNEL_BIN" \
  -serial mon:stdio \
  -display none \
  -no-reboot 