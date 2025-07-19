#![no_std]

//! Zerovisor UEFI Bootloader
//! 
//! This module provides UEFI bootloader functionality for the Zerovisor hypervisor.
//! It handles system initialization, memory management, and hypervisor loading.

pub mod memory;
pub mod loader;

pub use memory::MemoryMap;
pub use loader::HypervisorLoader;