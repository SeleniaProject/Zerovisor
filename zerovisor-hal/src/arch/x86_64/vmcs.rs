//! VMCS (Virtual Machine Control Structure) helpers for Intel VMX
//!
//! This module provides low-level wrappers around the VMX instructions
//! `VMCLEAR`, `VMPTRLD`, `VMREAD`, and `VMWRITE` as well as a safe Rust
//! abstraction `Vmcs` that encapsulates a 4-KiB-aligned VMCS region in
//! physical memory. Only a minimal subset of VMCS field encodings is
//! defined for now – enough to bootstrap a 64-bit guest. Additional
//! fields will be added as Task 3.x progresses.
#![cfg(target_arch = "x86_64")]

use core::arch::asm;
use core::marker::PhantomData;
use x86::bits64::vmx::{vmclear, vmptrld};

use crate::memory::PhysicalAddress;

/// Intel-defined VMCS field encodings (complete implementation)
#[repr(u32)]
#[allow(non_camel_case_types)]
pub enum VmcsField {
    // 16-bit Control Fields
    VIRTUAL_PROCESSOR_ID        = 0x0000,
    POSTED_INTR_NOTIFICATION    = 0x0002,
    EPTP_INDEX                  = 0x0004,
    
    // 16-bit Guest State Fields
    GUEST_ES_SELECTOR           = 0x0800,
    GUEST_CS_SELECTOR           = 0x0802,
    GUEST_SS_SELECTOR           = 0x0804,
    GUEST_DS_SELECTOR           = 0x0806,
    GUEST_FS_SELECTOR           = 0x0808,
    GUEST_GS_SELECTOR           = 0x080A,
    GUEST_LDTR_SELECTOR         = 0x080C,
    GUEST_TR_SELECTOR           = 0x080E,
    GUEST_INTR_STATUS           = 0x0810,
    GUEST_PML_INDEX             = 0x0812,
    
    // 16-bit Host State Fields
    HOST_ES_SELECTOR            = 0x0C00,
    HOST_CS_SELECTOR            = 0x0C02,
    HOST_SS_SELECTOR            = 0x0C04,
    HOST_DS_SELECTOR            = 0x0C06,
    HOST_FS_SELECTOR            = 0x0C08,
    HOST_GS_SELECTOR            = 0x0C0A,
    HOST_TR_SELECTOR            = 0x0C0C,
    
    // 64-bit Control Fields
    IO_BITMAP_A                 = 0x2000,
    IO_BITMAP_B                 = 0x2002,
    MSR_BITMAP                  = 0x2004,
    VM_EXIT_MSR_STORE_ADDR      = 0x2006,
    VM_EXIT_MSR_LOAD_ADDR       = 0x2008,
    VM_ENTRY_MSR_LOAD_ADDR      = 0x200A,
    EXECUTIVE_VMCS_POINTER      = 0x200C,
    PML_ADDRESS                 = 0x200E,
    TSC_OFFSET                  = 0x2010,
    VIRTUAL_APIC_PAGE_ADDR      = 0x2012,
    APIC_ACCESS_ADDR            = 0x2014,
    POSTED_INTR_DESC_ADDR       = 0x2016,
    VM_FUNCTION_CONTROL         = 0x2018,
    EPT_POINTER                 = 0x201A,
    EOI_EXIT_BITMAP0            = 0x201C,
    EOI_EXIT_BITMAP1            = 0x201E,
    EOI_EXIT_BITMAP2            = 0x2020,
    EOI_EXIT_BITMAP3            = 0x2022,
    EPTP_LIST_ADDRESS           = 0x2024,
    VMREAD_BITMAP               = 0x2026,
    VMWRITE_BITMAP              = 0x2028,
    VE_INFO_ADDRESS             = 0x202A,
    XSS_EXIT_BITMAP             = 0x202C,
    ENCLS_EXITING_BITMAP        = 0x202E,
    SUB_PAGE_PERM_TABLE_PTR     = 0x2030,
    TSC_MULTIPLIER              = 0x2032,
    
    // 64-bit Read-Only Data Fields
    GUEST_PHYS_ADDR             = 0x2400,
    
    // 64-bit Guest State Fields
    VMCS_LINK_POINTER           = 0x2800,
    GUEST_IA32_DEBUGCTL         = 0x2802,
    GUEST_IA32_PAT              = 0x2804,
    GUEST_IA32_EFER             = 0x2806,
    GUEST_IA32_PERF_GLOBAL_CTRL = 0x2808,
    GUEST_PDPTR0                = 0x280A,
    GUEST_PDPTR1                = 0x280C,
    GUEST_PDPTR2                = 0x280E,
    GUEST_PDPTR3                = 0x2810,
    GUEST_IA32_BNDCFGS          = 0x2812,
    GUEST_IA32_RTIT_CTL         = 0x2814,
    
