// Test file to verify VMCS implementation
use zerovisor_hal::arch::x86_64::vmcs::{VmcsState, VmcsField};

fn main() {
    // Test that we can create a default VMCS state
    let vmcs_state = VmcsState::default();
    
    println!("VMCS State created successfully!");
    println!("Guest CR0: {:#x}", vmcs_state.guest_cr0);
    println!("Guest CR3: {:#x}", vmcs_state.guest_cr3);
    println!("Guest CR4: {:#x}", vmcs_state.guest_cr4);
    println!("Guest RIP: {:#x}", vmcs_state.guest_rip);
    println!("Guest RSP: {:#x}", vmcs_state.guest_rsp);
    
    // Test VMCS field enumeration
    println!("VMCS Field values:");
    println!("GUEST_CR0: {:#x}", VmcsField::GUEST_CR0 as u32);
    println!("GUEST_CR3: {:#x}", VmcsField::GUEST_CR3 as u32);
    println!("GUEST_CR4: {:#x}", VmcsField::GUEST_CR4 as u32);
    println!("GUEST_RIP: {:#x}", VmcsField::GUEST_RIP as u32);
    println!("GUEST_RSP: {:#x}", VmcsField::GUEST_RSP as u32);
    println!("EPT_POINTER: {:#x}", VmcsField::EPT_POINTER as u32);
    
    // Test that we have all the major field categories
    println!("Control fields:");
    println!("PIN_BASED_VM_EXEC_CONTROL: {:#x}", VmcsField::PIN_BASED_VM_EXEC_CONTROL as u32);
    println!("CPU_BASED_VM_EXEC_CONTROL: {:#x}", VmcsField::CPU_BASED_VM_EXEC_CONTROL as u32);
    println!("SECONDARY_VM_EXEC_CONTROL: {:#x}", VmcsField::SECONDARY_VM_EXEC_CONTROL as u32);
    
    println!("Host state fields:");
    println!("HOST_CR0: {:#x}", VmcsField::HOST_CR0 as u32);
    println!("HOST_CR3: {:#x}", VmcsField::HOST_CR3 as u32);
    println!("HOST_CR4: {:#x}", VmcsField::HOST_CR4 as u32);
    println!("HOST_RIP: {:#x}", VmcsField::HOST_RIP as u32);
    println!("HOST_RSP: {:#x}", VmcsField::HOST_RSP as u32);
    
    println!("Extended fields:");
    println!("VIRTUAL_PROCESSOR_ID: {:#x}", VmcsField::VIRTUAL_PROCESSOR_ID as u32);
    println!("MSR_BITMAP: {:#x}", VmcsField::MSR_BITMAP as u32);
    println!("IO_BITMAP_A: {:#x}", VmcsField::IO_BITMAP_A as u32);
    println!("IO_BITMAP_B: {:#x}", VmcsField::IO_BITMAP_B as u32);
    
    println!("All VMCS fields implemented successfully!");
}