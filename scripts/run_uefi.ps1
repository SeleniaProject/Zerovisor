# Zerovisor UEFI boot helper (Task 15.2)
# Builds the UEFI image and copies it to removable media.
param(
    [string]$Device = "E:"
)

Write-Host "Building UEFI bootloader..."
cargo build --workspace --package zerovisor-bootloader --release --target x86_64-unknown-uefi

$efiBin = "target/x86_64-unknown-uefi/release/zerovisor-bootloader.efi"
if (-Not (Test-Path $efiBin)) {
    Write-Error "EFI binary not found: $efiBin"
    exit 1
}

$dest = Join-Path $Device "EFI\BOOT\BOOTX64.EFI"
Write-Host "Copying to $dest"
Copy-Item $efiBin $dest -Force 