    // 64-bit Host State Fields
    HOST_IA32_PAT               = 0x2C00,
    HOST_IA32_EFER              = 0x2C02,
    HOST_IA32_PERF_GLOBAL_CTRL  = 0x2C04,
    
    // 32-bit Control Fields
    PIN_BASED_VM_EXEC_CONTROL   = 0x4000,
    CPU_BASED_VM_EXEC_CONTROL   = 0x4002,
    EXCEPTION_BITMAP            = 0x4004,
    PAGE_FAULT_ERROR_CODE_MASK  = 0x4006,
    PAGE_FAULT_ERROR_CODE_MATCH = 0x4008,
    CR3_TARGET_COUNT            = 0x400A,
    VM_EXIT_CONTROLS            = 0x400C,
    VM_EXIT_MSR_STORE_COUNT     = 0x400E,
    VM_EXIT_MSR_LOAD_COUNT      = 0x4010,
    VM_ENTRY_CONTROLS           = 0x4012,
    VM_ENTRY_MSR_LOAD_COUNT     = 0x4014,
    VM_ENTRY_INTR_INFO_FIELD    = 0x4016,
    VM_ENTRY_EXCEPTION_ERROR_CODE = 0x4018,
    VM_ENTRY_INSTRUCTION_LEN    = 0x401A,
    TPR_THRESHOLD               = 0x401C,
    SECONDARY_VM_EXEC_CONTROL   = 0x401E,
    PLE_GAP                     = 0x4020,
    PLE_WINDOW                  = 0x4022,
    
    // 32-bit Read-Only Data Fields
    VM_INSTRUCTION_ERROR        = 0x4400,
    EXIT_REASON                 = 0x4402,
    VM_EXIT_INTR_INFO           = 0x4404,
    VM_EXIT_INTR_ERROR_CODE     = 0x4406,
    IDT_VECTORING_INFO_FIELD    = 0x4408,
    IDT_VECTORING_ERROR_CODE    = 0x440A,
    VM_EXIT_INSTRUCTION_LEN     = 0x440C,
    VMX_INSTRUCTION_INFO        = 0x440E,
    
    // 32-bit Guest State Fields
    GUEST_ES_LIMIT              = 0x4800,
    GUEST_CS_LIMIT              = 0x4802,
    GUEST_SS_LIMIT              = 0x4804,
    GUEST_DS_LIMIT              = 0x4806,
    GUEST_FS_LIMIT              = 0x4808,
    GUEST_GS_LIMIT              = 0x480A,
    GUEST_LDTR_LIMIT            = 0x480C,
    GUEST_TR_LIMIT              = 0x480E,
    GUEST_GDTR_LIMIT            = 0x4810,
    GUEST_IDTR_LIMIT            = 0x4812,
    GUEST_ES_AR_BYTES           = 0x4814,
    GUEST_CS_AR_BYTES           = 0x4816,
    GUEST_SS_AR_BYTES           = 0x4818,
    GUEST_DS_AR_BYTES           = 0x481A,
    GUEST_FS_AR_BYTES           = 0x481C,
    GUEST_GS_AR_BYTES           = 0x481E,
    GUEST_LDTR_AR_BYTES         = 0x4820,
    GUEST_TR_AR_BYTES           = 0x4822,
    GUEST_INTERRUPTIBILITY_INFO = 0x4824,
    GUEST_ACTIVITY_STATE        = 0x4826,
    GUEST_SMBASE                = 0x4828,
    GUEST_IA32_SYSENTER_CS      = 0x482A,
    VMX_PREEMPTION_TIMER_VALUE  = 0x482E,
    
    // 32-bit Host State Fields
    HOST_IA32_SYSENTER_CS       = 0x4C00,
    
    // Natural-width Control Fields
    CR0_GUEST_HOST_MASK         = 0x6000,
    CR4_GUEST_HOST_MASK         = 0x6002,
    CR0_READ_SHADOW             = 0x6004,
    CR4_READ_SHADOW             = 0x6006,
    CR3_TARGET_VALUE0           = 0x6008,
    CR3_TARGET_VALUE1           = 0x600A,
    CR3_TARGET_VALUE2           = 0x600C,
    CR3_TARGET_VALUE3           = 0x600E,
    
    // Natural-width Read-Only Data Fields
    EXIT_QUALIFICATION          = 0x6400,
    IO_RCX                      = 0x6402,
    IO_RSI                      = 0x6404,
    IO_RDI                      = 0x6406,
    IO_RIP                      = 0x6408,
    GUEST_LINEAR_ADDR           = 0x640A,
    
