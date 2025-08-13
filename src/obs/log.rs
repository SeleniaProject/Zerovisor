#![allow(dead_code)]

use core::fmt::Write as _;
use uefi::prelude::Boot;
use uefi::table::SystemTable;

#[derive(Clone, Copy, Debug)]
pub enum Level { Info, Warn, Error }

pub fn write(system_table: &mut SystemTable<Boot>, level: Level, category: &str, message: &str) {
    let lang = crate::i18n::detect_lang(system_table);
    let stdout = system_table.stdout();
    let mut buf = [0u8; 192]; let mut n = 0;
    for &b in b"LOG [" { buf[n] = b; n += 1; }
    match level {
        Level::Info => { for &b in b"INFO" { buf[n] = b; n += 1; } }
        Level::Warn => { for &b in b"WARN" { buf[n] = b; n += 1; } }
        Level::Error => { for &b in b"ERROR" { buf[n] = b; n += 1; } }
    }
    for &b in b"] {" { buf[n] = b; n += 1; }
    for &b in category.as_bytes() { buf[n] = b; n += 1; }
    for &b in b"} " { buf[n] = b; n += 1; }
    for &b in message.as_bytes() { if n < buf.len() { buf[n] = b; n += 1; } }
    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    let _ = lang; // placeholder to keep language in scope for future localization
}


