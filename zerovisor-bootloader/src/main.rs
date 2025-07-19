#![no_main]
#![no_std]

use uefi::prelude::*;
use uefi::proto::console::text::Output;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{File, FileMode, FileAttribute};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::table::boot::{MemoryType, AllocateType};
use uefi::{CStr16, Handle};

mod memory;
mod loader;

use memory::MemoryMap;
use loader::HypervisorLoader;

#[entry]
fn main(image_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    uefi_services::init(&mut system_table).unwrap();
    
    let stdout = system_table.stdout();
    
    // Print welcome message
    stdout.clear().unwrap();
    stdout.output_string(cstr16!("Zerovisor UEFI Bootloader v0.1.0\r\n")).unwrap();
    stdout.output_string(cstr16!("Initializing hypervisor...\r\n")).unwrap();
    
    // Initialize memory management
    let memory_map = match MemoryMap::new(&system_table) {
        Ok(map) => {
            stdout.output_string(cstr16!("Memory map initialized\r\n")).unwrap();
            map
        }
        Err(_) => {
            stdout.output_string(cstr16!("Failed to initialize memory map\r\n")).unwrap();
            return Status::ABORTED;
        }
    };
    
    // Load hypervisor binary
    let mut loader = match HypervisorLoader::new(&system_table) {
        Ok(loader) => {
            stdout.output_string(cstr16!("Hypervisor loader initialized\r\n")).unwrap();
            loader
        }
        Err(_) => {
            stdout.output_string(cstr16!("Failed to initialize hypervisor loader\r\n")).unwrap();
            return Status::ABORTED;
        }
    };
    
    // Load the hypervisor kernel
    let hypervisor_entry = match loader.load_hypervisor(cstr16!("\\zerovisor.efi")) {
        Ok(entry) => {
            stdout.output_string(cstr16!("Hypervisor loaded successfully\r\n")).unwrap();
            entry
        }
        Err(_) => {
            stdout.output_string(cstr16!("Failed to load hypervisor\r\n")).unwrap();
            return Status::ABORTED;
        }
    };
    
    stdout.output_string(cstr16!("Exiting boot services...\r\n")).unwrap();
    
    // Exit boot services and transfer control to hypervisor
    let (_runtime_table, memory_map) = system_table
        .exit_boot_services(image_handle, &mut memory_map.buffer)
        .unwrap();
    
    // Jump to hypervisor entry point
    unsafe {
        let entry_fn: extern "C" fn(*const u8, usize) -> ! = 
            core::mem::transmute(hypervisor_entry);
        entry_fn(memory_map.as_ptr(), memory_map.len());
    }
}