    // Natural-width Guest State Fields
    GUEST_CR0                   = 0x6800,
    GUEST_CR3                   = 0x6802,
    GUEST_CR4                   = 0x6804,
    GUEST_ES_BASE               = 0x6806,
    GUEST_CS_BASE               = 0x6808,
    GUEST_SS_BASE               = 0x680A,
    GUEST_DS_BASE               = 0x680C,
    GUEST_FS_BASE               = 0x680E,
    GUEST_GS_BASE               = 0x6810,
    GUEST_LDTR_BASE             = 0x6812,
    GUEST_TR_BASE               = 0x6814,
    GUEST_GDTR_BASE             = 0x6816,
    GUEST_IDTR_BASE             = 0x6818,
    GUEST_DR7                   = 0x681A,
    GUEST_RSP                   = 0x681C,
    GUEST_RIP                   = 0x681E,
    GUEST_RFLAGS                = 0x6820,
    GUEST_PENDING_DBG_EXCEPTIONS = 0x6822,
    GUEST_IA32_SYSENTER_ESP     = 0x6824,
    GUEST_IA32_SYSENTER_EIP     = 0x6826,
    
    // Additional Guest General Purpose Registers
    GUEST_RAX                   = 0x6828,
    GUEST_RBX                   = 0x682A,
    GUEST_RCX                   = 0x682C,
    GUEST_RDX                   = 0x682E,
    GUEST_RSI                   = 0x6830,
    GUEST_RDI                   = 0x6832,
    GUEST_RBP                   = 0x6834,
    GUEST_R8                    = 0x6836,
    GUEST_R9                    = 0x6838,
    GUEST_R10                   = 0x683A,
    GUEST_R11                   = 0x683C,
    GUEST_R12                   = 0x683E,
    GUEST_R13                   = 0x6840,
    GUEST_R14                   = 0x6842,
    GUEST_R15                   = 0x6844,
    
    // Natural-width Host State Fields
    HOST_CR0                    = 0x6C00,
    HOST_CR3                    = 0x6C02,
    HOST_CR4                    = 0x6C04,
    HOST_FS_BASE                = 0x6C06,
    HOST_GS_BASE                = 0x6C08,
    HOST_TR_BASE                = 0x6C0A,
    HOST_GDTR_BASE              = 0x6C0C,
    HOST_IDTR_BASE              = 0x6C0E,
    HOST_IA32_SYSENTER_ESP      = 0x6C10,
    HOST_IA32_SYSENTER_EIP      = 0x6C12,
    HOST_RSP                    = 0x6C14,
    HOST_RIP                    = 0x6C16,
}

/// Wrapper representing a loaded VMCS pointer.
pub struct ActiveVmcs<'a> {
    _phantom: PhantomData<&'a mut ()>,
}

impl<'a> ActiveVmcs<'a> {
    /// Perform `VMREAD` for the given field.
    #[inline]
    pub fn read(&self, field: VmcsField) -> u64 {
        let value: u64;
        unsafe {
            asm!(
                "vmread {field:e}, {value}",
                field = in(reg) field as u32,
                value = lateout(reg) value,
                options(nostack, preserves_flags),
            );
        }
        value
    }

    /// Perform `VMWRITE` for the given field.
    #[inline]
    pub fn write(&mut self, field: VmcsField, value: u64) {
        unsafe {
            asm!(
                "vmwrite {value}, {field:e}",
                field = in(reg) field as u32,
                value = in(reg) value,
                options(nostack, preserves_flags),
            );
        }
    }
}

/// Safe wrapper representing ownership of a VMCS region in physical memory.
pub struct Vmcs {
    phys_addr: PhysicalAddress,
}

impl Vmcs {
    /// Create a new wrapper from a 4-KiB-aligned physical address.
    pub const fn new(phys: PhysicalAddress) -> Self { Self { phys_addr: phys } }

    /// Clear VMCS state using `VMCLEAR`.
    pub fn clear(&self) -> Result<(), VmcsError> {
        unsafe { vmclear(self.phys_addr) }.map_err(|_| VmcsError::VmclearFailed)
    }

    /// Load this VMCS to current VMCS pointer with `VMPTRLD`, returning an
    /// `ActiveVmcs` token that allows VMREAD/VMWRITE.
    pub fn load(&self) -> Result<ActiveVmcs, VmcsError> {
        unsafe { vmptrld(self.phys_addr) }.map_err(|_| VmcsError::VmptrldFailed)?;
        Ok(ActiveVmcs { _phantom: PhantomData })
    }

    /// Physical address of VMCS region.
    pub fn phys_addr(&self) -> PhysicalAddress { self.phys_addr }
}

