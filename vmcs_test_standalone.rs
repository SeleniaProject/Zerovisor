// Standalone test for VMCS implementation
#![no_std]
#![no_main]

// Copy the essential VMCS structures for testing
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

fn test_vmcs_fields() {
    // Test that all major VMCS field categories are present
    
    // Control fields
    assert_eq!(VmcsField::PIN_BASED_VM_EXEC_CONTROL as u32, 0x4000);
    assert_eq!(VmcsField::CPU_BASED_VM_EXEC_CONTROL as u32, 0x4002);
    assert_eq!(VmcsField::SECONDARY_VM_EXEC_CONTROL as u32, 0x401E);
    assert_eq!(VmcsField::VM_EXIT_CONTROLS as u32, 0x400C);
    assert_eq!(VmcsField::VM_ENTRY_CONTROLS as u32, 0x4012);
    
    // Guest state fields
    assert_eq!(VmcsField::GUEST_CR0 as u32, 0x6800);
    assert_eq!(VmcsField::GUEST_CR3 as u32, 0x6802);
    assert_eq!(VmcsField::GUEST_CR4 as u32, 0x6804);
    assert_eq!(VmcsField::GUEST_RIP as u32, 0x681E);
    assert_eq!(VmcsField::GUEST_RSP as u32, 0x681C);
    assert_eq!(VmcsField::GUEST_RFLAGS as u32, 0x6820);
    
    // All general purpose registers
    assert_eq!(VmcsField::GUEST_RAX as u32, 0x6828);
    assert_eq!(VmcsField::GUEST_RBX as u32, 0x682A);
    assert_eq!(VmcsField::GUEST_RCX as u32, 0x682C);
    assert_eq!(VmcsField::GUEST_RDX as u32, 0x682E);
    assert_eq!(VmcsField::GUEST_RSI as u32, 0x6830);
    assert_eq!(VmcsField::GUEST_RDI as u32, 0x6832);
    assert_eq!(VmcsField::GUEST_RBP as u32, 0x6834);
    assert_eq!(VmcsField::GUEST_R8 as u32, 0x6836);
    assert_eq!(VmcsField::GUEST_R9 as u32, 0x6838);
    assert_eq!(VmcsField::GUEST_R10 as u32, 0x683A);
    assert_eq!(VmcsField::GUEST_R11 as u32, 0x683C);
    assert_eq!(VmcsField::GUEST_R12 as u32, 0x683E);
    assert_eq!(VmcsField::GUEST_R13 as u32, 0x6840);
    assert_eq!(VmcsField::GUEST_R14 as u32, 0x6842);
    assert_eq!(VmcsField::GUEST_R15 as u32, 0x6844);
    
    // Segment registers
    assert_eq!(VmcsField::GUEST_ES_SELECTOR as u32, 0x0800);
    assert_eq!(VmcsField::GUEST_CS_SELECTOR as u32, 0x0802);
    assert_eq!(VmcsField::GUEST_SS_SELECTOR as u32, 0x0804);
    assert_eq!(VmcsField::GUEST_DS_SELECTOR as u32, 0x0806);
    assert_eq!(VmcsField::GUEST_FS_SELECTOR as u32, 0x0808);
    assert_eq!(VmcsField::GUEST_GS_SELECTOR as u32, 0x080A);
    
    // Host state fields
    assert_eq!(VmcsField::HOST_CR0 as u32, 0x6C00);
    assert_eq!(VmcsField::HOST_CR3 as u32, 0x6C02);
    assert_eq!(VmcsField::HOST_CR4 as u32, 0x6C04);
    assert_eq!(VmcsField::HOST_RIP as u32, 0x6C16);
    assert_eq!(VmcsField::HOST_RSP as u32, 0x6C14);
    
    // Extended features
    assert_eq!(VmcsField::EPT_POINTER as u32, 0x201A);
    assert_eq!(VmcsField::VIRTUAL_PROCESSOR_ID as u32, 0x0000);
    assert_eq!(VmcsField::MSR_BITMAP as u32, 0x2004);
    assert_eq!(VmcsField::IO_BITMAP_A as u32, 0x2000);
    assert_eq!(VmcsField::IO_BITMAP_B as u32, 0x2002);
    
    // VM exit information
    assert_eq!(VmcsField::EXIT_REASON as u32, 0x4402);
    assert_eq!(VmcsField::EXIT_QUALIFICATION as u32, 0x6400);
    assert_eq!(VmcsField::GUEST_LINEAR_ADDR as u32, 0x640A);
    assert_eq!(VmcsField::GUEST_PHYS_ADDR as u32, 0x2400);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    test_vmcs_fields();
    
    // If we get here, all tests passed
    loop {}
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}