#![allow(dead_code)]

use core::fmt::Write as _;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use uefi::prelude::Boot;
use uefi::table::SystemTable;

#[derive(Clone, Copy, Debug)]
pub enum Level { Info, Warn, Error }

// Simple in-memory ring for recent log lines (ASCII only, no allocation)
const LOG_CAP: usize = 256;
const CAT_MAX: usize = 24;
const MSG_MAX: usize = 160;

#[derive(Clone, Copy)]
struct LogEntry { level: Level, cat_len: u8, msg_len: u8, cat: [u8; CAT_MAX], msg: [u8; MSG_MAX] }

static mut LOG_RING: [LogEntry; LOG_CAP] = [LogEntry { level: Level::Info, cat_len: 0, msg_len: 0, cat: [0; CAT_MAX], msg: [0; MSG_MAX] }; LOG_CAP];
static LOG_WIDX: AtomicUsize = AtomicUsize::new(0);
static LOG_MIN_LEVEL: AtomicU8 = AtomicU8::new(0); // 0=Info,1=Warn,2=Error

fn record_to_ring(level: Level, category: &str, message: &str) {
    let i = LOG_WIDX.fetch_add(1, Ordering::Relaxed) % LOG_CAP;
    unsafe {
        let e = &mut LOG_RING[i];
        e.level = level;
        let cb = category.as_bytes();
        let mb = message.as_bytes();
        let cl = cb.len().min(CAT_MAX);
        let ml = mb.len().min(MSG_MAX);
        e.cat_len = cl as u8;
        e.msg_len = ml as u8;
        core::ptr::copy_nonoverlapping(cb.as_ptr(), e.cat.as_mut_ptr(), cl);
        core::ptr::copy_nonoverlapping(mb.as_ptr(), e.msg.as_mut_ptr(), ml);
    }
}

pub fn write(system_table: &mut SystemTable<Boot>, level: Level, category: &str, message: &str) {
    // Record first to ring
    record_to_ring(level, category, message);
    // Then print to console
    let _lang = crate::i18n::detect_lang(system_table);
    // Respect minimal level for console output
    let min = LOG_MIN_LEVEL.load(Ordering::Relaxed);
    let lev_u8 = match level { Level::Info => 0, Level::Warn => 1, Level::Error => 2 };
    if lev_u8 < min { return; }
    let stdout = system_table.stdout();
    let mut buf = [0u8; 224]; let mut n = 0;
    for &b in b"LOG [" { buf[n] = b; n += 1; }
    match level {
        Level::Info => { for &b in b"INFO" { buf[n] = b; n += 1; } }
        Level::Warn => { for &b in b"WARN" { buf[n] = b; n += 1; } }
        Level::Error => { for &b in b"ERROR" { buf[n] = b; n += 1; } }
    }
    for &b in b"] {" { buf[n] = b; n += 1; }
    for &b in category.as_bytes() { if n < buf.len() { buf[n] = b; n += 1; } }
    for &b in b"} " { buf[n] = b; n += 1; }
    for &b in message.as_bytes() { if n < buf.len() { buf[n] = b; n += 1; } }
    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
}

pub fn dump(system_table: &mut SystemTable<Boot>) {
    let stdout = system_table.stdout();
    let cur = LOG_WIDX.load(Ordering::Relaxed);
    let start = cur.saturating_sub(LOG_CAP);
    for idx in start..cur {
        let e = unsafe { core::ptr::read_volatile(&LOG_RING[idx % LOG_CAP]) };
        let mut buf = [0u8; 224]; let mut n = 0;
        for &b in b"LOG [" { buf[n] = b; n += 1; }
        match e.level {
            Level::Info => { for &b in b"INFO" { buf[n] = b; n += 1; } }
            Level::Warn => { for &b in b"WARN" { buf[n] = b; n += 1; } }
            Level::Error => { for &b in b"ERROR" { buf[n] = b; n += 1; } }
        }
        for &b in b"] {" { buf[n] = b; n += 1; }
        let cl = e.cat_len.min(CAT_MAX as u8) as usize;
        let ml = e.msg_len.min(MSG_MAX as u8) as usize;
        for i in 0..cl { buf[n] = e.cat[i]; n += 1; }
        for &b in b"} " { buf[n] = b; n += 1; }
        for i in 0..ml { if n < buf.len() { buf[n] = e.msg[i]; n += 1; } }
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    }
}