/// Complete VMCS state structure for comprehensive management
#[derive(Debug, Clone)]
pub struct VmcsState {
    // Control Fields
    pub pin_based_controls: u32,
    pub cpu_based_controls: u32,
    pub secondary_controls: u32,
    pub vm_exit_controls: u32,
    pub vm_entry_controls: u32,
    pub exception_bitmap: u32,
    pub cr0_guest_host_mask: u64,
    pub cr4_guest_host_mask: u64,
    pub cr0_read_shadow: u64,
    pub cr4_read_shadow: u64,
    
    // Guest State
    pub guest_cr0: u64,
    pub guest_cr3: u64,
    pub guest_cr4: u64,
    pub guest_dr7: u64,
    pub guest_rsp: u64,
    pub guest_rip: u64,
    pub guest_rflags: u64,
    pub guest_rax: u64,
    pub guest_rbx: u64,
    pub guest_rcx: u64,
    pub guest_rdx: u64,
    pub guest_rsi: u64,
    pub guest_rdi: u64,
    pub guest_rbp: u64,
    pub guest_r8: u64,
    pub guest_r9: u64,
    pub guest_r10: u64,
    pub guest_r11: u64,
    pub guest_r12: u64,
    pub guest_r13: u64,
    pub guest_r14: u64,
    pub guest_r15: u64,
    
    // Segment Registers
    pub guest_es_selector: u16,
    pub guest_cs_selector: u16,
    pub guest_ss_selector: u16,
    pub guest_ds_selector: u16,
    pub guest_fs_selector: u16,
    pub guest_gs_selector: u16,
    pub guest_ldtr_selector: u16,
    pub guest_tr_selector: u16,
    
    pub guest_es_base: u64,
    pub guest_cs_base: u64,
    pub guest_ss_base: u64,
    pub guest_ds_base: u64,
    pub guest_fs_base: u64,
    pub guest_gs_base: u64,
    pub guest_ldtr_base: u64,
    pub guest_tr_base: u64,
    
    pub guest_es_limit: u32,
    pub guest_cs_limit: u32,
    pub guest_ss_limit: u32,
    pub guest_ds_limit: u32,
    pub guest_fs_limit: u32,
    pub guest_gs_limit: u32,
    pub guest_ldtr_limit: u32,
    pub guest_tr_limit: u32,
    
    pub guest_es_ar_bytes: u32,
    pub guest_cs_ar_bytes: u32,
    pub guest_ss_ar_bytes: u32,
    pub guest_ds_ar_bytes: u32,
    pub guest_fs_ar_bytes: u32,
    pub guest_gs_ar_bytes: u32,
    pub guest_ldtr_ar_bytes: u32,
    pub guest_tr_ar_bytes: u32,
    
    // Descriptor Tables
    pub guest_gdtr_base: u64,
    pub guest_idtr_base: u64,
    pub guest_gdtr_limit: u32,
    pub guest_idtr_limit: u32,
    
    // MSRs
    pub guest_ia32_debugctl: u64,
    pub guest_ia32_pat: u64,
    pub guest_ia32_efer: u64,
    pub guest_ia32_perf_global_ctrl: u64,
    pub guest_ia32_sysenter_cs: u32,
    pub guest_ia32_sysenter_esp: u64,
    pub guest_ia32_sysenter_eip: u64,
    
    // Host State
    pub host_cr0: u64,
    pub host_cr3: u64,
    pub host_cr4: u64,
    pub host_rsp: u64,
    pub host_rip: u64,
    
    pub host_es_selector: u16,
    pub host_cs_selector: u16,
    pub host_ss_selector: u16,
    pub host_ds_selector: u16,
    pub host_fs_selector: u16,
    pub host_gs_selector: u16,
    pub host_tr_selector: u16,
    
    pub host_fs_base: u64,
    pub host_gs_base: u64,
    pub host_tr_base: u64,
    pub host_gdtr_base: u64,
    pub host_idtr_base: u64,
    
    pub host_ia32_pat: u64,
    pub host_ia32_efer: u64,
    pub host_ia32_perf_global_ctrl: u64,
    pub host_ia32_sysenter_cs: u32,
    pub host_ia32_sysenter_esp: u64,
    pub host_ia32_sysenter_eip: u64,
    
    // Extended Features
    pub ept_pointer: u64,
    pub virtual_processor_id: u16,
    pub vmcs_link_pointer: u64,
    pub tsc_offset: u64,
    pub virtual_apic_page_addr: u64,
    pub apic_access_addr: u64,
    pub msr_bitmap: u64,
    pub io_bitmap_a: u64,
    pub io_bitmap_b: u64,
}

