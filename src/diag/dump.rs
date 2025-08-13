#![allow(dead_code)]

use core::fmt::Write as _;

#[inline(always)]
fn read_cr0() -> u64 { let v: u64; unsafe { core::arch::asm!("mov {}, cr0", out(reg) v, options(nostack, preserves_flags)); } v }
#[inline(always)]
fn read_cr2() -> u64 { let v: u64; unsafe { core::arch::asm!("mov {}, cr2", out(reg) v, options(nostack, preserves_flags)); } v }
#[inline(always)]
fn read_cr3() -> u64 { let v: u64; unsafe { core::arch::asm!("mov {}, cr3", out(reg) v, options(nostack, preserves_flags)); } v }
#[inline(always)]
fn read_cr4() -> u64 { let v: u64; unsafe { core::arch::asm!("mov {}, cr4", out(reg) v, options(nostack, preserves_flags)); } v }
#[inline(always)]
fn read_rflags() -> u64 { let v: u64; unsafe { core::arch::asm!("pushfq; pop {}", out(reg) v, options(nostack, preserves_flags)); } v }
#[inline(always)]
fn read_cs() -> u16 { let v: u16; unsafe { core::arch::asm!("mov {}, cs", out(reg) v, options(nostack, preserves_flags)); } v }
#[inline(always)]
fn read_ss() -> u16 { let v: u16; unsafe { core::arch::asm!("mov {}, ss", out(reg) v, options(nostack, preserves_flags)); } v }
#[inline(always)]
fn read_ds() -> u16 { let v: u16; unsafe { core::arch::asm!("mov {}, ds", out(reg) v, options(nostack, preserves_flags)); } v }
#[inline(always)]
fn read_es() -> u16 { let v: u16; unsafe { core::arch::asm!("mov {}, es", out(reg) v, options(nostack, preserves_flags)); } v }
#[inline(always)]
fn read_fs() -> u16 { let v: u16; unsafe { core::arch::asm!("mov {}, fs", out(reg) v, options(nostack, preserves_flags)); } v }
#[inline(always)]
fn read_gs() -> u16 { let v: u16; unsafe { core::arch::asm!("mov {}, gs", out(reg) v, options(nostack, preserves_flags)); } v }

#[repr(C, packed)]
struct DescPtr { limit: u16, base: u64 }

#[inline(always)]
fn sidt() -> DescPtr { let mut dp = DescPtr { limit: 0, base: 0 }; unsafe { core::arch::asm!("sidt [{}]", in(reg) &mut dp, options(nostack, preserves_flags)); } dp }
#[inline(always)]
fn sgdt() -> DescPtr { let mut dp = DescPtr { limit: 0, base: 0 }; unsafe { core::arch::asm!("sgdt [{}]", in(reg) &mut dp, options(nostack, preserves_flags)); } dp }

pub fn dump_regs(system_table: &mut uefi::table::SystemTable<uefi::prelude::Boot>) {
    let stdout = system_table.stdout();
    let mut buf = [0u8; 256];
    let mut n = 0;
    let pairs: [(&[u8], u64); 6] = [
        (b"CR0=0x", read_cr0()),
        (b"CR2=0x", read_cr2()),
        (b"CR3=0x", read_cr3()),
        (b"CR4=0x", read_cr4()),
        (b"RFLAGS=0x", read_rflags()),
        (b" ", 0),
    ];
    for (lbl, val) in pairs.iter() {
        for &b in *lbl { buf[n] = b; n += 1; }
        if *lbl != b" " { n += crate::util::format::u64_hex(*val, &mut buf[n..]); }
        buf[n] = b' '; n += 1;
    }
    // Segments
    let segs: [(&[u8], u16); 6] = [
        (b"CS=", read_cs()), (b"SS=", read_ss()), (b"DS=", read_ds()),
        (b"ES=", read_es()), (b"FS=", read_fs()), (b"GS=", read_gs()),
    ];
    for (lbl, sel) in segs.iter() {
        for &b in *lbl { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(*sel as u32, &mut buf[n..]);
        buf[n] = b' '; n += 1;
    }
    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
}

pub fn dump_idt(system_table: &mut uefi::table::SystemTable<uefi::prelude::Boot>) {
    let dp = sidt();
    let stdout = system_table.stdout();
    let mut buf = [0u8; 96]; let mut n = 0;
    for &b in b"IDT limit=" { buf[n] = b; n += 1; }
    n += crate::firmware::acpi::u32_to_dec(dp.limit as u32, &mut buf[n..]);
    for &b in b" base=0x" { buf[n] = b; n += 1; }
    n += crate::util::format::u64_hex(dp.base, &mut buf[n..]);
    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
}

pub fn dump_gdt(system_table: &mut uefi::table::SystemTable<uefi::prelude::Boot>) {
    let dp = sgdt();
    let stdout = system_table.stdout();
    let mut buf = [0u8; 96]; let mut n = 0;
    for &b in b"GDT limit=" { buf[n] = b; n += 1; }
    n += crate::firmware::acpi::u32_to_dec(dp.limit as u32, &mut buf[n..]);
    for &b in b" base=0x" { buf[n] = b; n += 1; }
    n += crate::util::format::u64_hex(dp.base, &mut buf[n..]);
    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
}