pub fn dump_filtered(system_table: &mut SystemTable<Boot>, min_level: u8, cat_prefix: &str) {
    let stdout = system_table.stdout();
    let cur = LOG_WIDX.load(Ordering::Relaxed);
    let start = cur.saturating_sub(LOG_CAP);
    for idx in start..cur {
        let e = unsafe { core::ptr::read_volatile(&LOG_RING[idx % LOG_CAP]) };
        let lev_u8 = match e.level { Level::Info => 0, Level::Warn => 1, Level::Error => 2 };
        if lev_u8 < min_level { continue; }
        let cl = e.cat_len.min(CAT_MAX as u8) as usize;
        let ml = e.msg_len.min(MSG_MAX as u8) as usize;
        // Category prefix match (ASCII)
        let mut match_prefix = true;
        let p = cat_prefix.as_bytes();
        for i in 0..p.len() {
            if i >= cl { match_prefix = false; break; }
            if e.cat[i].to_ascii_lowercase() != p[i].to_ascii_lowercase() { match_prefix = false; break; }
        }
        if !match_prefix && !cat_prefix.is_empty() { continue; }
        let mut buf = [0u8; 224]; let mut n = 0;
        for &b in b"LOG [" { buf[n] = b; n += 1; }
        match e.level {
            Level::Info => { for &b in b"INFO" { buf[n] = b; n += 1; } }
            Level::Warn => { for &b in b"WARN" { buf[n] = b; n += 1; } }
            Level::Error => { for &b in b"ERROR" { buf[n] = b; n += 1; } }
        }
        for &b in b"] {" { buf[n] = b; n += 1; }
        for i in 0..cl { buf[n] = e.cat[i]; n += 1; }
        for &b in b"} " { buf[n] = b; n += 1; }
        for i in 0..ml { if n < buf.len() { buf[n] = e.msg[i]; n += 1; } }
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let s = match core::str::from_utf8(&buf[..n]) { Ok(v) => v, Err(_) => "\r\n" };
        let _ = stdout.write_str(s);
    }
}

/// Dump recent logs to an arbitrary byte writer (ASCII). Useful for panic-time dump.
pub fn dump_with_writer(mut write_bytes: impl FnMut(&[u8])) {
    let cur = LOG_WIDX.load(Ordering::Relaxed);
    let start = cur.saturating_sub(LOG_CAP);
    let mut line = [0u8; 224];
    for idx in start..cur {
        let e = unsafe { core::ptr::read_volatile(&LOG_RING[idx % LOG_CAP]) };
        let mut n = 0;
        for &b in b"LOG [" { line[n] = b; n += 1; }
        match e.level {
            Level::Info => { for &b in b"INFO" { line[n] = b; n += 1; } }
            Level::Warn => { for &b in b"WARN" { line[n] = b; n += 1; } }
            Level::Error => { for &b in b"ERROR" { line[n] = b; n += 1; } }
        }
        for &b in b"] {" { line[n] = b; n += 1; }
        let cl = e.cat_len.min(CAT_MAX as u8) as usize;
        let ml = e.msg_len.min(MSG_MAX as u8) as usize;
        for i in 0..cl { line[n] = e.cat[i]; n += 1; }
        for &b in b"} " { line[n] = b; n += 1; }
        for i in 0..ml { if n < line.len() { line[n] = e.msg[i]; n += 1; } }
        line[n] = b'\r'; n += 1; line[n] = b'\n'; n += 1;
        write_bytes(&line[..n]);
    }
}

#[inline(always)]
pub fn info(system_table: &mut SystemTable<Boot>, category: &str, message: &str) { write(system_table, Level::Info, category, message); }
#[inline(always)]
pub fn warn(system_table: &mut SystemTable<Boot>, category: &str, message: &str) { write(system_table, Level::Warn, category, message); }
#[inline(always)]
pub fn error(system_table: &mut SystemTable<Boot>, category: &str, message: &str) { write(system_table, Level::Error, category, message); }

#[inline(always)]
pub fn set_min_level_info() { LOG_MIN_LEVEL.store(0, Ordering::Relaxed); }
#[inline(always)]
pub fn set_min_level_warn() { LOG_MIN_LEVEL.store(1, Ordering::Relaxed); }
#[inline(always)]
pub fn set_min_level_error() { LOG_MIN_LEVEL.store(2, Ordering::Relaxed); }
#[inline(always)]
pub fn get_min_level() -> u8 { LOG_MIN_LEVEL.load(Ordering::Relaxed) }