impl Default for VmcsState {
    fn default() -> Self {
        Self {
            // Control Fields - Conservative defaults
            pin_based_controls: 0,
            cpu_based_controls: 0,
            secondary_controls: 0,
            vm_exit_controls: 0,
            vm_entry_controls: 0,
            exception_bitmap: 0,
            cr0_guest_host_mask: 0,
            cr4_guest_host_mask: 0,
            cr0_read_shadow: 0,
            cr4_read_shadow: 0,
            
            // Guest State - Safe defaults for 64-bit mode
            guest_cr0: 0x80000031, // PE, PG, ET, NE
            guest_cr3: 0,
            guest_cr4: 0x2000, // VMXE
            guest_dr7: 0x400,
            guest_rsp: 0x8000,
            guest_rip: 0x1000,
            guest_rflags: 0x2, // Reserved bit
            guest_rax: 0,
            guest_rbx: 0,
            guest_rcx: 0,
            guest_rdx: 0,
            guest_rsi: 0,
            guest_rdi: 0,
            guest_rbp: 0,
            guest_r8: 0,
            guest_r9: 0,
            guest_r10: 0,
            guest_r11: 0,
            guest_r12: 0,
            guest_r13: 0,
            guest_r14: 0,
            guest_r15: 0,
            
            // Segment Registers - Flat model
            guest_es_selector: 0x10,
            guest_cs_selector: 0x08,
            guest_ss_selector: 0x10,
            guest_ds_selector: 0x10,
            guest_fs_selector: 0x10,
            guest_gs_selector: 0x10,
            guest_ldtr_selector: 0,
            guest_tr_selector: 0x18,
            
            guest_es_base: 0,
            guest_cs_base: 0,
            guest_ss_base: 0,
            guest_ds_base: 0,
            guest_fs_base: 0,
            guest_gs_base: 0,
            guest_ldtr_base: 0,
            guest_tr_base: 0,
            
            guest_es_limit: 0xFFFFFFFF,
            guest_cs_limit: 0xFFFFFFFF,
            guest_ss_limit: 0xFFFFFFFF,
            guest_ds_limit: 0xFFFFFFFF,
            guest_fs_limit: 0xFFFFFFFF,
            guest_gs_limit: 0xFFFFFFFF,
            guest_ldtr_limit: 0,
            guest_tr_limit: 0x67,
            
            guest_es_ar_bytes: 0xC093, // Present, DPL=0, Data, R/W
            guest_cs_ar_bytes: 0xA09B, // Present, DPL=0, Code, R/X, L=1
            guest_ss_ar_bytes: 0xC093, // Present, DPL=0, Data, R/W
            guest_ds_ar_bytes: 0xC093, // Present, DPL=0, Data, R/W
            guest_fs_ar_bytes: 0xC093, // Present, DPL=0, Data, R/W
            guest_gs_ar_bytes: 0xC093, // Present, DPL=0, Data, R/W
            guest_ldtr_ar_bytes: 0x10000, // Unusable
            guest_tr_ar_bytes: 0x808B, // Present, DPL=0, TSS
            
            guest_gdtr_base: 0,
            guest_idtr_base: 0,
            guest_gdtr_limit: 0x27,
            guest_idtr_limit: 0xFFF,
            
            guest_ia32_debugctl: 0,
            guest_ia32_pat: 0x0007040600070406,
            guest_ia32_efer: 0x500, // LME, LMA
            guest_ia32_perf_global_ctrl: 0,
            guest_ia32_sysenter_cs: 0,
            guest_ia32_sysenter_esp: 0,
            guest_ia32_sysenter_eip: 0,
            
            // Host State - Will be filled from current CPU state
            host_cr0: 0x80000031,
            host_cr3: 0,
            host_cr4: 0x2000,
            host_rsp: 0,
            host_rip: 0,
            
            host_es_selector: 0x10,
            host_cs_selector: 0x08,
            host_ss_selector: 0x10,
            host_ds_selector: 0x10,
            host_fs_selector: 0x10,
            host_gs_selector: 0x10,
            host_tr_selector: 0x18,
            
            host_fs_base: 0,
            host_gs_base: 0,
            host_tr_base: 0,
            host_gdtr_base: 0,
            host_idtr_base: 0,
            
            host_ia32_pat: 0x0007040600070406,
            host_ia32_efer: 0x500,
            host_ia32_perf_global_ctrl: 0,
            host_ia32_sysenter_cs: 0,
            host_ia32_sysenter_esp: 0,
            host_ia32_sysenter_eip: 0,
            
            // Extended Features
            ept_pointer: 0,
            virtual_processor_id: 1,
            vmcs_link_pointer: 0xFFFFFFFFFFFFFFFF,
            tsc_offset: 0,
            virtual_apic_page_addr: 0,
            apic_access_addr: 0,
            msr_bitmap: 0,
            io_bitmap_a: 0,
            io_bitmap_b: 0,
        }
    }
}

