# Zerovisor – UEFI Bootstrap Build Guide

This guide describes how to build the minimal UEFI application for Zerovisor on Windows.

## Prerequisites

- Rust toolchain (rustup) installed
- Nightly is NOT required; stable is sufficient

## Setup

1. Install the UEFI target:
   - PowerShell:
     ```powershell
     rustup target add x86_64-unknown-uefi
     ```

2. Build the EFI binary:
   ```powershell
   cargo build --release --target x86_64-unknown-uefi
   ```

3. Locate the output:
   - The produced file is a PE/COFF image suitable for UEFI:
     - `target/x86_64-unknown-uefi/release/zerovisor.efi`

## Run (QEMU + OVMF example)

The exact steps depend on your local environment. One common approach is:

1. Prepare a FAT image (e.g., `fat.img`) and create the UEFI boot path:
   - `EFI/BOOT/BOOTX64.EFI`
2. Copy `zerovisor.efi` to `EFI/BOOT/BOOTX64.EFI` inside the FAT image.
3. Launch QEMU with OVMF firmware and the FAT image attached as a drive.

> Note: Firmware, paths, and command lines vary by installation. If you use a physical USB stick, format it as FAT32 and create the same `EFI/BOOT/BOOTX64.EFI` path.

## Notes

- The bootstrap prints a short banner to the UEFI text console. If you do not see any output, verify that your firmware console is enabled and that the file was placed under the standard removable media path (`EFI/BOOT/BOOTX64.EFI`).
- Subsequent milestones will add CPUID/MSR probing, logging facilities, and i18n.


