//! AMD SVM (Secure Virtual Machine) support for x86_64
//!
//! This module provides low-level wrappers around AMD SVM instructions
//! and structures including VMCB (Virtual Machine Control Block) management.

#![cfg(target_arch = "x86_64")]

use core::arch::asm;
use core::marker::PhantomData;
use crate::memory::PhysicalAddress;
use crate::{vec, Vec};

/// AMD SVM VMCB (Virtual Machine Control Block) field offsets
/// Based on AMD64 Architecture Programmer's Manual Volume 2
pub mod vmcb_offsets {
    // Control Area (offset 0x000 - 0x3FF)
    pub const INTERCEPT_CR_READ: usize = 0x000;
    pub const INTERCEPT_CR_WRITE: usize = 0x002;
    pub const INTERCEPT_DR_READ: usize = 0x004;
    pub const INTERCEPT_DR_WRITE: usize = 0x006;
    pub const INTERCEPT_EXCEPTION: usize = 0x008;
    pub const INTERCEPT_INSTR1: usize = 0x00C;
    pub const INTERCEPT_INSTR2: usize = 0x010;
    pub const INTERCEPT_INSTR3: usize = 0x014;
    pub const PAUSE_FILTER_THRESHOLD: usize = 0x03C;
    pub const PAUSE_FILTER_COUNT: usize = 0x03E;
    pub const IOPM_BASE_PA: usize = 0x040;
    pub const MSRPM_BASE_PA: usize = 0x048;
    pub const TSC_OFFSET: usize = 0x050;
    pub const GUEST_ASID: usize = 0x058;
    pub const TLB_CONTROL: usize = 0x05C;
    pub const VINTR: usize = 0x060;
    pub const INTERRUPT_SHADOW: usize = 0x068;
    pub const EXITCODE: usize = 0x070;
    pub const EXITINFO1: usize = 0x078;
    pub const EXITINFO2: usize = 0x080;
    pub const EXITINTINFO: usize = 0x088;
    pub const NP_ENABLE: usize = 0x090;
    pub const AVIC_APIC_BAR: usize = 0x098;
    pub const GHCB_PA: usize = 0x0A0;
    pub const EVENTINJ: usize = 0x0A8;
    pub const N_CR3: usize = 0x0B0;
    pub const LBR_VIRTUALIZATION_ENABLE: usize = 0x0B8;
    pub const VMCB_CLEAN: usize = 0x0C0;
    pub const NRIP: usize = 0x0C8;
    pub const GUEST_INST_BYTES: usize = 0x0D0;
    pub const AVIC_APIC_BACKING_PAGE_PTR: usize = 0x0E0;
    pub const AVIC_LOGICAL_TABLE_PTR: usize = 0x0F0;
    pub const AVIC_PHYSICAL_TABLE_PTR: usize = 0x0F8;
    
    // Save State Area (offset 0x400 - 0x5FF)
    pub const GUEST_ES_SELECTOR: usize = 0x400;
    pub const GUEST_ES_ATTRIB: usize = 0x402;
    pub const GUEST_ES_LIMIT: usize = 0x404;
    pub const GUEST_ES_BASE: usize = 0x408;
    
    pub const GUEST_CS_SELECTOR: usize = 0x410;
    pub const GUEST_CS_ATTRIB: usize = 0x412;
    pub const GUEST_CS_LIMIT: usize = 0x414;
    pub const GUEST_CS_BASE: usize = 0x418;
    
    pub const GUEST_SS_SELECTOR: usize = 0x420;
    pub const GUEST_SS_ATTRIB: usize = 0x422;
    pub const GUEST_SS_LIMIT: usize = 0x424;
    pub const GUEST_SS_BASE: usize = 0x428;
    
    pub const GUEST_DS_SELECTOR: usize = 0x430;
    pub const GUEST_DS_ATTRIB: usize = 0x432;
    pub const GUEST_DS_LIMIT: usize = 0x434;
    pub const GUEST_DS_BASE: usize = 0x438;
    
    pub const GUEST_FS_SELECTOR: usize = 0x440;
    pub const GUEST_FS_ATTRIB: usize = 0x442;
    pub const GUEST_FS_LIMIT: usize = 0x444;
    pub const GUEST_FS_BASE: usize = 0x448;
    
    pub const GUEST_GS_SELECTOR: usize = 0x450;
    pub const GUEST_GS_ATTRIB: usize = 0x452;
    pub const GUEST_GS_LIMIT: usize = 0x454;
    pub const GUEST_GS_BASE: usize = 0x458;
    
    pub const GUEST_GDTR_SELECTOR: usize = 0x460;
    pub const GUEST_GDTR_ATTRIB: usize = 0x462;
    pub const GUEST_GDTR_LIMIT: usize = 0x464;
    pub const GUEST_GDTR_BASE: usize = 0x468;
    
    pub const GUEST_LDTR_SELECTOR: usize = 0x470;
    pub const GUEST_LDTR_ATTRIB: usize = 0x472;
    pub const GUEST_LDTR_LIMIT: usize = 0x474;
    pub const GUEST_LDTR_BASE: usize = 0x478;
    
    pub const GUEST_IDTR_SELECTOR: usize = 0x480;
    pub const GUEST_IDTR_ATTRIB: usize = 0x482;
    pub const GUEST_IDTR_LIMIT: usize = 0x484;
    pub const GUEST_IDTR_BASE: usize = 0x488;
    
    pub const GUEST_TR_SELECTOR: usize = 0x490;
    pub const GUEST_TR_ATTRIB: usize = 0x492;
    pub const GUEST_TR_LIMIT: usize = 0x494;
    pub const GUEST_TR_BASE: usize = 0x498;
    