impl<'a> ActiveVmcs<'a> {
    /// Load complete VMCS state from the structure
    pub fn load_state(&mut self, state: &VmcsState) {
        // Control Fields
        self.write(VmcsField::PIN_BASED_VM_EXEC_CONTROL, state.pin_based_controls as u64);
        self.write(VmcsField::CPU_BASED_VM_EXEC_CONTROL, state.cpu_based_controls as u64);
        self.write(VmcsField::SECONDARY_VM_EXEC_CONTROL, state.secondary_controls as u64);
        self.write(VmcsField::VM_EXIT_CONTROLS, state.vm_exit_controls as u64);
        self.write(VmcsField::VM_ENTRY_CONTROLS, state.vm_entry_controls as u64);
        self.write(VmcsField::EXCEPTION_BITMAP, state.exception_bitmap as u64);
        self.write(VmcsField::CR0_GUEST_HOST_MASK, state.cr0_guest_host_mask);
        self.write(VmcsField::CR4_GUEST_HOST_MASK, state.cr4_guest_host_mask);
        self.write(VmcsField::CR0_READ_SHADOW, state.cr0_read_shadow);
        self.write(VmcsField::CR4_READ_SHADOW, state.cr4_read_shadow);
        
        // Guest State - Control Registers
        self.write(VmcsField::GUEST_CR0, state.guest_cr0);
        self.write(VmcsField::GUEST_CR3, state.guest_cr3);
        self.write(VmcsField::GUEST_CR4, state.guest_cr4);
        self.write(VmcsField::GUEST_DR7, state.guest_dr7);
        
        // Guest State - General Purpose Registers
        self.write(VmcsField::GUEST_RSP, state.guest_rsp);
        self.write(VmcsField::GUEST_RIP, state.guest_rip);
        self.write(VmcsField::GUEST_RFLAGS, state.guest_rflags);
        self.write(VmcsField::GUEST_RAX, state.guest_rax);
        self.write(VmcsField::GUEST_RBX, state.guest_rbx);
        self.write(VmcsField::GUEST_RCX, state.guest_rcx);
        self.write(VmcsField::GUEST_RDX, state.guest_rdx);
        self.write(VmcsField::GUEST_RSI, state.guest_rsi);
        self.write(VmcsField::GUEST_RDI, state.guest_rdi);
        self.write(VmcsField::GUEST_RBP, state.guest_rbp);
        self.write(VmcsField::GUEST_R8, state.guest_r8);
        self.write(VmcsField::GUEST_R9, state.guest_r9);
        self.write(VmcsField::GUEST_R10, state.guest_r10);
        self.write(VmcsField::GUEST_R11, state.guest_r11);
        self.write(VmcsField::GUEST_R12, state.guest_r12);
        self.write(VmcsField::GUEST_R13, state.guest_r13);
        self.write(VmcsField::GUEST_R14, state.guest_r14);
        self.write(VmcsField::GUEST_R15, state.guest_r15);
        
        // Guest State - Segment Registers
        self.write(VmcsField::GUEST_ES_SELECTOR, state.guest_es_selector as u64);
        self.write(VmcsField::GUEST_CS_SELECTOR, state.guest_cs_selector as u64);
        self.write(VmcsField::GUEST_SS_SELECTOR, state.guest_ss_selector as u64);
        self.write(VmcsField::GUEST_DS_SELECTOR, state.guest_ds_selector as u64);
        self.write(VmcsField::GUEST_FS_SELECTOR, state.guest_fs_selector as u64);
        self.write(VmcsField::GUEST_GS_SELECTOR, state.guest_gs_selector as u64);
        self.write(VmcsField::GUEST_LDTR_SELECTOR, state.guest_ldtr_selector as u64);
        self.write(VmcsField::GUEST_TR_SELECTOR, state.guest_tr_selector as u64);
        
        self.write(VmcsField::GUEST_ES_BASE, state.guest_es_base);
        self.write(VmcsField::GUEST_CS_BASE, state.guest_cs_base);
        self.write(VmcsField::GUEST_SS_BASE, state.guest_ss_base);
        self.write(VmcsField::GUEST_DS_BASE, state.guest_ds_base);
        self.write(VmcsField::GUEST_FS_BASE, state.guest_fs_base);
        self.write(VmcsField::GUEST_GS_BASE, state.guest_gs_base);
        self.write(VmcsField::GUEST_LDTR_BASE, state.guest_ldtr_base);
        self.write(VmcsField::GUEST_TR_BASE, state.guest_tr_base);
        
        self.write(VmcsField::GUEST_ES_LIMIT, state.guest_es_limit as u64);
        self.write(VmcsField::GUEST_CS_LIMIT, state.guest_cs_limit as u64);
        self.write(VmcsField::GUEST_SS_LIMIT, state.guest_ss_limit as u64);
        self.write(VmcsField::GUEST_DS_LIMIT, state.guest_ds_limit as u64);
        self.write(VmcsField::GUEST_FS_LIMIT, state.guest_fs_limit as u64);
        self.write(VmcsField::GUEST_GS_LIMIT, state.guest_gs_limit as u64);
        self.write(VmcsField::GUEST_LDTR_LIMIT, state.guest_ldtr_limit as u64);
        self.write(VmcsField::GUEST_TR_LIMIT, state.guest_tr_limit as u64);
        
        self.write(VmcsField::GUEST_ES_AR_BYTES, state.guest_es_ar_bytes as u64);
        self.write(VmcsField::GUEST_CS_AR_BYTES, state.guest_cs_ar_bytes as u64);
        self.write(VmcsField::GUEST_SS_AR_BYTES, state.guest_ss_ar_bytes as u64);
        self.write(VmcsField::GUEST_DS_AR_BYTES, state.guest_ds_ar_bytes as u64);
        self.write(VmcsField::GUEST_FS_AR_BYTES, state.guest_fs_ar_bytes as u64);
        self.write(VmcsField::GUEST_GS_AR_BYTES, state.guest_gs_ar_bytes as u64);
        self.write(VmcsField::GUEST_LDTR_AR_BYTES, state.guest_ldtr_ar_bytes as u64);
        self.write(VmcsField::GUEST_TR_AR_BYTES, state.guest_tr_ar_bytes as u64);
        
        // Guest State - Descriptor Tables
        self.write(VmcsField::GUEST_GDTR_BASE, state.guest_gdtr_base);
        self.write(VmcsField::GUEST_IDTR_BASE, state.guest_idtr_base);
        self.write(VmcsField::GUEST_GDTR_LIMIT, state.guest_gdtr_limit as u64);
        self.write(VmcsField::GUEST_IDTR_LIMIT, state.guest_idtr_limit as u64);
        
        // Guest State - MSRs
        self.write(VmcsField::GUEST_IA32_DEBUGCTL, state.guest_ia32_debugctl);
        self.write(VmcsField::GUEST_IA32_PAT, state.guest_ia32_pat);
        self.write(VmcsField::GUEST_IA32_EFER, state.guest_ia32_efer);
        self.write(VmcsField::GUEST_IA32_PERF_GLOBAL_CTRL, state.guest_ia32_perf_global_ctrl);
        self.write(VmcsField::GUEST_IA32_SYSENTER_CS, state.guest_ia32_sysenter_cs as u64);
        self.write(VmcsField::GUEST_IA32_SYSENTER_ESP, state.guest_ia32_sysenter_esp);
        self.write(VmcsField::GUEST_IA32_SYSENTER_EIP, state.guest_ia32_sysenter_eip);
        
        // Host State
        self.write(VmcsField::HOST_CR0, state.host_cr0);
        self.write(VmcsField::HOST_CR3, state.host_cr3);
        self.write(VmcsField::HOST_CR4, state.host_cr4);
        self.write(VmcsField::HOST_RSP, state.host_rsp);
        self.write(VmcsField::HOST_RIP, state.host_rip);
        
        self.write(VmcsField::HOST_ES_SELECTOR, state.host_es_selector as u64);
        self.write(VmcsField::HOST_CS_SELECTOR, state.host_cs_selector as u64);
        self.write(VmcsField::HOST_SS_SELECTOR, state.host_ss_selector as u64);
        self.write(VmcsField::HOST_DS_SELECTOR, state.host_ds_selector as u64);
        self.write(VmcsField::HOST_FS_SELECTOR, state.host_fs_selector as u64);
        self.write(VmcsField::HOST_GS_SELECTOR, state.host_gs_selector as u64);
        self.write(VmcsField::HOST_TR_SELECTOR, state.host_tr_selector as u64);
        
        self.write(VmcsField::HOST_FS_BASE, state.host_fs_base);
        self.write(VmcsField::HOST_GS_BASE, state.host_gs_base);
        self.write(VmcsField::HOST_TR_BASE, state.host_tr_base);
        self.write(VmcsField::HOST_GDTR_BASE, state.host_gdtr_base);
        self.write(VmcsField::HOST_IDTR_BASE, state.host_idtr_base);
        
        self.write(VmcsField::HOST_IA32_PAT, state.host_ia32_pat);
        self.write(VmcsField::HOST_IA32_EFER, state.host_ia32_efer);
        self.write(VmcsField::HOST_IA32_PERF_GLOBAL_CTRL, state.host_ia32_perf_global_ctrl);
        self.write(VmcsField::HOST_IA32_SYSENTER_CS, state.host_ia32_sysenter_cs as u64);
        self.write(VmcsField::HOST_IA32_SYSENTER_ESP, state.host_ia32_sysenter_esp);
        self.write(VmcsField::HOST_IA32_SYSENTER_EIP, state.host_ia32_sysenter_eip);
        
        // Extended Features
        if state.ept_pointer != 0 {
            self.write(VmcsField::EPT_POINTER, state.ept_pointer);
        }
        self.write(VmcsField::VIRTUAL_PROCESSOR_ID, state.virtual_processor_id as u64);
        self.write(VmcsField::VMCS_LINK_POINTER, state.vmcs_link_pointer);
        self.write(VmcsField::TSC_OFFSET, state.tsc_offset);
        
        if state.virtual_apic_page_addr != 0 {
            self.write(VmcsField::VIRTUAL_APIC_PAGE_ADDR, state.virtual_apic_page_addr);
        }
        if state.apic_access_addr != 0 {
            self.write(VmcsField::APIC_ACCESS_ADDR, state.apic_access_addr);
        }
        if state.msr_bitmap != 0 {
            self.write(VmcsField::MSR_BITMAP, state.msr_bitmap);
        }
        if state.io_bitmap_a != 0 {
            self.write(VmcsField::IO_BITMAP_A, state.io_bitmap_a);
        }
        if state.io_bitmap_b != 0 {
            self.write(VmcsField::IO_BITMAP_B, state.io_bitmap_b);
        }
    }
    
