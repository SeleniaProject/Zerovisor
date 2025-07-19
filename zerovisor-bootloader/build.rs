use std::env;
use std::path::PathBuf;

fn main() {
    // Set target-specific linker script for UEFI
    let target = env::var("TARGET").unwrap();
    
    if target.contains("x86_64") && target.contains("uefi") {
        println!("cargo:rustc-link-arg=/SUBSYSTEM:EFI_APPLICATION");
        println!("cargo:rustc-link-arg=/ENTRY:efi_main");
    }
    
    // Rerun if build script changes
    println!("cargo:rerun-if-changed=build.rs");
}