# Zerovisor Operations Guide

This document provides installation, configuration and maintenance instructions for running Zerovisor in production environments.

## 1. Installation

### Prerequisites

- x86_64 host CPU with VT-x/VT-d or AMD-V/SVM support
- Minimum 4 GB RAM, 1 GB reserved for host
- Linux kernel ≥ 5.15 with KVM enabled, or bare-metal UEFI firmware
- Rust toolchain (nightly) and cargo-binutils for building from source
- QEMU ≥ 8.0 (for virtual test deployments)

### Build

```bash
# Clone repository
$ git clone https://github.com/SeleniaProject/zerovisor.git
$ cd zerovisor

# Build hypervisor and bootloader
$ cargo build --release --workspace
```

### Deploy to QEMU/KVM

```bash
$ ./scripts/qemu_run.sh
```

Serial console will appear on standard input/output. Use `Ctrl+A X` to quit.

### Deploy to Physical Machine (UEFI)

1. Insert FAT32 USB drive (label: `ZVI_BOOT`).
2. Run PowerShell script as Administrator:

```powershell
PS> ./scripts/run_uefi.ps1 -Device E:
```

3. Reboot target machine, select USB device in UEFI boot menu.

## 2. Configuration

All runtime parameters are provided via a JSON file embedded in the bootloader EFI partition (`/EFI/ZEROVISOR/config.json`). Example:

```json
{
  "serial": 115200,
  "log_level": "info",
  "initial_vms": [
    {
      "image": "aetheros.img",
      "memory": 104857600,
      "cpus": 2,
      "type": "MicroVm"
    }
  ]
}
```

## 3. Monitoring & Debugging

- Map physical address of metrics page (printed during boot) in host OS to read real-time counters.
- Use GDB remote stub: `target remote :1234` after `gdbstub: waiting for connection` message.
- Logs are sent to both serial and memory-mapped console buffer at `0xB8000`.

## 4. Maintenance

### Firmware Update

```bash
$ git pull --rebase
$ cargo build --release --workspace
$ ./scripts/run_uefi.ps1 -Device E:\
```

### Crash Recovery

If hardware fault detected (`ha::hw_fault()`), Zerovisor triggers automatic fail-over. Review crash dumps in `/var/log/zerovisor/` on surviving nodes.

## 5. Troubleshooting

| Symptom | Resolution |
|---------|------------|
| No serial output | Ensure `-serial mon:stdio` or USB-TTL adapter connected |
| `VMX not supported` | Enable virtualization in BIOS/UEFI |
| QEMU exits immediately | Check bootloader build architecture matches QEMU args |
| Guest OS panic | Consult guest console, verify EPT mapping flags | 