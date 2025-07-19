use uefi::prelude::*;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{File, FileMode, FileAttribute};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::table::boot::{MemoryType, AllocateType};
use uefi::{CStr16, Handle};

/// Hypervisor loader for loading the main hypervisor binary
pub struct HypervisorLoader<'a> {
    system_table: &'a SystemTable<Boot>,
    file_system: &'a mut SimpleFileSystem,
}

impl<'a> HypervisorLoader<'a> {
    /// Create a new hypervisor loader
    pub fn new(system_table: &'a SystemTable<Boot>) -> Result<Self, uefi::Error> {
        let boot_services = system_table.boot_services();
        
        // Get the loaded image protocol
        let loaded_image = boot_services
            .open_protocol_exclusive::<LoadedImage>(boot_services.image_handle())
            .map_err(|e| e.status())?;
        
        // Get the simple file system protocol
        let mut file_system = boot_services
            .open_protocol_exclusive::<SimpleFileSystem>(loaded_image.device())
            .map_err(|e| e.status())?;
        
        Ok(HypervisorLoader {
            system_table,
            file_system: &mut file_system,
        })
    }
    
    /// Load the hypervisor binary from the specified path
    pub fn load_hypervisor(&mut self, path: &CStr16) -> Result<u64, uefi::Error> {
        let boot_services = self.system_table.boot_services();
        
        // Open the root directory
        let mut root_dir = self.file_system
            .open_volume()
            .map_err(|e| e.status())?;
        
        // Open the hypervisor file
        let mut file_handle = root_dir
            .open(path, FileMode::Read, FileAttribute::empty())
            .map_err(|e| e.status())?
            .into_regular_file()
            .ok_or(Status::NOT_FOUND)?;
        
        // Get file size
        let file_info = file_handle
            .get_info::<uefi::proto::media::file::FileInfo>(&mut [0u8; 512])
            .map_err(|e| e.status())?;
        
        let file_size = file_info.file_size() as usize;
        
        // Allocate memory for the hypervisor
        let pages = (file_size + 4095) / 4096; // Round up to page boundary
        let hypervisor_addr = boot_services
            .allocate_pages(
                AllocateType::AnyPages,
                MemoryType::LOADER_DATA,
                pages,
            )
            .map_err(|e| e.status())?;
        
        // Read the hypervisor binary into memory
        let buffer = unsafe {
            core::slice::from_raw_parts_mut(hypervisor_addr as *mut u8, file_size)
        };
        
        file_handle
            .read(buffer)
            .map_err(|e| e.status())?;
        
        // Parse PE/COFF header to find entry point
        let entry_point = self.parse_pe_entry_point(buffer)?;
        
        Ok(hypervisor_addr + entry_point)
    }
    
    /// Parse PE/COFF header to extract entry point
    fn parse_pe_entry_point(&self, buffer: &[u8]) -> Result<u64, uefi::Error> {
        // Basic PE/COFF parsing
        if buffer.len() < 64 {
            return Err(Status::INVALID_PARAMETER.into());
        }
        
        // Check DOS header signature
        if &buffer[0..2] != b"MZ" {
            return Err(Status::INVALID_PARAMETER.into());
        }
        
        // Get PE header offset
        let pe_offset = u32::from_le_bytes([
            buffer[60], buffer[61], buffer[62], buffer[63]
        ]) as usize;
        
        if buffer.len() < pe_offset + 24 {
            return Err(Status::INVALID_PARAMETER.into());
        }
        
        // Check PE signature
        if &buffer[pe_offset..pe_offset + 4] != b"PE\0\0" {
            return Err(Status::INVALID_PARAMETER.into());
        }
        
        // Get entry point RVA from optional header
        let optional_header_offset = pe_offset + 24;
        if buffer.len() < optional_header_offset + 16 {
            return Err(Status::INVALID_PARAMETER.into());
        }
        
        let entry_point_rva = u32::from_le_bytes([
            buffer[optional_header_offset + 16],
            buffer[optional_header_offset + 17],
            buffer[optional_header_offset + 18],
            buffer[optional_header_offset + 19],
        ]);
        
        Ok(entry_point_rva as u64)
    }
}