    // Control registers and other state
    pub const GUEST_CR0: usize = 0x500;
    pub const GUEST_CR2: usize = 0x508;
    pub const GUEST_CR3: usize = 0x510;
    pub const GUEST_CR4: usize = 0x518;
    pub const GUEST_DR6: usize = 0x520;
    pub const GUEST_DR7: usize = 0x528;
    pub const GUEST_RFLAGS: usize = 0x530;
    pub const GUEST_RIP: usize = 0x538;
    pub const GUEST_RSP: usize = 0x5D8;
    pub const GUEST_RAX: usize = 0x5F8;
}

/// SVM exit reasons
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvmExitReason {
    CpuId,
    Hlt,
    IoInstruction,
    NestedPageFault,
    Vmmcall,
    Unknown(u64),
}

/// VMCB (Virtual Machine Control Block) errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvmError {
    NotSupported,
    SvmNotEnabled,
    VmcbAllocFailed,
    InvalidVmcb,
    NestedPagingSetupFailed,
    Failure,
}

/// Assembly wrapper for VMRUN instruction
unsafe fn vmrun_asm(vmcb_pa: u64) -> u64 {
    let exit_code: u64;
    unsafe {
        asm!(
            "vmrun",
            in("rax") vmcb_pa,
            out("rax") exit_code,
            clobber_abi("C")
        );
    }
    exit_code
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmcbError {
    NotSupported,
    AllocationFailed,
    VmrunFailed,
    InvalidState,
    HardwareError,
}

/// SVM Exit Codes
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvmExitCode {
    CR0_READ = 0x000,
    CR1_READ = 0x001,
    CR2_READ = 0x002,
    CR3_READ = 0x003,
    CR4_READ = 0x004,
    CR5_READ = 0x005,
    CR6_READ = 0x006,
    CR7_READ = 0x007,
    CR8_READ = 0x008,
    CR9_READ = 0x009,
    CR10_READ = 0x00A,
    CR11_READ = 0x00B,
    CR12_READ = 0x00C,
    CR13_READ = 0x00D,
    CR14_READ = 0x00E,
    CR15_READ = 0x00F,
    CR0_WRITE = 0x010,
    CR1_WRITE = 0x011,
    CR2_WRITE = 0x012,
    CR3_WRITE = 0x013,
    CR4_WRITE = 0x014,
    CR5_WRITE = 0x015,
    CR6_WRITE = 0x016,
    CR7_WRITE = 0x017,
    CR8_WRITE = 0x018,
    CR9_WRITE = 0x019,
    CR10_WRITE = 0x01A,
    CR11_WRITE = 0x01B,
    CR12_WRITE = 0x01C,
    CR13_WRITE = 0x01D,
    CR14_WRITE = 0x01E,
    CR15_WRITE = 0x01F,
    DR0_READ = 0x020,
    DR1_READ = 0x021,
    DR2_READ = 0x022,
    DR3_READ = 0x023,
    DR4_READ = 0x024,
    DR5_READ = 0x025,
    DR6_READ = 0x026,
    DR7_READ = 0x027,
    DR8_READ = 0x028,
    DR9_READ = 0x029,
    DR10_READ = 0x02A,
    DR11_READ = 0x02B,
    DR12_READ = 0x02C,
    DR13_READ = 0x02D,
    DR14_READ = 0x02E,
    DR15_READ = 0x02F,
    EXCEPTION_DE = 0x040,
    EXCEPTION_DB = 0x041,
    EXCEPTION_NMI = 0x042,
    EXCEPTION_BP = 0x043,
    EXCEPTION_OF = 0x044,
    EXCEPTION_BR = 0x045,
    EXCEPTION_UD = 0x046,
    EXCEPTION_NM = 0x047,
    EXCEPTION_DF = 0x048,
    EXCEPTION_TS = 0x04A,
    EXCEPTION_NP = 0x04B,
    EXCEPTION_SS = 0x04C,
    EXCEPTION_GP = 0x04D,
    EXCEPTION_PF = 0x04E,
    EXCEPTION_MF = 0x050,
    EXCEPTION_AC = 0x051,
    EXCEPTION_MC = 0x052,
    EXCEPTION_XF = 0x053,
    INTR = 0x060,
    NMI = 0x061,
    SMI = 0x062,
    INIT = 0x063,
    VINTR = 0x064,
    CR0_SEL_WRITE = 0x065,
    IDTR_READ = 0x066,
    GDTR_READ = 0x067,
    LDTR_READ = 0x068,
    TR_READ = 0x069,
    IDTR_WRITE = 0x06A,
    GDTR_WRITE = 0x06B,
    LDTR_WRITE = 0x06C,
    TR_WRITE = 0x06D,
    RDTSC = 0x06E,
    RDPMC = 0x06F,
    PUSHF = 0x070,
    POPF = 0x071,
    CPUID = 0x072,
    RSM = 0x073,
    IRET = 0x074,
    SWINT = 0x075,
    INVD = 0x076,
    PAUSE = 0x077,
    HLT = 0x078,
    INVLPG = 0x079,
    INVLPGA = 0x07A,
    IOIO = 0x07B,
    MSR = 0x07C,
    TASK_SWITCH = 0x07D,
    FERR_FREEZE = 0x07E,
    SHUTDOWN = 0x07F,
    VMRUN = 0x080,
    VMMCALL = 0x081,
    VMLOAD = 0x082,
    VMSAVE = 0x083,
    STGI = 0x084,
    CLGI = 0x085,
    SKINIT = 0x086,
    RDTSCP = 0x087,
    ICEBP = 0x088,
    WBINVD = 0x089,
    MONITOR = 0x08A,
    MWAIT = 0x08B,
    MWAIT_CONDITIONAL = 0x08C,
    XSETBV = 0x08D,
    NPF = 0x400,
    AVIC_INCOMPLETE_IPI = 0x401,
    AVIC_NOACCEL = 0x402,
    VMGEXIT = 0x403,
    INVALID = 0xFFFFFFFFFFFFFFFF,
}

impl From<u64> for SvmExitCode {
    fn from(value: u64) -> Self {
        match value {
            0x000 => SvmExitCode::CR0_READ,
            0x072 => SvmExitCode::CPUID,
            0x078 => SvmExitCode::HLT,
            0x07B => SvmExitCode::IOIO,
            0x07C => SvmExitCode::MSR,
            0x400 => SvmExitCode::NPF,
            _ => SvmExitCode::INVALID,
        }
    }
}

/// Wrapper representing a loaded VMCB pointer
pub struct ActiveVmcb<'a> {
    vmcb_pa: PhysicalAddress,
    _phantom: PhantomData<&'a mut ()>,
}

