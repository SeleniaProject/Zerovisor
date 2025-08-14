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

   Optional feature flags:

   - Enable VirtIO-net writer integration (migration via virtio queue):
     ```powershell
     cargo build --release --target x86_64-unknown-uefi --features "virtio-net"
     ```

   - Enable UEFI SNP (Simple Network Protocol) writer integration:
     ```powershell
     cargo build --release --target x86_64-unknown-uefi --features "snp"
     ```

   - Combine features:
     ```powershell
     cargo build --release --target x86_64-unknown-uefi --features "virtio-net snp"
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

### Windows (PowerShell) quick run (directory FAT mapping)

If your QEMU supports mapping a host directory as a FAT drive, you can avoid creating an image.

```powershell
# Place the built binary into a directory following the UEFI removable media path
$BootDir = Join-Path $PWD "out-efi/EFI/BOOT"
New-Item -ItemType Directory -Force -Path $BootDir | Out-Null
Copy-Item "target/x86_64-unknown-uefi/release/zerovisor.efi" (Join-Path $BootDir "BOOTX64.EFI") -Force

# Adjust OVMF paths to your local installation
$OVMF_CODE = "C:/Program Files/qemu/OVMF_CODE.fd"
$OVMF_VARS = "C:/Program Files/qemu/OVMF_VARS.fd"

qemu-system-x86_64 `
  -machine q35,accel=tcg `
  -cpu host `
  -m 1024 `
  -drive if=pflash,format=raw,unit=0,readonly=on,file="$OVMF_CODE" `
  -drive if=pflash,format=raw,unit=1,file="$OVMF_VARS" `
  -drive format=raw,file=fat:rw:$(Join-Path $PWD "out-efi")
```

Notes:

- Replace OVMF paths with the ones installed on your machine.
- For VirtIO testing, add `-device virtio-net-pci,netdev=n0 -netdev user,id=n0` (or your preferred netdev).


> Note: Firmware, paths, and command lines vary by installation. If you use a physical USB stick, format it as FAT32 and create the same `EFI/BOOT/BOOTX64.EFI` path.

## Quick self-test (IOMMU)

Once Zerovisor boots and the CLI prompt (`> `) appears on the UEFI console, you can run a conservative VT-d self-test to validate minimal setup:

```text
iommu                 # probe & report VT-d/AMD-Vi and DMAR/IVRS summaries
iommu plan            # print planned context assignments from current state
iommu apply-safe      # disable TE, apply contexts+mappings, enable TE
iommu selftest quick  # run plan/apply/verify/(invalidate) and sample walk/xlate
```

The self-test command accepts options:

- `quick`: uses a compact apply path.
- `no-apply`: skip re-applying contexts/mappings.
- `no-inv`: skip global invalidate at the end.
- `dom=<id>`: restrict sampling to a specific domain.
- `walk=<n>` / `xlate=<n>`: number of walk/translate samples.

You can also sample translations/walks across all BDFs in a domain:

```text
iommu sample dom=<id> iova=<hex> [count=<n>] [walk] [xlate]
```

For quick end-to-end setup and validation, use:

```text
iommu quick          # plan -> apply-safe -> verify -> verify-map -> invalidate
```

Persist/restore IOMMU assignments across boots via UEFI variables:

```text
iommu cfg save       # save current seg:bus:dev.func -> domain assignments
iommu cfg load       # restore and re-apply, then refresh VT-d caches
```

## Notes

- The bootstrap prints a short banner to the UEFI text console. If you do not see any output, verify that your firmware console is enabled and that the file was placed under the standard removable media path (`EFI/BOOT/BOOTX64.EFI`).
- Subsequent milestones will add CPUID/MSR probing, logging facilities, and i18n.


