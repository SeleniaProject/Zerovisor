#![cfg(test)]

use zerovisor_bootloader::{MemoryMap, HypervisorLoader};

#[test]
fn test_bootloader_modules_compile() {
    // This test ensures that the bootloader modules compile correctly
    // Actual UEFI testing would require a UEFI environment
    assert!(true);
}

#[test] 
fn test_memory_map_structure() {
    // Test that MemoryMap has the expected structure
    // This is a compile-time test to ensure the struct is properly defined
    let _test_fn = |buffer: Vec<u8>| {
        let _map = MemoryMap {
            buffer,
            map_size: 0,
            descriptor_size: 0,
            descriptor_version: 0,
        };
    };
    assert!(true);
}