impl<'a> ActiveVmcb<'a> {
    /// Read a field from the VMCB
    #[inline]
    pub fn read_u64(&self, offset: usize) -> u64 {
        unsafe {
            let vmcb_va = self.vmcb_pa as *const u8;
            core::ptr::read_volatile(vmcb_va.add(offset) as *const u64)
        }
    }

    /// Write a field to the VMCB
    #[inline]
    pub fn write_u64(&mut self, offset: usize, value: u64) {
        unsafe {
            let vmcb_va = self.vmcb_pa as *mut u8;
            core::ptr::write_volatile(vmcb_va.add(offset) as *mut u64, value);
        }
    }

    /// Read a 32-bit field from the VMCB
    #[inline]
    pub fn read_u32(&self, offset: usize) -> u32 {
        unsafe {
            let vmcb_va = self.vmcb_pa as *const u8;
            core::ptr::read_volatile(vmcb_va.add(offset) as *const u32)
        }
    }

    /// Write a 32-bit field to the VMCB
    #[inline]
    pub fn write_u32(&mut self, offset: usize, value: u32) {
        unsafe {
            let vmcb_va = self.vmcb_pa as *mut u8;
            core::ptr::write_volatile(vmcb_va.add(offset) as *mut u32, value);
        }
    }

    /// Read a 16-bit field from the VMCB
    #[inline]
    pub fn read_u16(&self, offset: usize) -> u16 {
        unsafe {
            let vmcb_va = self.vmcb_pa as *const u8;
            core::ptr::read_volatile(vmcb_va.add(offset) as *const u16)
        }
    }

    /// Write a 16-bit field to the VMCB
    #[inline]
    pub fn write_u16(&mut self, offset: usize, value: u16) {
        unsafe {
            let vmcb_va = self.vmcb_pa as *mut u8;
            core::ptr::write_volatile(vmcb_va.add(offset) as *mut u16, value);
        }
    }
}

/// Safe wrapper representing ownership of a VMCB region in physical memory
pub struct Vmcb {
    phys_addr: PhysicalAddress,
}

impl Vmcb {
    /// Create a new wrapper from a 4-KiB-aligned physical address
    pub const fn new(phys: PhysicalAddress) -> Self { 
        Self { phys_addr: phys } 
    }

    /// Load this VMCB, returning an ActiveVmcb token that allows read/write
    pub fn load(&self) -> Result<ActiveVmcb, VmcbError> {
        // Validate VMCB alignment
        if self.phys_addr & 0xFFF != 0 {
            return Err(VmcbError::InvalidState);
        }
        
        Ok(ActiveVmcb { 
            vmcb_pa: self.phys_addr,
            _phantom: PhantomData 
        })
    }

    /// Physical address of VMCB region
    pub fn phys_addr(&self) -> PhysicalAddress { 
        self.phys_addr 
    }
}

/// Complete VMCB state structure for comprehensive management
#[repr(C, align(4096))]
pub struct VmcbState {
    // Control Area (0x000-0x3FF)
    pub intercept_cr_read: u16,
    pub intercept_cr_write: u16,
    pub intercept_dr_read: u16,
    pub intercept_dr_write: u16,
    pub intercept_exception: u32,
    pub intercept_instr1: u32,
    pub intercept_instr2: u32,
    pub intercept_instr3: u32,
    pub pause_filter_threshold: u16,
    pub pause_filter_count: u16,
    pub iopm_base_pa: u64,
    pub msrpm_base_pa: u64,
    pub tsc_offset: u64,
    pub guest_asid: u32,
    pub tlb_control: u8,
    pub vintr: u64,
    pub interrupt_shadow: u64,
    pub exitcode: u64,
    pub exitinfo1: u64,
    pub exitinfo2: u64,
    pub exitintinfo: u64,
    pub np_enable: u64,
    pub avic_apic_bar: u64,
    pub ghcb_pa: u64,
    pub eventinj: u64,
    pub n_cr3: u64,
    pub lbr_virtualization_enable: u64,
    pub vmcb_clean: u32,
    pub nrip: u64,
    pub guest_inst_bytes: [u8; 15],
    pub avic_apic_backing_page_ptr: u64,
    pub avic_logical_table_ptr: u64,
    pub avic_physical_table_ptr: u64,
    
    // Save State Area (0x400-0x5FF)
    pub guest_es: SegmentRegister,
    pub guest_cs: SegmentRegister,
    pub guest_ss: SegmentRegister,
    pub guest_ds: SegmentRegister,
    pub guest_fs: SegmentRegister,
    pub guest_gs: SegmentRegister,
    pub guest_gdtr: DescriptorRegister,
    pub guest_ldtr: SegmentRegister,
    pub guest_idtr: DescriptorRegister,
    pub guest_tr: SegmentRegister,
    
    pub guest_cpl: u8,
    pub guest_efer: u64,
    pub guest_cr4: u64,
    pub guest_cr3: u64,
    pub guest_cr0: u64,
    pub guest_dr7: u64,
    pub guest_dr6: u64,
    pub guest_rflags: u64,
    pub guest_rip: u64,
    pub guest_rsp: u64,
    pub guest_rax: u64,
    pub guest_star: u64,
    pub guest_lstar: u64,
    pub guest_cstar: u64,
    pub guest_sfmask: u64,
    pub guest_kernel_gs_base: u64,
    pub guest_sysenter_cs: u64,
    pub guest_sysenter_esp: u64,
    pub guest_sysenter_eip: u64,
    pub guest_cr2: u64,
    pub guest_pat: u64,
    pub guest_dbgctl: u64,
    pub guest_br_from: u64,
    pub guest_br_to: u64,
    pub guest_last_excp_from: u64,
    pub guest_last_excp_to: u64,
    