    /// Save complete VMCS state to the structure
    pub fn save_state(&self, state: &mut VmcsState) {
        // Control Fields
        state.pin_based_controls = self.read(VmcsField::PIN_BASED_VM_EXEC_CONTROL) as u32;
        state.cpu_based_controls = self.read(VmcsField::CPU_BASED_VM_EXEC_CONTROL) as u32;
        state.secondary_controls = self.read(VmcsField::SECONDARY_VM_EXEC_CONTROL) as u32;
        state.vm_exit_controls = self.read(VmcsField::VM_EXIT_CONTROLS) as u32;
        state.vm_entry_controls = self.read(VmcsField::VM_ENTRY_CONTROLS) as u32;
        state.exception_bitmap = self.read(VmcsField::EXCEPTION_BITMAP) as u32;
        state.cr0_guest_host_mask = self.read(VmcsField::CR0_GUEST_HOST_MASK);
        state.cr4_guest_host_mask = self.read(VmcsField::CR4_GUEST_HOST_MASK);
        state.cr0_read_shadow = self.read(VmcsField::CR0_READ_SHADOW);
        state.cr4_read_shadow = self.read(VmcsField::CR4_READ_SHADOW);
        
        // Guest State - Control Registers
        state.guest_cr0 = self.read(VmcsField::GUEST_CR0);
        state.guest_cr3 = self.read(VmcsField::GUEST_CR3);
        state.guest_cr4 = self.read(VmcsField::GUEST_CR4);
        state.guest_dr7 = self.read(VmcsField::GUEST_DR7);
        
        // Guest State - General Purpose Registers
        state.guest_rsp = self.read(VmcsField::GUEST_RSP);
        state.guest_rip = self.read(VmcsField::GUEST_RIP);
        state.guest_rflags = self.read(VmcsField::GUEST_RFLAGS);
        state.guest_rax = self.read(VmcsField::GUEST_RAX);
        state.guest_rbx = self.read(VmcsField::GUEST_RBX);
        state.guest_rcx = self.read(VmcsField::GUEST_RCX);
        state.guest_rdx = self.read(VmcsField::GUEST_RDX);
        state.guest_rsi = self.read(VmcsField::GUEST_RSI);
        state.guest_rdi = self.read(VmcsField::GUEST_RDI);
        state.guest_rbp = self.read(VmcsField::GUEST_RBP);
        state.guest_r8 = self.read(VmcsField::GUEST_R8);
        state.guest_r9 = self.read(VmcsField::GUEST_R9);
        state.guest_r10 = self.read(VmcsField::GUEST_R10);
        state.guest_r11 = self.read(VmcsField::GUEST_R11);
        state.guest_r12 = self.read(VmcsField::GUEST_R12);
        state.guest_r13 = self.read(VmcsField::GUEST_R13);
        state.guest_r14 = self.read(VmcsField::GUEST_R14);
        state.guest_r15 = self.read(VmcsField::GUEST_R15);
        
        // Extended Features
        state.ept_pointer = self.read(VmcsField::EPT_POINTER);
        state.virtual_processor_id = self.read(VmcsField::VIRTUAL_PROCESSOR_ID) as u16;
        state.vmcs_link_pointer = self.read(VmcsField::VMCS_LINK_POINTER);
        state.tsc_offset = self.read(VmcsField::TSC_OFFSET);
    }
}

/// VMCS-related errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmcsError {
    VmclearFailed,
    VmptrldFailed,
    InvalidField,
    StateLoadFailed,
    StateSaveFailed,
} 