    // Padding to 4KB
    _reserved: [u8; 1024],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SegmentRegister {
    pub selector: u16,
    pub attrib: u16,
    pub limit: u32,
    pub base: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DescriptorRegister {
    pub limit: u16,
    pub base: u64,
}

impl Default for VmcbState {
    fn default() -> Self {
        Self {
            // Control Area - Conservative defaults
            intercept_cr_read: 0,
            intercept_cr_write: 0,
            intercept_dr_read: 0,
            intercept_dr_write: 0,
            intercept_exception: 0,
            intercept_instr1: 0,
            intercept_instr2: 0,
            intercept_instr3: 0,
            iopm_base_pa: 0,
            msrpm_base_pa: 0,
            tsc_offset: 0,
            guest_asid: 1,
            tlb_control: 0,
            np_enable: 1, // Enable nested paging
            n_cr3: 0,
            
            // Guest State - Safe defaults for 64-bit mode
            guest_es_selector: 0x10,
            guest_es_attrib: 0x93, // Present, DPL=0, Data, R/W
            guest_es_limit: 0xFFFFFFFF,
            guest_es_base: 0,
            
            guest_cs_selector: 0x08,
            guest_cs_attrib: 0x29B, // Present, DPL=0, Code, R/X, L=1
            guest_cs_limit: 0xFFFFFFFF,
            guest_cs_base: 0,
            
            guest_ss_selector: 0x10,
            guest_ss_attrib: 0x93, // Present, DPL=0, Data, R/W
            guest_ss_limit: 0xFFFFFFFF,
            guest_ss_base: 0,
            
            guest_ds_selector: 0x10,
            guest_ds_attrib: 0x93, // Present, DPL=0, Data, R/W
            guest_ds_limit: 0xFFFFFFFF,
            guest_ds_base: 0,
            
            guest_fs_selector: 0x10,
            guest_fs_attrib: 0x93, // Present, DPL=0, Data, R/W
            guest_fs_limit: 0xFFFFFFFF,
            guest_fs_base: 0,
            
            guest_gs_selector: 0x10,
            guest_gs_attrib: 0x93, // Present, DPL=0, Data, R/W
            guest_gs_limit: 0xFFFFFFFF,
            guest_gs_base: 0,
            
            guest_gdtr_selector: 0,
            guest_gdtr_attrib: 0,
            guest_gdtr_limit: 0x27,
            guest_gdtr_base: 0,
            
            guest_ldtr_selector: 0,
            guest_ldtr_attrib: 0x82, // Present, LDT
            guest_ldtr_limit: 0,
            guest_ldtr_base: 0,
            
            guest_idtr_selector: 0,
            guest_idtr_attrib: 0,
            guest_idtr_limit: 0xFFF,
            guest_idtr_base: 0,
            
            guest_tr_selector: 0x18,
            guest_tr_attrib: 0x8B, // Present, DPL=0, TSS
            guest_tr_limit: 0x67,
            guest_tr_base: 0,
            
            // Control Registers - Safe defaults
            guest_cr0: 0x80000031, // PE, PG, ET, NE
            guest_cr2: 0,
            guest_cr3: 0,
            guest_cr4: 0x2000, // VMXE (for compatibility)
            guest_dr6: 0xFFFF0FF0,
            guest_dr7: 0x400,
            guest_rflags: 0x2, // Reserved bit
            guest_rip: 0x1000,
            guest_rsp: 0x8000,
            guest_rax: 0,
        }
    }
}

impl<'a> ActiveVmcb<'a> {
    /// Load complete VMCB state from the structure
    pub fn load_state(&mut self, state: &VmcbState) {
        // Control Area
        self.write_u16(vmcb_offsets::INTERCEPT_CR_READ, state.intercept_cr_read);
        self.write_u16(vmcb_offsets::INTERCEPT_CR_WRITE, state.intercept_cr_write);
        self.write_u16(vmcb_offsets::INTERCEPT_DR_READ, state.intercept_dr_read);
        self.write_u16(vmcb_offsets::INTERCEPT_DR_WRITE, state.intercept_dr_write);
        self.write_u32(vmcb_offsets::INTERCEPT_EXCEPTION, state.intercept_exception);
        self.write_u32(vmcb_offsets::INTERCEPT_INSTR1, state.intercept_instr1);
        self.write_u32(vmcb_offsets::INTERCEPT_INSTR2, state.intercept_instr2);
        self.write_u32(vmcb_offsets::INTERCEPT_INSTR3, state.intercept_instr3);
        self.write_u64(vmcb_offsets::IOPM_BASE_PA, state.iopm_base_pa);
        self.write_u64(vmcb_offsets::MSRPM_BASE_PA, state.msrpm_base_pa);
        self.write_u64(vmcb_offsets::TSC_OFFSET, state.tsc_offset);
        self.write_u32(vmcb_offsets::GUEST_ASID, state.guest_asid);
        self.write_u32(vmcb_offsets::TLB_CONTROL, state.tlb_control);
        self.write_u64(vmcb_offsets::NP_ENABLE, state.np_enable);
        self.write_u64(vmcb_offsets::N_CR3, state.n_cr3);
        
        // Guest Segment Registers
        self.write_u16(vmcb_offsets::GUEST_ES_SELECTOR, state.guest_es_selector);
        self.write_u16(vmcb_offsets::GUEST_ES_ATTRIB, state.guest_es_attrib);
        self.write_u32(vmcb_offsets::GUEST_ES_LIMIT, state.guest_es_limit);
        self.write_u64(vmcb_offsets::GUEST_ES_BASE, state.guest_es_base);
        
        self.write_u16(vmcb_offsets::GUEST_CS_SELECTOR, state.guest_cs_selector);
        self.write_u16(vmcb_offsets::GUEST_CS_ATTRIB, state.guest_cs_attrib);
        self.write_u32(vmcb_offsets::GUEST_CS_LIMIT, state.guest_cs_limit);
        self.write_u64(vmcb_offsets::GUEST_CS_BASE, state.guest_cs_base);
        
        self.write_u16(vmcb_offsets::GUEST_SS_SELECTOR, state.guest_ss_selector);
        self.write_u16(vmcb_offsets::GUEST_SS_ATTRIB, state.guest_ss_attrib);
        self.write_u32(vmcb_offsets::GUEST_SS_LIMIT, state.guest_ss_limit);
        self.write_u64(vmcb_offsets::GUEST_SS_BASE, state.guest_ss_base);
        
        self.write_u16(vmcb_offsets::GUEST_DS_SELECTOR, state.guest_ds_selector);
        self.write_u16(vmcb_offsets::GUEST_DS_ATTRIB, state.guest_ds_attrib);
        self.write_u32(vmcb_offsets::GUEST_DS_LIMIT, state.guest_ds_limit);
        self.write_u64(vmcb_offsets::GUEST_DS_BASE, state.guest_ds_base);
        
        self.write_u16(vmcb_offsets::GUEST_FS_SELECTOR, state.guest_fs_selector);
        self.write_u16(vmcb_offsets::GUEST_FS_ATTRIB, state.guest_fs_attrib);
        self.write_u32(vmcb_offsets::GUEST_FS_LIMIT, state.guest_fs_limit);
        self.write_u64(vmcb_offsets::GUEST_FS_BASE, state.guest_fs_base);
        
        self.write_u16(vmcb_offsets::GUEST_GS_SELECTOR, state.guest_gs_selector);
        self.write_u16(vmcb_offsets::GUEST_GS_ATTRIB, state.guest_gs_attrib);
        self.write_u32(vmcb_offsets::GUEST_GS_LIMIT, state.guest_gs_limit);
        self.write_u64(vmcb_offsets::GUEST_GS_BASE, state.guest_gs_base);
        
        self.write_u16(vmcb_offsets::GUEST_GDTR_SELECTOR, state.guest_gdtr_selector);
        self.write_u16(vmcb_offsets::GUEST_GDTR_ATTRIB, state.guest_gdtr_attrib);
        self.write_u32(vmcb_offsets::GUEST_GDTR_LIMIT, state.guest_gdtr_limit);
        self.write_u64(vmcb_offsets::GUEST_GDTR_BASE, state.guest_gdtr_base);
        
        self.write_u16(vmcb_offsets::GUEST_LDTR_SELECTOR, state.guest_ldtr_selector);
        self.write_u16(vmcb_offsets::GUEST_LDTR_ATTRIB, state.guest_ldtr_attrib);
        self.write_u32(vmcb_offsets::GUEST_LDTR_LIMIT, state.guest_ldtr_limit);
        self.write_u64(vmcb_offsets::GUEST_LDTR_BASE, state.guest_ldtr_base);
        
        self.write_u16(vmcb_offsets::GUEST_IDTR_SELECTOR, state.guest_idtr_selector);
        self.write_u16(vmcb_offsets::GUEST_IDTR_ATTRIB, state.guest_idtr_attrib);
        self.write_u32(vmcb_offsets::GUEST_IDTR_LIMIT, state.guest_idtr_limit);
        self.write_u64(vmcb_offsets::GUEST_IDTR_BASE, state.guest_idtr_base);
        
        self.write_u16(vmcb_offsets::GUEST_TR_SELECTOR, state.guest_tr_selector);
        self.write_u16(vmcb_offsets::GUEST_TR_ATTRIB, state.guest_tr_attrib);
        self.write_u32(vmcb_offsets::GUEST_TR_LIMIT, state.guest_tr_limit);
        self.write_u64(vmcb_offsets::GUEST_TR_BASE, state.guest_tr_base);
        
        // Control Registers and General State
        self.write_u64(vmcb_offsets::GUEST_CR0, state.guest_cr0);
        self.write_u64(vmcb_offsets::GUEST_CR2, state.guest_cr2);
        self.write_u64(vmcb_offsets::GUEST_CR3, state.guest_cr3);
        self.write_u64(vmcb_offsets::GUEST_CR4, state.guest_cr4);
        self.write_u64(vmcb_offsets::GUEST_DR6, state.guest_dr6);
        self.write_u64(vmcb_offsets::GUEST_DR7, state.guest_dr7);
        self.write_u64(vmcb_offsets::GUEST_RFLAGS, state.guest_rflags);
        self.write_u64(vmcb_offsets::GUEST_RIP, state.guest_rip);
        self.write_u64(vmcb_offsets::GUEST_RSP, state.guest_rsp);
        self.write_u64(vmcb_offsets::GUEST_RAX, state.guest_rax);
    }
    
    /// Save complete VMCB state to the structure
    pub fn save_state(&self, state: &mut VmcbState) {
        // Control Area
        state.intercept_cr_read = self.read_u16(vmcb_offsets::INTERCEPT_CR_READ);
        state.intercept_cr_write = self.read_u16(vmcb_offsets::INTERCEPT_CR_WRITE);
        state.intercept_dr_read = self.read_u16(vmcb_offsets::INTERCEPT_DR_READ);
        state.intercept_dr_write = self.read_u16(vmcb_offsets::INTERCEPT_DR_WRITE);
        state.intercept_exception = self.read_u32(vmcb_offsets::INTERCEPT_EXCEPTION);
        state.intercept_instr1 = self.read_u32(vmcb_offsets::INTERCEPT_INSTR1);
        state.intercept_instr2 = self.read_u32(vmcb_offsets::INTERCEPT_INSTR2);
        state.intercept_instr3 = self.read_u32(vmcb_offsets::INTERCEPT_INSTR3);
        state.iopm_base_pa = self.read_u64(vmcb_offsets::IOPM_BASE_PA);
        state.msrpm_base_pa = self.read_u64(vmcb_offsets::MSRPM_BASE_PA);
        state.tsc_offset = self.read_u64(vmcb_offsets::TSC_OFFSET);
        state.guest_asid = self.read_u32(vmcb_offsets::GUEST_ASID);
        state.tlb_control = self.read_u32(vmcb_offsets::TLB_CONTROL);
        state.np_enable = self.read_u64(vmcb_offsets::NP_ENABLE);
        state.n_cr3 = self.read_u64(vmcb_offsets::N_CR3);
        
        // Guest State - Control Registers
        state.guest_cr0 = self.read_u64(vmcb_offsets::GUEST_CR0);
        state.guest_cr2 = self.read_u64(vmcb_offsets::GUEST_CR2);
        state.guest_cr3 = self.read_u64(vmcb_offsets::GUEST_CR3);
        state.guest_cr4 = self.read_u64(vmcb_offsets::GUEST_CR4);
        state.guest_dr6 = self.read_u64(vmcb_offsets::GUEST_DR6);
        state.guest_dr7 = self.read_u64(vmcb_offsets::GUEST_DR7);
        state.guest_rflags = self.read_u64(vmcb_offsets::GUEST_RFLAGS);
        state.guest_rip = self.read_u64(vmcb_offsets::GUEST_RIP);
        state.guest_rsp = self.read_u64(vmcb_offsets::GUEST_RSP);
        state.guest_rax = self.read_u64(vmcb_offsets::GUEST_RAX);
    }
}

/// Execute VMRUN instruction
#[inline]
pub unsafe fn vmrun(vmcb_pa: PhysicalAddress) -> Result<(), VmcbError> {
    let result: u64;
    unsafe {
        asm!(
            "vmrun",
            in("rax") vmcb_pa,
            lateout("rax") result,
            options(nostack, preserves_flags),
        );
    }
    
    if result == 0 {
        Ok(())
    } else {
        Err(VmcbError::VmrunFailed)
    }
}

/// Execute VMLOAD instruction
#[inline]
pub unsafe fn vmload(vmcb_pa: PhysicalAddress) -> Result<(), VmcbError> {
    unsafe {
        asm!(
            "vmload",
            in("rax") vmcb_pa,
            options(nostack, preserves_flags),
        );
    }
    Ok(())
}

/// Execute VMSAVE instruction
#[inline]
pub unsafe fn vmsave(vmcb_pa: PhysicalAddress) -> Result<(), VmcbError> {
    unsafe {
        asm!(
            "vmsave",
            in("rax") vmcb_pa,
            options(nostack, preserves_flags),
        );
    }
    Ok(())
}

/// Complete SVM virtualization engine implementation
pub struct SvmEngine {
    vmcb_pool: Vec<PhysicalAddress>,
    nested_paging_enabled: bool,
    host_save_area: PhysicalAddress,
}


impl SvmEngine {
    /// Create new SVM engine with complete initialization
    pub fn new() -> Result<Self, SvmError> {
        // Check SVM support
        if !Self::is_svm_supported() {
            return Err(SvmError::NotSupported);
        }
        
        // Enable SVM in EFER
        unsafe {
            let mut efer: u64;
            asm!("rdmsr", in("ecx") 0xC0000080u32, out("eax") efer, out("edx") _);
            efer |= 1 << 12; // SVME bit
            asm!("wrmsr", in("ecx") 0xC0000080u32, in("eax") efer, in("edx") 0u32);
        }
        
        // Allocate host save area
        let host_save_area = PhysicalAddress::new(
            crate::memory::allocate_aligned(4096, 4096)? as u64
        );
        
        // Set VM_HSAVE_PA MSR
        unsafe {
            asm!("wrmsr", 
                in("ecx") 0xC0010117u32, 
                in("eax") host_save_area.as_u64() as u32,
                in("edx") (host_save_area.as_u64() >> 32) as u32
            );
        }
        
        Ok(SvmEngine {
            vmcb_pool: Vec::new(),
            host_save_area,
            nested_paging_enabled: Self::is_nested_paging_supported(),
        })
    }
    
    /// Check if SVM is supported
    fn is_svm_supported() -> bool {
        let (_, _, ecx, _) = unsafe { core::arch::x86_64::__cpuid(0x80000001) };
        (ecx & (1 << 2)) != 0 // SVM bit
    }
    
    /// Check if nested paging is supported
    fn is_nested_paging_supported() -> bool {
        let (_, _, _, edx) = unsafe { core::arch::x86_64::__cpuid(0x8000000A) };
        (edx & (1 << 0)) != 0 // NP bit
    }
    
    /// Create and initialize a new VMCB
    pub fn create_vmcb(&mut self) -> Result<usize, SvmError> {
        let mut vmcb = VmcbState::default();
        
        // Initialize control area
        vmcb.intercept_cr_read = 0xFFFF; // Intercept all CR reads
        vmcb.intercept_cr_write = 0xFFFF; // Intercept all CR writes
        vmcb.intercept_dr_read = 0xFFFF; // Intercept all DR reads
        vmcb.intercept_dr_write = 0xFFFF; // Intercept all DR writes
        vmcb.intercept_exception = 0xFFFFFFFF; // Intercept all exceptions
        
        // Intercept instructions
        vmcb.intercept_instr1 = 
            (1 << 0) |  // INTR
            (1 << 1) |  // NMI
            (1 << 2) |  // SMI
            (1 << 3) |  // INIT
            (1 << 4) |  // VINTR
            (1 << 5) |  // CR0_SEL_WRITE
            (1 << 6) |  // IDTR_READ
            (1 << 7) |  // GDTR_READ
            (1 << 8) |  // LDTR_READ
            (1 << 9) |  // TR_READ
            (1 << 10) | // IDTR_WRITE
            (1 << 11) | // GDTR_WRITE
            (1 << 12) | // LDTR_WRITE
            (1 << 13) | // TR_WRITE
            (1 << 14) | // RDTSC
            (1 << 15) | // RDPMC
            (1 << 16) | // PUSHF
            (1 << 17) | // POPF
            (1 << 18) | // CPUID
            (1 << 19) | // RSM
            (1 << 20) | // IRET
            (1 << 21) | // INTn
            (1 << 22) | // INVD
            (1 << 23) | // PAUSE
            (1 << 24) | // HLT
            (1 << 25) | // INVLPG
            (1 << 26) | // INVLPGA
            (1 << 27) | // IOIO_PROT
            (1 << 28) | // MSR_PROT
            (1 << 29) | // TASK_SWITCH
            (1 << 30) | // FERR_FREEZE
            (1 << 31);  // SHUTDOWN
            
        vmcb.intercept_instr2 = 
            (1 << 0) |  // VMRUN
            (1 << 1) |  // VMMCALL
            (1 << 2) |  // VMLOAD
            (1 << 3) |  // VMSAVE
            (1 << 4) |  // STGI
            (1 << 5) |  // CLGI
            (1 << 6) |  // SKINIT
            (1 << 7) |  // RDTSCP
            (1 << 8) |  // ICEBP
            (1 << 9) |  // WBINVD
            (1 << 10) | // MONITOR
            (1 << 11) | // MWAIT
            (1 << 12) | // MWAIT_CONDITIONAL
            (1 << 13) | // XSETBV
            (1 << 14) | // RDPRU
            (1 << 15);  // EFER_WRITE_TRAP
        
        // Enable nested paging if supported
        if self.nested_paging_enabled {
            vmcb.np_enable = 1;
            // Set up nested page table (identity mapping for now)
            vmcb.n_cr3 = self.setup_nested_page_table()?;
        }
        
        // Initialize guest state to real mode
        vmcb.guest_cs.selector = 0xF000;
        vmcb.guest_cs.base = 0xFFFF0000;
        vmcb.guest_cs.limit = 0xFFFF;
        vmcb.guest_cs.attrib = 0x009B; // Code segment, present, readable
        
        vmcb.guest_rip = 0xFFF0; // Reset vector
        vmcb.guest_rflags = 0x2; // Reserved bit must be 1
        
        // Set guest ASID
        vmcb.guest_asid = (self.vmcb_pool.len() + 1) as u32;
        
        self.vmcb_pool.push(vmcb);
        Ok(self.vmcb_pool.len() - 1)
    }
    
    /// Set up nested page table for guest physical memory
    fn setup_nested_page_table(&self) -> Result<u64, SvmError> {
        // Allocate PML4 table
        let pml4_addr = crate::memory::allocate_aligned(4096, 4096)
            .map_err(|_| SvmError::NestedPagingSetupFailed)?;
        
        // Identity map first 4GB for simplicity
        unsafe {
            let pml4 = pml4_addr as *mut u64;
            
            // Allocate PDPT
            let pdpt_addr = crate::memory::allocate_aligned(4096, 4096)
                .map_err(|_| SvmError::NestedPagingSetupFailed)?;
            *pml4 = pdpt_addr as u64 | 0x7; // Present, writable, user
            
            let pdpt = pdpt_addr as *mut u64;
            
            // Map 4 x 1GB pages
            for i in 0..4 {
                *pdpt.add(i) = (i as u64 * 0x40000000) | 0x87; // 1GB page, present, writable, user
            }
        }
        
        Ok(pml4_addr as u64)
    }
    
    /// Execute VMRUN with comprehensive exit handling
    pub fn vmrun(&mut self, vmcb_index: usize) -> Result<SvmExitReason, SvmError> {
        if vmcb_index >= self.vmcb_pool.len() {
            return Err(SvmError::InvalidVmcb);
        }
        
        let vmcb_pa = &self.vmcb_pool[vmcb_index] as *const _ as u64;
        
        // Execute VMRUN
        let exit_code = unsafe {
            vmrun_asm(vmcb_pa)
        };
        
        // Handle exit
        self.handle_vmexit(vmcb_index, exit_code)
    }
    
    /// Comprehensive VMEXIT handling
    fn handle_vmexit(&mut self, vmcb_index: usize, exit_code: u64) -> Result<SvmExitReason, SvmError> {
        let vmcb = &mut self.vmcb_pool[vmcb_index];
        
        match exit_code {
            0x40 => Ok(SvmExitReason::CpuId),
            0x41 => Ok(SvmExitReason::Hlt),
            0x7B => Ok(SvmExitReason::IoInstruction),
            0x400 => Ok(SvmExitReason::NestedPageFault),
            0x81 => Ok(SvmExitReason::Vmmcall),
            _ => Ok(SvmExitReason::Unknown(exit_code)),
        }
    }
    /// Initialize SVM engine and enable SVM mode
    pub fn new() -> Result<Self, VmcbError> {
        // Check SVM support via CPUID
        let cpuid = unsafe { core::arch::x86_64::__cpuid(0x80000001) };
        if (cpuid.ecx & (1 << 2)) == 0 {
            return Err(VmcbError::NotSupported);
        }
        
        // Allocate host save area (4KB aligned)
        let host_save_area = Self::allocate_host_save_area()?;
        
        // Enable SVM in EFER
        unsafe {
            let mut efer = Self::read_msr(0xC0000080); // IA32_EFER
            efer |= 1 << 12; // SVME bit
            Self::write_msr(0xC0000080, efer);
            
            // Set host save area in VM_HSAVE_PA MSR
            Self::write_msr(0xC0010117, host_save_area);
        }
        
        // Check nested paging support
        let cpuid_np = unsafe { core::arch::x86_64::__cpuid(0x8000000A) };
        let nested_paging_enabled = (cpuid_np.edx & (1 << 0)) != 0;
        
        Ok(SvmEngine {
            vmcb_pool: Vec::new(),
            nested_paging_enabled,
            host_save_area,
        })
    }
    
    /// Allocate host save area
    fn allocate_host_save_area() -> Result<PhysicalAddress, VmcbError> {
        static mut HOST_SAVE_AREA: [u8; 4096] = [0; 4096];
        unsafe {
            core::ptr::write_bytes(HOST_SAVE_AREA.as_mut_ptr(), 0, 4096);
            Ok(HOST_SAVE_AREA.as_ptr() as PhysicalAddress)
        }
    }
    
    /// Allocate and initialize a VMCB
    pub fn allocate_vmcb(&mut self) -> Result<PhysicalAddress, VmcbError> {
        // Allocate 4KB aligned VMCB
        static mut VMCB_STORAGE: [u8; 4096 * 256] = [0; 4096 * 256];
        static mut NEXT_OFFSET: usize = 0;
        
        unsafe {
            if NEXT_OFFSET + 4096 > VMCB_STORAGE.len() {
                return Err(VmcbError::AllocationFailed);
            }
            let ptr = &VMCB_STORAGE[NEXT_OFFSET] as *const u8 as usize;
            NEXT_OFFSET += 4096;
            
            // Zero the VMCB
            core::ptr::write_bytes(ptr as *mut u8, 0, 4096);
            
            let vmcb_pa = ptr as PhysicalAddress;
            self.vmcb_pool.push(vmcb_pa);
            Ok(vmcb_pa)
        }
    }
    
    /// Setup VMCB control area with comprehensive intercepts
    pub fn setup_vmcb_controls(&self, vmcb_pa: PhysicalAddress) -> Result<(), VmcbError> {
        unsafe {
            let vmcb = vmcb_pa as *mut u8;
            
            // Set up intercepts for comprehensive virtualization
            // CR read/write intercepts
            core::ptr::write_volatile(
                vmcb.add(vmcb_offsets::INTERCEPT_CR_READ) as *mut u16,
                0xFFFF // Intercept all CR reads
            );
            core::ptr::write_volatile(
                vmcb.add(vmcb_offsets::INTERCEPT_CR_WRITE) as *mut u16,
                0xFFFF // Intercept all CR writes
            );
            
            // Exception intercepts
            core::ptr::write_volatile(
                vmcb.add(vmcb_offsets::INTERCEPT_EXCEPTION) as *mut u32,
                0xFFFFFFFF // Intercept all exceptions initially
            );
            
            // Instruction intercepts 1 - comprehensive set
            let instr1_intercepts = 0x00000000 |
                (1 << 14) | // RDTSC
                (1 << 15) | // RDPMC
                (1 << 18) | // CPUID
                (1 << 24) | // HLT
                (1 << 25) | // INVLPG
                (1 << 27) | // IOIO
                (1 << 28) | // MSR
                (1 << 29); // TASK_SWITCH
            core::ptr::write_volatile(
                vmcb.add(vmcb_offsets::INTERCEPT_INSTR1) as *mut u32,
                instr1_intercepts
            );
            
            // Instruction intercepts 2
            let instr2_intercepts = 0x00000000 |
                (1 << 0) |  // VMRUN
                (1 << 1) |  // VMMCALL
                (1 << 7);   // RDTSCP
            core::ptr::write_volatile(
                vmcb.add(vmcb_offsets::INTERCEPT_INSTR2) as *mut u32,
                instr2_intercepts
            );
            
            // Enable nested paging if supported
            if self.nested_paging_enabled {
                core::ptr::write_volatile(
                    vmcb.add(vmcb_offsets::NP_ENABLE) as *mut u64,
                    1u64
                );
                
                // Set up nested page table pointer (NCR3)
                let ncr3 = Self::build_nested_page_tables()?;
                core::ptr::write_volatile(
                    vmcb.add(vmcb_offsets::N_CR3) as *mut u64,
                    ncr3
                );
            }
            
            // Set guest ASID
            core::ptr::write_volatile(
                vmcb.add(vmcb_offsets::GUEST_ASID) as *mut u32,
                1u32
            );
        }
        
        Ok(())
    }
    
    /// Build nested page tables for SVM
    fn build_nested_page_tables() -> Result<PhysicalAddress, VmcbError> {
        // Create identity mapping for first 4GB using 2MB pages
        static mut NPT_PML4: [u64; 512] = [0; 512];
        static mut NPT_PDPT: [u64; 512] = [0; 512];
        static mut NPT_PD: [u64; 512] = [0; 512];
        
        unsafe {
            // Set up PD with 2MB pages
            for i in 0..512 {
                let phys = (i as u64) << 21; // 2MB pages
                NPT_PD[i] = phys | 0x87; // Present, Writable, User, Large page
            }
            
            // Point PDPT[0] to PD
            NPT_PDPT[0] = (&NPT_PD as *const _ as u64) | 0x07; // Present, Writable, User
            
            // Point PML4[0] to PDPT
            NPT_PML4[0] = (&NPT_PDPT as *const _ as u64) | 0x07; // Present, Writable, User
            
            Ok(&NPT_PML4 as *const _ as PhysicalAddress)
        }
    }
    
    /// Read MSR
    unsafe fn read_msr(msr: u32) -> u64 {
        let (high, low): (u32, u32);
        unsafe {
            core::arch::asm!("rdmsr", in("ecx") msr, out("eax") low, out("edx") high);
        }
        ((high as u64) << 32) | (low as u64)
    }
    
    /// Write MSR
    unsafe fn write_msr(msr: u32, value: u64) {
        let low = value as u32;
        let high = (value >> 32) as u32;
        unsafe {
            core::arch::asm!("wrmsr", in("ecx") msr, in("eax") low, in("edx") high);
        }
    }
}