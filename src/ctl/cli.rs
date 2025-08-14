#![allow(dead_code)]

use uefi::prelude::Boot;
use uefi::table::SystemTable;
use core::fmt::Write as _;
use crate::i18n;
use crate::i18n::Lang;

use crate::virtio;
use crate::iommu::{vtd, amdv};

/// Very small interactive CLI on UEFI text console.
/// Supported commands:
///   help | info | virtio | iommu | quit
pub fn run_cli(system_table: &mut SystemTable<Boot>) {
    let lang = crate::i18n::detect_lang(system_table);
    {
        let stdout = system_table.stdout();
        let _ = stdout.write_str("CLI: type 'help' for commands\r\n");
    }
    // Buffer for input line (ASCII only)
    let mut buf = [0u8; 80];
    loop {
        // Prompt
        {
            let stdout = system_table.stdout();
            let _ = stdout.write_str("> ");
        }
        let mut len = 0usize;
        // Reset input and read keys until Enter
        {
            let stdin = system_table.stdin();
            let _ = stdin.reset(false);
        }
        'readline: loop {
            let key_res = {
                let stdin = system_table.stdin();
                stdin.read_key()
            };
            match key_res {
                Ok(Some(k)) => {
                    // Key printable path: try unicode
                    match k {
                        uefi::proto::console::text::Key::Printable(ch) => {
                            let c: char = ch.into();
                            if c == '\r' || c == '\n' {
                                {
                                    let stdout = system_table.stdout();
                                    let _ = stdout.write_str("\r\n");
                                }
                                break 'readline;
                            }
                            if c == '\u{8}' || c == '\u{7f}' { // backspace/del (no-echo)
                                if len > 0 { len -= 1; }
                            } else if c.is_ascii() && len < buf.len() {
                                buf[len] = c as u8; len += 1;
                            }
                        }
                        uefi::proto::console::text::Key::Special(_) => {
                            // Ignore
                        }
                    }
                }
                Ok(None) => { let _ = system_table.boot_services().stall(1000); }
                Err(_) => { let _ = system_table.boot_services().stall(1000); }
            }
        }
        // Parse line
        let cmd = core::str::from_utf8(&buf[..len]).unwrap_or("").trim();
        if cmd.eq_ignore_ascii_case("help") {
            let stdout = system_table.stdout();
            let _ = stdout.write_str("Commands: help | version | info | virtio | iommu | pci | pci find [vid=<hex>] [did=<hex>] | pci class <cc> <sc> | vm | vm pause|vm resume | vm list | migrate | migrate start|migrate start id=<id>|migrate scan [clear] | migrate plan | migrate export start=<hex> len=<hex> [sink=console|null|buffer|snp|virtio] | migrate precopy [rounds=<n>] [clear] [sink=console|null|buffer|snp|virtio] | migrate precopy-throttle [rounds=<n>] [clear] [sink=console|null|buffer|snp|virtio] rate=<kbps> | migrate send-dirty [compress] [sink=console|null|buffer|snp|virtio] | migrate resend from=<seq> [count=<n>] [compress] [sink=console|null|buffer|snp|virtio] | migrate ctrl ack <seq> [sink=console|null|buffer|snp|virtio] | migrate ctrl nak <seq> [sink=console|null|buffer|snp|virtio] | migrate chan new [pages=<n>] | migrate chan clear | migrate chan dump [len=<n>] [hex] | migrate chan chunk [get|set <bytes>] | migrate chan consume <bytes> | migrate net mac [get|set xx:xx:xx:xx:xx:xx] | migrate net mtu [get|set <n>] | migrate net ether [get|set <hex>] | snp [discover|use <idx>|info|pump [limit=<n>] | poll [cycles=<n>] [sleep=<us>] [ctrl] [verify] [empty=<n>]] | virtio net pump [limit=<n>] | virtio net poll [cycles=<n>] [sleep=<us>] [ctrl] [verify] [empty=<n>] | migrate ctrl resend-sink [console|null|buffer|snp|virtio] | migrate ctrl auto-ack [on|off] | migrate ctrl auto-nak [on|off] | migrate default-sink [console|null|buffer|snp|virtio] | migrate txlog [count=<n>] | migrate reset | migrate cfg save|load | migrate session start|elapsed|bw|bw_net | migrate summary | migrate handle-ctrl [limit=<n>] | migrate verify [limit=<n>] [quiet] | migrate replay [pages=<n>] | migrate export-dirty | migrate stop | trace | trace clear | metrics | metrics clear | audit | logs | logs filter [level=<info|warn|error>] [cat=<prefix>] | loglevel [info|warn|error] | time [show|wait <usec> [busy|stall]] | wdog [off|<secs>] | sec | lang [en|ja|zh|auto] | dump [regs|idt|gdt] | quit\r\n");
        if cmd.starts_with("virtio net pump") {
            // virtio net pump [limit=<n>]
            let rest = cmd.strip_prefix("virtio net pump").unwrap_or("").trim();
            let mut limit: usize = 0;
            for tok in rest.split_whitespace() { if let Some(v) = tok.strip_prefix("limit=") { let _ = v.parse::<usize>().map(|n| limit = n); } }
            crate::virtio::net::rx_pump(system_table, limit);
            continue;
        }
        if cmd.starts_with("virtio net poll") {
            // virtio net poll [cycles=<n>] [sleep=<us>] [ctrl] [verify] [empty=<n>]
            let rest = cmd.strip_prefix("virtio net poll").unwrap_or("").trim();
            let mut cycles: usize = 0; let mut sleep_us: usize = 1000; let mut do_ctrl = false; let mut do_verify = false; let mut empty: usize = 0;
            for tok in rest.split_whitespace() {
                if let Some(v) = tok.strip_prefix("cycles=") { let _ = v.parse::<usize>().map(|n| cycles = n); continue; }
                if let Some(v) = tok.strip_prefix("sleep=") { let _ = v.parse::<usize>().map(|n| sleep_us = n); continue; }
                if let Some(v) = tok.strip_prefix("empty=") { let _ = v.parse::<usize>().map(|n| empty = n); continue; }
                if tok.eq_ignore_ascii_case("ctrl") { do_ctrl = true; continue; }
                if tok.eq_ignore_ascii_case("verify") { do_verify = true; continue; }
            }
            crate::migrate::virtio_poll_ex(system_table, cycles, sleep_us, do_ctrl, do_verify, empty);
            continue;
        }
        if cmd.starts_with("migrate default-sink ") {
            let v = &cmd[21..].trim();
            let sink = if v.eq_ignore_ascii_case("console") { crate::migrate::ExportSink::Console }
                       else if v.eq_ignore_ascii_case("null") { crate::migrate::ExportSink::Null }
                       else if v.eq_ignore_ascii_case("buffer") { crate::migrate::ExportSink::Buffer }
                       else if v.eq_ignore_ascii_case("snp") { crate::migrate::ExportSink::Snp }
                       else if v.eq_ignore_ascii_case("virtio") { crate::migrate::ExportSink::Virtio }
                       else { crate::migrate::ExportSink::Buffer };
            crate::migrate::set_default_sink(sink);
            let _ = system_table.stdout().write_str("migrate: default sink updated\r\n");
            continue;
        }
        if cmd.starts_with("migrate ctrl auto-ack ") {
            let v = &cmd[22..].trim();
            crate::migrate::ctrl_set_auto_ack(v.eq_ignore_ascii_case("on"));
            let _ = system_table.stdout().write_str("migrate: ctrl auto-ack updated\r\n");
            continue;
        }
        if cmd.starts_with("migrate ctrl auto-nak ") {
            let v = &cmd[22..].trim();
            crate::migrate::ctrl_set_auto_nak(v.eq_ignore_ascii_case("on"));
            let _ = system_table.stdout().write_str("migrate: ctrl auto-nak updated\r\n");
            continue;
        }
        if cmd.starts_with("migrate ctrl resend-sink ") {
            let v = &cmd[25..].trim();
            let sink = if v.eq_ignore_ascii_case("console") { crate::migrate::ExportSink::Console }
                       else if v.eq_ignore_ascii_case("null") { crate::migrate::ExportSink::Null }
                       else if v.eq_ignore_ascii_case("buffer") { crate::migrate::ExportSink::Buffer }
                       else if v.eq_ignore_ascii_case("snp") { crate::migrate::ExportSink::Snp }
                       else { crate::migrate::ExportSink::Buffer };
            crate::migrate::ctrl_set_resend_sink(sink);
            let _ = system_table.stdout().write_str("migrate: ctrl resend-sink updated\r\n");
            continue;
        }
        if cmd.eq_ignore_ascii_case("snp discover") {
            crate::migrate::snp_discover(system_table);
            continue;
        }
        if cmd.starts_with("snp use ") {
            let rest = &cmd[8..].trim();
            if let Ok(idx) = rest.parse::<usize>() { crate::migrate::snp_use(system_table, idx); continue; }
            let _ = system_table.stdout().write_str("usage: snp use <index>\r\n");
            continue;
        }
        if cmd.eq_ignore_ascii_case("snp info") {
            crate::migrate::snp_info(system_table);
            continue;
        }
        if cmd.starts_with("snp pump") {
            // snp pump [limit=<n>]
            let rest = cmd.strip_prefix("snp pump").unwrap_or("").trim();
            let mut limit: usize = 0;
            for tok in rest.split_whitespace() { if let Some(v) = tok.strip_prefix("limit=") { let _ = v.parse::<usize>().map(|n| limit = n); } }
            crate::migrate::snp_pump(system_table, limit);
            continue;
        }
        if cmd.starts_with("snp poll") {
            // snp poll [cycles=<n>] [sleep=<us>] [ctrl] [verify] [empty=<n>]
            let rest = cmd.strip_prefix("snp poll").unwrap_or("").trim();
            let mut cycles: usize = 0; let mut sleep_us: usize = 1000; let mut do_ctrl = false; let mut do_verify = false; let mut empty: usize = 0;
            for tok in rest.split_whitespace() {
                if let Some(v) = tok.strip_prefix("cycles=") { let _ = v.parse::<usize>().map(|n| cycles = n); continue; }
                if let Some(v) = tok.strip_prefix("sleep=") { let _ = v.parse::<usize>().map(|n| sleep_us = n); continue; }
                if let Some(v) = tok.strip_prefix("empty=") { let _ = v.parse::<usize>().map(|n| empty = n); continue; }
                if tok.eq_ignore_ascii_case("ctrl") { do_ctrl = true; continue; }
                if tok.eq_ignore_ascii_case("verify") { do_verify = true; continue; }
            }
            crate::migrate::snp_poll_ex(system_table, cycles, sleep_us, do_ctrl, do_verify, empty);
            continue;
        }
        if cmd.eq_ignore_ascii_case("migrate summary") {
            crate::migrate::summary(system_table);
            continue;
        }
        if cmd.starts_with("migrate session ") {
            let rest = &cmd[16..].trim();
            if rest.eq_ignore_ascii_case("start") { crate::migrate::session_start(system_table); let _ = system_table.stdout().write_str("migrate: session start\r\n"); continue; }
            if rest.eq_ignore_ascii_case("elapsed") { crate::migrate::session_elapsed(system_table); continue; }
            if rest.eq_ignore_ascii_case("bw") { crate::migrate::session_bw(system_table); continue; }
            if rest.eq_ignore_ascii_case("bw_net") { crate::migrate::session_bw_net(system_table); continue; }
            let _ = system_table.stdout().write_str("usage: migrate session [start|elapsed|bw|bw_net]\r\n");
            continue;
        }
        if cmd.starts_with("migrate txlog") {
            // migrate txlog [count=<n>]
            let rest = cmd.strip_prefix("migrate txlog").unwrap_or("").trim();
            let mut count: usize = 32;
            for tok in rest.split_whitespace() { if let Some(v) = tok.strip_prefix("count=") { let _ = v.parse::<usize>().map(|n| count = n); } }
            crate::migrate::txlog_dump(system_table, count);
            continue;
        }
        if cmd.eq_ignore_ascii_case("migrate reset") {
            crate::migrate::reset(system_table);
            let _ = system_table.stdout().write_str("migrate: reset done\r\n");
            continue;
        }
        if cmd.starts_with("migrate cfg ") {
            let rest = &cmd[12..].trim();
            if rest.eq_ignore_ascii_case("save") { crate::migrate::cfg_save(system_table); let _ = system_table.stdout().write_str("migrate: cfg saved\r\n"); continue; }
            if rest.eq_ignore_ascii_case("load") { crate::migrate::cfg_load(system_table); let _ = system_table.stdout().write_str("migrate: cfg loaded\r\n"); continue; }
            let _ = system_table.stdout().write_str("usage: migrate cfg [save|load]\r\n");
            continue;
        }
            let _ = stdout.write_str("  iommu: info | units | root <bus> | lsctx <bus> | dump <bus:dev.func> | plan | validate | verify | verify-map | xlate bdf=<seg:bus:dev.func> iova=<hex> | walk bdf=<seg:bus:dev.func> iova=<hex> | apply | apply-refresh | apply-safe | quick | sync | invalidate | invalidate dom=<id> | invalidate bdf=<seg:bus:dev.func> | hard-invalidate | fsts | fclear | stats | summary | cfg save|cfg load | selftest [quick] [no-apply] [no-inv] [dom=<id>] [walk=<n>] [xlate=<n>] | sample dom=<id> iova=<hex> [count=<n>] [walk] [xlate] | amdv enable|amdv disable | amdv quick\r\n");
            let _ = stdout.write_str("  dom: new | destroy <id> | purge <id> | seg:bus:dev.func assign <id> | seg:bus:dev.func unassign | list | map dom=<id> iova=<hex> pa=<hex> len=<hex> perm=[rwx] | unmap dom=<id> iova=<hex> len=<hex> | mappings | dump\r\n");
            continue;
        }
        if cmd.eq_ignore_ascii_case("version") {
            let stdout = system_table.stdout();
            let mut buf = [0u8; 192]; let mut n = 0;
            for &b in b"zerovisor " { buf[n] = b; n += 1; }
            for &b in env!("CARGO_PKG_VERSION").as_bytes() { buf[n] = b; n += 1; }
            for &b in b" (x86_64-uefi)" { buf[n] = b; n += 1; }
            // Print compiled feature flags for quick introspection
            for &b in b" features=[" { buf[n] = b; n += 1; }
            let mut first = true;
            #[cfg(feature = "snp")]
            { if !first { buf[n] = b' '; n += 1; } for &c in b"snp" { buf[n] = c; n += 1; } first = false; }
            #[cfg(feature = "virtio-net")]
            { if !first { buf[n] = b' '; n += 1; } for &c in b"virtio-net" { buf[n] = c; n += 1; } first = false; }
            if first { for &c in b"none" { buf[n] = c; n += 1; } }
            for &b in b"]\r\n" { buf[n] = b; n += 1; }
            let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
            continue;
        }
        if cmd.starts_with("dom ") {
            let rest = &cmd[4..];
            if let Some(idstr) = rest.strip_prefix("destroy ") {
                if let Ok(id) = idstr.trim().parse::<u16>() {
                    let ok = crate::iommu::state::destroy_domain(id);
                    let stdout = system_table.stdout();
                    let _ = stdout.write_str(if ok { "domain destroyed\r\n" } else { "domain not found\r\n" });
                    continue;
                }
            }
            if let Some(idstr) = rest.strip_prefix("purge ") {
                if let Ok(id) = idstr.trim().parse::<u16>() {
                    let n = crate::iommu::state::remove_mappings_for_domain(id);
                    let stdout = system_table.stdout();
                    let mut buf = [0u8; 64]; let mut nbytes = 0;
                    for &b in b"purged maps=" { buf[nbytes] = b; nbytes += 1; }
                    nbytes += crate::firmware::acpi::u32_to_dec(n, &mut buf[nbytes..]);
                    buf[nbytes] = b'\r'; nbytes += 1; buf[nbytes] = b'\n'; nbytes += 1;
                    let _ = stdout.write_str(core::str::from_utf8(&buf[..nbytes]).unwrap_or("\r\n"));
                    continue;
                }
            }
            if rest.eq_ignore_ascii_case("new") {
                if let Some(id) = crate::iommu::state::create_domain() {
                    let stdout = system_table.stdout();
                    let mut buf = [0u8; 64]; let mut n = 0;
                    for &b in b"domain id=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(id as u32, &mut buf[n..]);
                    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                }
                continue;
            }
            if let Some(idx) = rest.find(" assign ") {
                let left = &rest[..idx];
                let right = &rest[idx+8..]; // after " assign "
                // left: "seg:bus:dev.func"  right: domain id (decimal)
                let parse_bdf = |s: &str| -> Option<(u16,u8,u8,u8)> {
                    let mut parts = s.split(':');
                    let seg = parts.next()?.trim();
                    let bus = parts.next()?.trim();
                    let devfunc = parts.next()?.trim();
                    let mut df = devfunc.split('.');
                    let dev = df.next()?.trim();
                    let func = df.next()?.trim();
                    let seg = u16::from_str_radix(seg, 16).ok()?;
                    let bus = u8::from_str_radix(bus, 16).ok()?;
                    let dev = u8::from_str_radix(dev, 16).ok()?;
                    let func = u8::from_str_radix(func, 16).ok()?;
                    Some((seg, bus, dev, func))
                };
                if let Some((seg,bus,dev,func)) = parse_bdf(left) {
                    if let Ok(domid) = right.trim().parse::<u16>() {
                        let ok = crate::iommu::state::assign_device(seg,bus,dev,func,domid);
                        let stdout = system_table.stdout();
                        let _ = stdout.write_str(if ok { "assigned\r\n" } else { "assign failed\r\n" });
                    }
                }
                continue;
            }
            if let Some(idx) = rest.find(" unassign ") {
                let left = &rest[..idx];
                let parse_bdf = |s: &str| -> Option<(u16,u8,u8,u8)> {
                    let mut parts = s.split(':');
                    let seg = parts.next()?.trim();
                    let bus = parts.next()?.trim();
                    let devfunc = parts.next()?.trim();
                    let mut df = devfunc.split('.');
                    let dev = df.next()?.trim();
                    let func = df.next()?.trim();
                    Some((u16::from_str_radix(seg,16).ok()?, u8::from_str_radix(bus,16).ok()?, u8::from_str_radix(dev,16).ok()?, u8::from_str_radix(func,16).ok()?))
                };
                if let Some((seg,bus,dev,func)) = parse_bdf(left) {
                    let ok = crate::iommu::state::unassign_device(seg,bus,dev,func);
                    let stdout = system_table.stdout();
                    let _ = stdout.write_str(if ok { "unassigned\r\n" } else { "unassign failed\r\n" });
                }
                continue;
            }
            if rest.eq_ignore_ascii_case("list") {
                let stdout = system_table.stdout();
                // list domains
                let _ = stdout.write_str("domains:\r\n");
                crate::iommu::state::list_domains(|id| {
                    let mut buf = [0u8; 32]; let mut n = 0;
                    for &b in b"  id=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(id as u32, &mut buf[n..]);
                    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                });
                // list assignments
                let _ = stdout.write_str("assignments:\r\n");
                crate::iommu::state::list_assignments(|seg,bus,dev,func,dom| {
                    let mut buf = [0u8; 96]; let mut n = 0;
                    for &b in b"  " { buf[n] = b; n += 1; }
                    for &b in b"seg=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
                    for &b in b" bus=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
                    for &b in b" dev=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(dev as u32, &mut buf[n..]);
                    for &b in b" fn=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(func as u32, &mut buf[n..]);
                    for &b in b" dom=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(dom as u32, &mut buf[n..]);
                    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                });
                continue;
            }
            if let Some(idx) = rest.find(" map ") {
                let right = &rest[idx+5..];
                let mut domid: Option<u16> = None; let mut iova: Option<u64> = None; let mut pa: Option<u64> = None; let mut len: Option<u64> = None; let mut r=false; let mut w=false; let mut x=false;
                for tok in right.split_whitespace() {
                    if let Some(v) = tok.strip_prefix("dom=") { domid = v.parse::<u16>().ok(); continue; }
                    if let Some(v) = tok.strip_prefix("iova=") { iova = u64::from_str_radix(v.trim_start_matches("0x"), 16).ok(); continue; }
                    if let Some(v) = tok.strip_prefix("pa=") { pa = u64::from_str_radix(v.trim_start_matches("0x"), 16).ok(); continue; }
                    if let Some(v) = tok.strip_prefix("len=") { len = u64::from_str_radix(v.trim_start_matches("0x"), 16).ok(); continue; }
                    if let Some(v) = tok.strip_prefix("perm=") { r = v.contains('r'); w = v.contains('w'); x = v.contains('x'); continue; }
                }
                if let (Some(domid), Some(iova), Some(pa), Some(len)) = (domid, iova, pa, len) {
                    let ok = crate::iommu::state::add_mapping(domid, iova, pa, len, r, w, x);
                    let stdout = system_table.stdout();
                    if ok { let _ = stdout.write_str("mapped\r\n"); crate::iommu::vtd::apply_mappings(system_table); } else { let _ = stdout.write_str("map failed\r\n"); }
                }
                continue;
            }
            if let Some(idx) = rest.find(" unmap ") {
                let right = &rest[idx+7..];
                let mut domid: Option<u16> = None; let mut iova: Option<u64> = None; let mut len: Option<u64> = None;
                for tok in right.split_whitespace() {
                    if let Some(v) = tok.strip_prefix("dom=") { domid = v.parse::<u16>().ok(); continue; }
                    if let Some(v) = tok.strip_prefix("iova=") { iova = u64::from_str_radix(v.trim_start_matches("0x"), 16).ok(); continue; }
                    if let Some(v) = tok.strip_prefix("len=") { len = u64::from_str_radix(v.trim_start_matches("0x"), 16).ok(); continue; }
                }
                if let (Some(domid), Some(iova), Some(len)) = (domid, iova, len) {
                    let ok = crate::iommu::state::remove_mapping(domid, iova, len);
                    if ok {
                        crate::iommu::vtd::unmap_range(system_table, domid, iova, len);
                        let stdout = system_table.stdout();
                        let _ = stdout.write_str("unmapped\r\n");
                    } else {
                        let stdout = system_table.stdout();
                        let _ = stdout.write_str("unmap failed\r\n");
                    }
                }
                continue;
            }
            if rest.eq_ignore_ascii_case("dump") {
                let stdout = system_table.stdout();
                let _ = stdout.write_str("domains:\r\n");
                crate::iommu::state::list_domains(|id| {
                    let mut buf = [0u8; 32]; let mut n = 0;
                    for &b in b"  id=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(id as u32, &mut buf[n..]);
                    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                });
                let _ = stdout.write_str("assignments:\r\n");
                crate::iommu::state::list_assignments(|seg,bus,dev,func,dom| {
                    let mut buf = [0u8; 96]; let mut n = 0;
                    for &b in b"  seg=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
                    for &b in b" bus=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
                    for &b in b" dev=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(dev as u32, &mut buf[n..]);
                    for &b in b" fn=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(func as u32, &mut buf[n..]);
                    for &b in b" dom=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(dom as u32, &mut buf[n..]);
                    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                });
                let _ = stdout.write_str("mappings:\r\n");
                crate::iommu::state::list_mappings(|dom,iova,pa,len,r,w,x| {
                    let mut buf = [0u8; 128]; let mut n = 0;
                    for &b in b"  dom=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(dom as u32, &mut buf[n..]);
                    for &b in b" iova=0x" { buf[n] = b; n += 1; }
                    n += crate::util::format::u64_hex(iova, &mut buf[n..]);
                    for &b in b" pa=0x" { buf[n] = b; n += 1; }
                    n += crate::util::format::u64_hex(pa, &mut buf[n..]);
                    for &b in b" len=0x" { buf[n] = b; n += 1; }
                    n += crate::util::format::u64_hex(len, &mut buf[n..]);
                    for &b in b" perm=" { buf[n] = b; n += 1; }
                    buf[n] = if r { b'r' } else { b'-' }; n += 1;
                    buf[n] = if w { b'w' } else { b'-' }; n += 1;
                    buf[n] = if x { b'x' } else { b'-' }; n += 1;
                    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                });
                continue;
            }
            if rest.eq_ignore_ascii_case("mappings") {
                let stdout = system_table.stdout();
                crate::iommu::state::list_mappings(|dom,iova,pa,len,r,w,x| {
                    let mut buf = [0u8; 128]; let mut n = 0;
                    for &b in b"  dom=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(dom as u32, &mut buf[n..]);
                    for &b in b" iova=0x" { buf[n] = b; n += 1; }
                    n += crate::util::format::u64_hex(iova, &mut buf[n..]);
                    for &b in b" pa=0x" { buf[n] = b; n += 1; }
                    n += crate::util::format::u64_hex(pa, &mut buf[n..]);
                    for &b in b" len=0x" { buf[n] = b; n += 1; }
                    n += crate::util::format::u64_hex(len, &mut buf[n..]);
                    for &b in b" perm=" { buf[n] = b; n += 1; }
                    buf[n] = if r { b'r' } else { b'-' }; n += 1;
                    buf[n] = if w { b'w' } else { b'-' }; n += 1;
                    buf[n] = if x { b'x' } else { b'-' }; n += 1;
                    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                });
                continue;
            }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("usage: dom new | dom seg:bus:dev.func assign <id> | dom seg:bus:dev.func unassign | dom list | dom map dom=<id> iova=<hex> pa=<hex> len=<hex> perm=[rwx] | dom unmap dom=<id> iova=<hex> len=<hex> | dom mappings | dom dump\r\n");
            continue;
        }
        if cmd.eq_ignore_ascii_case("quit") || cmd.eq_ignore_ascii_case("exit") {
            let stdout = system_table.stdout();
            let _ = stdout.write_str("Bye\r\n");
            break;
        }
        if cmd.eq_ignore_ascii_case("info") {
            let stdout = system_table.stdout();
            let _ = stdout.write_str(crate::i18n::t(lang, crate::i18n::key::ENV));
            continue;
        }
        if cmd.eq_ignore_ascii_case("virtio") {
            virtio::devices_report_minimal(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu") || cmd.eq_ignore_ascii_case("iommu info") {
            vtd::probe_and_report(system_table);
            vtd::report_details(system_table);
            vtd::dump_device_scopes(system_table);
            crate::iommu::report_dmar_scoped_devices_with_ids(system_table);
            amdv::probe_and_report(system_table);
            amdv::report_units(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu amdv enable") {
            crate::iommu::amdv::minimal_init(system_table);
            crate::iommu::amdv::enable_translation_all(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu amdv quick") {
            crate::iommu::amdv::minimal_init(system_table);
            crate::iommu::amdv::enable_translation_all(system_table);
            crate::iommu::amdv::report_units(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu amdv disable") {
            crate::iommu::amdv::disable_translation_all(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu summary") {
            vtd::report_summary(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu stats") {
            vtd::report_stats(system_table);
            continue;
        }
        if cmd.starts_with("iommu sample ") {
            // iommu sample dom=<id> iova=<hex> [count=<n>] [walk] [xlate]
            let rest = &cmd[13..].trim();
            let mut dom: Option<u16> = None; let mut iova: Option<u64> = None; let mut count: usize = 1; let mut do_walk = true; let mut do_xlate = true;
            for tok in rest.split_whitespace() {
                if let Some(v) = tok.strip_prefix("dom=") { dom = v.parse::<u16>().ok(); continue; }
                if let Some(v) = tok.strip_prefix("iova=") { iova = u64::from_str_radix(v.trim_start_matches("0x"), 16).ok(); continue; }
                if let Some(v) = tok.strip_prefix("count=") { let _ = v.parse::<usize>().map(|n| count = n); continue; }
                if tok.eq_ignore_ascii_case("walk") { do_xlate = false; continue; }
                if tok.eq_ignore_ascii_case("xlate") { do_walk = false; continue; }
            }
            if let (Some(domid), Some(iova)) = (dom, iova) { vtd::sample_walk_xlate_for_domain(system_table, domid, iova, count, do_walk, do_xlate); continue; }
            let _ = system_table.stdout().write_str("usage: iommu sample dom=<id> iova=<hex> [count=<n>] [walk] [xlate]\r\n");
            continue;
        }
        if cmd.starts_with("iommu selftest") {
            // iommu selftest [quick] [no-apply] [no-inv] [dom=<id>] [walk=<n>] [xlate=<n>]
            let rest = cmd.strip_prefix("iommu selftest").unwrap_or("").trim();
            let mut cfg = vtd::SelfTestConfig::default();
            for tok in rest.split_whitespace() {
                if tok.eq_ignore_ascii_case("quick") { cfg.quick = true; continue; }
                if tok.eq_ignore_ascii_case("no-apply") { cfg.do_apply = false; continue; }
                if tok.eq_ignore_ascii_case("no-inv") { cfg.do_invalidate = false; continue; }
                if let Some(v) = tok.strip_prefix("dom=") { if let Ok(id) = v.parse::<u16>() { cfg.test_domain = Some(id); } continue; }
                if let Some(v) = tok.strip_prefix("walk=") { if let Ok(n) = v.parse::<u32>() { cfg.walk_samples = n; } continue; }
                if let Some(v) = tok.strip_prefix("xlate=") { if let Ok(n) = v.parse::<u32>() { cfg.xlate_samples = n; } continue; }
            }
            vtd::selftest(system_table, cfg);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu enable") {
            vtd::enable_translation_all(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu disable") {
            vtd::disable_translation_all(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu plan") {
            vtd::plan_assignments(system_table);
            continue;
        }
        if cmd.starts_with("iommu plan dom=") {
            let v = &cmd[15..].trim();
            if let Ok(domid) = v.parse::<u16>() { vtd::plan_assignments_for_domain(system_table, domid); continue; }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("usage: iommu plan dom=<id>\r\n");
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu units") {
            vtd::list_units(system_table);
            continue;
        }
        if cmd.starts_with("iommu te ") {
            // iommu te <index> on|off
            let args = &cmd[9..].trim();
            let mut parts = args.split_whitespace();
            if let (Some(i), Some(sw)) = (parts.next(), parts.next()) {
                if let Ok(idx) = i.parse::<usize>() {
                    vtd::set_te_for_unit(system_table, idx, sw.eq_ignore_ascii_case("on"));
                    continue;
                }
            }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("usage: iommu te <index> <on|off>\r\n");
            continue;
        }
        if cmd.starts_with("iommu lsctx ") {
            let args = &cmd[12..].trim();
            if let Ok(bus) = u8::from_str_radix(args, 16) {
                vtd::list_bus_contexts(system_table, bus);
                continue;
            }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("usage: iommu lsctx <bus> (hex)\r\n");
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu validate") {
            vtd::validate_assignments(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu cfg save") {
            crate::iommu::cfg_save(system_table);
            let lang2 = crate::i18n::detect_lang(system_table);
            let _ = system_table.stdout().write_str(crate::i18n::t(lang2, crate::i18n::key::IOMMU_CFG_SAVED));
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu cfg load") {
            crate::iommu::cfg_load(system_table);
            let lang2 = crate::i18n::detect_lang(system_table);
            let _ = system_table.stdout().write_str(crate::i18n::t(lang2, crate::i18n::key::IOMMU_CFG_LOADED));
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu verify") {
            vtd::verify_state(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu verify-map") {
            vtd::verify_mappings(system_table);
            continue;
        }
        if cmd.starts_with("iommu xlate ") {
            // iommu xlate bdf=<seg:bus:dev.func> iova=<hex>
            let args = &cmd[12..].trim();
            let mut seg: Option<u16> = None; let mut bus: Option<u8> = None; let mut dev: Option<u8> = None; let mut func: Option<u8> = None; let mut iova: Option<u64> = None;
            for tok in args.split_whitespace() {
                if let Some(v) = tok.strip_prefix("bdf=") {
                    let mut p = v.split(':');
                    if let (Some(s), Some(bd)) = (p.next(), p.next()) {
                        let mut df = bd.split('.');
                        if let (Some(d), Some(f)) = (df.next(), df.next()) {
                            seg = u16::from_str_radix(s, 16).ok();
                            bus = u8::from_str_radix(bd.split('.').next().unwrap_or("0"), 16).ok();
                            dev = u8::from_str_radix(d, 16).ok();
                            func = u8::from_str_radix(f, 16).ok();
                        }
                    }
                }
                if let Some(v) = tok.strip_prefix("iova=") { iova = u64::from_str_radix(v.trim_start_matches("0x"), 16).ok(); }
            }
            if let (Some(seg), Some(bus), Some(dev), Some(func), Some(iova)) = (seg,bus,dev,func,iova) {
                vtd::translate_bdf_iova(system_table, seg, bus, dev, func, iova);
                continue;
            }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("usage: iommu xlate bdf=<seg:bus:dev.func> iova=<hex>\r\n");
            continue;
        }
        if cmd.starts_with("iommu walk ") {
            // iommu walk bdf=<seg:bus:dev.func> iova=<hex>
            let args = &cmd[11..].trim();
            let mut seg: Option<u16> = None; let mut bus: Option<u8> = None; let mut dev: Option<u8> = None; let mut func: Option<u8> = None; let mut iova: Option<u64> = None;
            for tok in args.split_whitespace() {
                if let Some(v) = tok.strip_prefix("bdf=") {
                    let mut p = v.split(':');
                    if let (Some(s), Some(bd)) = (p.next(), p.next()) {
                        let mut df = bd.split('.');
                        if let (Some(d), Some(f)) = (df.next(), df.next()) {
                            seg = u16::from_str_radix(s, 16).ok();
                            bus = u8::from_str_radix(bd.split('.').next().unwrap_or("0"), 16).ok();
                            dev = u8::from_str_radix(d, 16).ok();
                            func = u8::from_str_radix(f, 16).ok();
                        }
                    }
                }
                if let Some(v) = tok.strip_prefix("iova=") { iova = u64::from_str_radix(v.trim_start_matches("0x"), 16).ok(); }
            }
            if let (Some(seg), Some(bus), Some(dev), Some(func), Some(iova)) = (seg,bus,dev,func,iova) {
                vtd::walk_bdf_iova(system_table, seg, bus, dev, func, iova);
                continue;
            }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("usage: iommu walk bdf=<seg:bus:dev.func> iova=<hex>\r\n");
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu apply") {
            vtd::apply_assignments(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu apply-refresh") {
            vtd::apply_and_refresh(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu apply-safe") {
            vtd::apply_safe(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu quick") {
            vtd::plan_assignments(system_table);
            vtd::apply_safe(system_table);
            vtd::verify_state(system_table);
            vtd::verify_mappings(system_table);
            vtd::invalidate_all(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu sync") {
            vtd::sync_contexts(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu invalidate") {
            vtd::invalidate_all(system_table);
            continue;
        }
        if cmd.starts_with("iommu invalidate dom=") {
            let v = &cmd[21..].trim();
            if let Ok(domid) = v.parse::<u16>() { vtd::invalidate_domain(system_table, domid); continue; }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("usage: iommu invalidate dom=<id>\r\n");
            continue;
        }
        if cmd.starts_with("iommu invalidate bdf=") {
            let v = &cmd[21..].trim();
            // hex: seg:bus:dev.func
            let mut parts = v.split(':');
            if let (Some(seg), Some(bus), Some(df)) = (parts.next(), parts.next(), parts.next()) {
                let mut dfs = df.split('.');
                if let (Some(dev), Some(func)) = (dfs.next(), dfs.next()) {
                    if let (Ok(seg), Ok(bus), Ok(dev), Ok(func)) = (
                        u16::from_str_radix(seg, 16),
                        u8::from_str_radix(bus, 16),
                        u8::from_str_radix(dev, 16),
                        u8::from_str_radix(func, 16),
                    ) {
                        vtd::invalidate_bdf(system_table, seg, bus, dev, func);
                        continue;
                    }
                }
            }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("usage: iommu invalidate bdf=<seg:bus:dev.func> (hex)\r\n");
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu hard-invalidate") {
            vtd::hard_invalidate_all(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu fsts") {
            vtd::report_faults(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("iommu fclear") {
            vtd::clear_faults(system_table);
            continue;
        }
        if cmd.starts_with("iommu root ") {
            let args = &cmd[11..].trim();
            if let Ok(bus) = u8::from_str_radix(args, 16) {
                vtd::dump_root(system_table, bus);
                continue;
            }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("usage: iommu root <bus> (hex)\r\n");
            continue;
        }
        if cmd.starts_with("iommu dump ") {
            let args = &cmd[11..].trim();
            // format: bus:dev.func in hex
            let mut parts = args.split(':');
            if let (Some(bus_str), Some(df_str)) = (parts.next(), parts.next()) {
                let mut df = df_str.split('.');
                if let (Ok(bus), Some(dev_str), Some(func_str)) = (u8::from_str_radix(bus_str, 16), df.next(), df.next()) {
                    if let (Ok(dev), Ok(func)) = (u8::from_str_radix(dev_str, 16), u8::from_str_radix(func_str, 16)) {
                        vtd::dump_context(system_table, bus, dev, func);
                        continue;
                    }
                }
            }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("usage: iommu dump <bus:dev.func> (hex)\r\n");
            continue;
        }
        if cmd.eq_ignore_ascii_case("trace") {
            crate::obs::trace::dump(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("migrate") {
            crate::migrate::dump_stats(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("migrate start") {
            let vm = crate::hv::vm::Vm::create(system_table, crate::hv::vm::VmConfig { memory_bytes: 256 << 20, vcpu_count: 1 });
            let _ = crate::hv::vm::register_vm(&vm);
            if crate::migrate::start_tracking(system_table, &vm) {
                let lang = crate::i18n::detect_lang(system_table);
                let _ = system_table.stdout().write_str(crate::i18n::t(lang, crate::i18n::key::MIG_TRACK_START_OK));
            } else {
                let lang = crate::i18n::detect_lang(system_table);
                let _ = system_table.stdout().write_str(crate::i18n::t(lang, crate::i18n::key::MIG_TRACK_START_FAIL));
            }
            continue;
        }
        if cmd.starts_with("migrate start id=") {
            let rest = &cmd[17..].trim();
            if let Ok(id) = rest.parse::<u64>() {
                if crate::migrate::start_tracking_by_id(system_table, id) {
                    let lang = crate::i18n::detect_lang(system_table);
                    let _ = system_table.stdout().write_str(crate::i18n::t(lang, crate::i18n::key::MIG_TRACK_START_OK));
                } else {
                    let lang = crate::i18n::detect_lang(system_table);
                    let _ = system_table.stdout().write_str(crate::i18n::t(lang, crate::i18n::key::MIG_TRACK_START_FAIL));
                }
                continue;
            }
            let _ = system_table.stdout().write_str("usage: migrate start id=<decimal>\r\n");
            continue;
        }
        if cmd.eq_ignore_ascii_case("migrate plan") {
            crate::migrate::plan_dirty_runs(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("migrate export-dirty") {
            let (_runs, _pages, bytes) = crate::migrate::export_dirty_runs(system_table, crate::migrate::ExportSink::Console);
            let mut buf = [0u8; 64]; let mut i = 0;
            for &b in b"migrate: export_bytes=" { buf[i] = b; i += 1; }
            i += crate::firmware::acpi::u32_to_dec(bytes as u32, &mut buf[i..]);
            buf[i] = b'\r'; i += 1; buf[i] = b'\n'; i += 1;
            let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..i]).unwrap_or("\r\n"));
            continue;
        }
        if cmd.starts_with("migrate scan") {
            let clear = cmd.trim_end().ends_with("clear");
            let n = crate::migrate::scan_round(clear);
            let stdout = system_table.stdout();
            let mut buf = [0u8; 64]; let mut i = 0;
            for &b in b"migrate: dirty_pages=" { buf[i] = b; i += 1; }
            i += crate::firmware::acpi::u32_to_dec(n as u32, &mut buf[i..]);
            buf[i] = b'\r'; i += 1; buf[i] = b'\n'; i += 1;
            let _ = stdout.write_str(core::str::from_utf8(&buf[..i]).unwrap_or("\r\n"));
            continue;
        }
        if cmd.starts_with("migrate export ") {
            // migrate export start=<hex> len=<hex> [sink=console|null]
            let rest = &cmd[15..].trim();
            let mut start: Option<u64> = None; let mut len: Option<u64> = None; let mut sink = crate::migrate::ExportSink::Console;
            for tok in rest.split_whitespace() {
                if let Some(v) = tok.strip_prefix("start=") { start = u64::from_str_radix(v.trim_start_matches("0x"), 16).ok(); continue; }
                if let Some(v) = tok.strip_prefix("len=") { len = u64::from_str_radix(v.trim_start_matches("0x"), 16).ok(); continue; }
                if let Some(v) = tok.strip_prefix("sink=") {
                    sink = if v.eq_ignore_ascii_case("null") { crate::migrate::ExportSink::Null }
                    else if v.eq_ignore_ascii_case("buffer") { crate::migrate::ExportSink::Buffer }
                    else if v.eq_ignore_ascii_case("snp") { crate::migrate::ExportSink::Snp }
                    else { crate::migrate::ExportSink::Console };
                    continue;
                }
            }
            if let (Some(s), Some(l)) = (start, len) {
                let bytes = crate::migrate::export_range(system_table, s, l, sink);
                let stdout = system_table.stdout();
                let mut buf = [0u8; 64]; let mut i = 0;
                for &b in b"migrate: export_bytes=" { buf[i] = b; i += 1; }
                i += crate::firmware::acpi::u32_to_dec(bytes as u32, &mut buf[i..]);
                buf[i] = b'\r'; i += 1; buf[i] = b'\n'; i += 1;
                let _ = stdout.write_str(core::str::from_utf8(&buf[..i]).unwrap_or("\r\n"));
                continue;
            }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("usage: migrate export start=<hex> len=<hex> [sink=console|null|buffer|snp]\r\n");
            continue;
        }
        if cmd.starts_with("migrate precopy") {
            // migrate precopy [rounds=<n>] [clear] [sink=console|null]
            let rest = &cmd[15..].trim();
            let mut rounds: u32 = 3; let mut clear = false; let mut sink = crate::migrate::get_default_sink();
            for tok in rest.split_whitespace() {
                if let Some(v) = tok.strip_prefix("rounds=") { if let Ok(n) = v.parse::<u32>() { rounds = n; } continue; }
                if tok.eq_ignore_ascii_case("clear") { clear = true; continue; }
                if let Some(v) = tok.strip_prefix("sink=") {
                    sink = if v.eq_ignore_ascii_case("console") { crate::migrate::ExportSink::Console }
                    else if v.eq_ignore_ascii_case("buffer") { crate::migrate::ExportSink::Buffer }
                    else if v.eq_ignore_ascii_case("snp") { crate::migrate::ExportSink::Snp }
                    else if v.eq_ignore_ascii_case("virtio") { crate::migrate::ExportSink::Virtio }
                    else { crate::migrate::ExportSink::Null };
                    continue;
                }
            }
            let (done, pages, bytes) = crate::migrate::precopy(system_table, rounds, clear, sink);
            let stdout = system_table.stdout();
            let mut buf = [0u8; 96]; let mut i = 0;
            for &b in b"migrate: precopy rounds=" { buf[i] = b; i += 1; }
            i += crate::firmware::acpi::u32_to_dec(done as u32, &mut buf[i..]);
            for &b in b" pages=" { buf[i] = b; i += 1; }
            i += crate::firmware::acpi::u32_to_dec(pages as u32, &mut buf[i..]);
            for &b in b" bytes=" { buf[i] = b; i += 1; }
            i += crate::firmware::acpi::u32_to_dec(bytes as u32, &mut buf[i..]);
            buf[i] = b'\r'; i += 1; buf[i] = b'\n'; i += 1;
            let _ = stdout.write_str(core::str::from_utf8(&buf[..i]).unwrap_or("\r\n"));
            continue;
        }
        if cmd.starts_with("migrate precopy-throttle") {
            // migrate precopy-throttle [rounds=<n>] [clear] [sink=console|null|buffer] rate=<kbps>
            let rest = &cmd[24..].trim();
            let mut rounds: u32 = 3; let mut clear = false; let mut sink = crate::migrate::get_default_sink(); let mut rate: u32 = 1024;
            for tok in rest.split_whitespace() {
                if let Some(v) = tok.strip_prefix("rounds=") { if let Ok(n) = v.parse::<u32>() { rounds = n; } continue; }
                if tok.eq_ignore_ascii_case("clear") { clear = true; continue; }
                if let Some(v) = tok.strip_prefix("sink=") {
                    sink = if v.eq_ignore_ascii_case("console") { crate::migrate::ExportSink::Console }
                    else if v.eq_ignore_ascii_case("buffer") { crate::migrate::ExportSink::Buffer }
                    else if v.eq_ignore_ascii_case("snp") { crate::migrate::ExportSink::Snp }
                    else if v.eq_ignore_ascii_case("virtio") { crate::migrate::ExportSink::Virtio }
                    else { crate::migrate::ExportSink::Null };
                    continue;
                }
                if let Some(v) = tok.strip_prefix("rate=") { let _ = v.parse::<u32>().map(|n| rate = n); continue; }
            }
            let (done, pages, bytes) = crate::migrate::precopy_throttled(system_table, rounds, clear, sink, rate);
            let stdout = system_table.stdout();
            let mut buf = [0u8; 96]; let mut i = 0;
            for &b in b"migrate: precopy rounds=" { buf[i] = b; i += 1; }
            i += crate::firmware::acpi::u32_to_dec(done as u32, &mut buf[i..]);
            for &b in b" pages=" { buf[i] = b; i += 1; }
            i += crate::firmware::acpi::u32_to_dec(pages as u32, &mut buf[i..]);
            for &b in b" bytes=" { buf[i] = b; i += 1; }
            i += crate::firmware::acpi::u32_to_dec(bytes as u32, &mut buf[i..]);
            buf[i] = b'\r'; i += 1; buf[i] = b'\n'; i += 1;
            let _ = stdout.write_str(core::str::from_utf8(&buf[..i]).unwrap_or("\r\n"));
            continue;
        }
        if cmd.eq_ignore_ascii_case("migrate stop") {
            if crate::migrate::stop_tracking(system_table) {
                let lang = crate::i18n::detect_lang(system_table);
                let _ = system_table.stdout().write_str(crate::i18n::t(lang, crate::i18n::key::MIG_TRACK_STOP_OK));
            } else {
                let lang = crate::i18n::detect_lang(system_table);
                let _ = system_table.stdout().write_str(crate::i18n::t(lang, crate::i18n::key::MIG_TRACK_STOP_FAIL));
            }
            continue;
        }
        if cmd.starts_with("migrate send-dirty") {
            // migrate send-dirty [compress] [sink=console|null]
            let rest = cmd.strip_prefix("migrate send-dirty").unwrap_or("").trim();
            let mut compress = false; let mut sink = crate::migrate::get_default_sink();
            for tok in rest.split_whitespace() {
                if tok.eq_ignore_ascii_case("compress") { compress = true; continue; }
                if let Some(v) = tok.strip_prefix("sink=") {
                    sink = if v.eq_ignore_ascii_case("console") { crate::migrate::ExportSink::Console }
                    else if v.eq_ignore_ascii_case("buffer") { crate::migrate::ExportSink::Buffer }
                    else if v.eq_ignore_ascii_case("snp") { crate::migrate::ExportSink::Snp }
                    else if v.eq_ignore_ascii_case("virtio") { crate::migrate::ExportSink::Virtio }
                    else { crate::migrate::ExportSink::Null };
                    continue;
                }
            }
            let (frames, pages, bytes) = crate::migrate::send_dirty_pages(system_table, compress, sink);
            let stdout = system_table.stdout();
            let mut buf = [0u8; 96]; let mut i = 0;
            for &b in b"migrate: sent frames=" { buf[i] = b; i += 1; }
            i += crate::firmware::acpi::u32_to_dec(frames as u32, &mut buf[i..]);
            for &b in b" pages=" { buf[i] = b; i += 1; }
            i += crate::firmware::acpi::u32_to_dec(pages as u32, &mut buf[i..]);
            for &b in b" bytes=" { buf[i] = b; i += 1; }
            i += crate::firmware::acpi::u32_to_dec(bytes as u32, &mut buf[i..]);
            buf[i] = b'\r'; i += 1; buf[i] = b'\n'; i += 1;
            let _ = stdout.write_str(core::str::from_utf8(&buf[..i]).unwrap_or("\r\n"));
            continue;
        }
        if cmd.starts_with("migrate chan ") {
            let rest = &cmd[13..].trim();
            if rest.starts_with("new") {
                let mut pages: usize = 64;
                for tok in rest[3..].trim().split_whitespace() {
                    if let Some(v) = tok.strip_prefix("pages=") { if let Ok(n) = v.parse::<usize>() { pages = n; } }
                }
                let ok = crate::migrate::chan_new(system_table, pages);
                let lang2 = crate::i18n::detect_lang(system_table);
                let _ = system_table.stdout().write_str(if ok { crate::i18n::t(lang2, crate::i18n::key::MIG_CHAN_NEW_OK) } else { crate::i18n::t(lang2, crate::i18n::key::MIG_CHAN_NEW_FAIL) });
                continue;
            }
            if rest.eq_ignore_ascii_case("clear") { crate::migrate::chan_clear(); let lang3 = crate::i18n::detect_lang(system_table); let _ = system_table.stdout().write_str(crate::i18n::t(lang3, crate::i18n::key::MIG_CHAN_CLEARED)); continue; }
            if rest.starts_with("dump") {
                let mut len: usize = 0; let mut hex = false;
                for tok in rest[4..].trim().split_whitespace() {
                    if let Some(v) = tok.strip_prefix("len=") { let _ = v.parse::<usize>().map(|n| len = n); continue; }
                    if tok.eq_ignore_ascii_case("hex") { hex = true; continue; }
                }
                crate::migrate::chan_dump(system_table, len, hex);
                continue;
            }
            if rest.starts_with("consume ") {
                let rest2 = &rest[8..].trim();
                if let Ok(n) = rest2.parse::<usize>() { crate::migrate::chan_consume(n); let _ = system_table.stdout().write_str("migrate: chan consumed\r\n"); continue; }
                let _ = system_table.stdout().write_str("usage: migrate chan consume <bytes>\r\n");
                continue;
            }
            if rest.starts_with("chunk ") {
                let rest2 = &rest[6..].trim();
                if rest2.eq_ignore_ascii_case("get") {
                    let sz = crate::migrate::get_chunk_size();
                    let mut buf = [0u8; 48]; let mut i = 0;
                    for &b in b"migrate: chunk=" { buf[i] = b; i += 1; }
                    i += crate::firmware::acpi::u32_to_dec(sz as u32, &mut buf[i..]);
                    buf[i] = b'\r'; i += 1; buf[i] = b'\n'; i += 1;
                    let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..i]).unwrap_or("\r\n"));
                    continue;
                }
                if let Some(v) = rest2.strip_prefix("set ") {
                    if let Ok(n) = v.trim().parse::<usize>() { crate::migrate::set_chunk_size(n); }
                    let _ = system_table.stdout().write_str("migrate: chunk updated\r\n");
                    continue;
                }
                let _ = system_table.stdout().write_str("usage: migrate chan chunk [get|set <bytes>]\r\n");
                continue;
            }
            let (len, cap) = crate::migrate::chan_stats();
            let mut buf = [0u8; 64]; let mut i = 0;
            for &b in b"migrate: chan len=" { buf[i] = b; i += 1; }
            i += crate::firmware::acpi::u32_to_dec(len as u32, &mut buf[i..]);
            for &b in b" cap=" { buf[i] = b; i += 1; }
            i += crate::firmware::acpi::u32_to_dec(cap as u32, &mut buf[i..]);
            buf[i] = b'\r'; i += 1; buf[i] = b'\n'; i += 1;
            let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..i]).unwrap_or("\r\n"));
            continue;
        }
        if cmd.starts_with("migrate verify") {
            // migrate verify [limit=<n>] [quiet]
            let rest = cmd.strip_prefix("migrate verify").unwrap_or("").trim();
            let mut limit: usize = 0; let mut quiet = false;
            for tok in rest.split_whitespace() {
                if let Some(v) = tok.strip_prefix("limit=") { let _ = v.parse::<usize>().map(|n| limit = n); continue; }
                if tok.eq_ignore_ascii_case("quiet") { quiet = true; continue; }
            }
            crate::migrate::chan_verify(system_table, limit, quiet);
            continue;
        }
        if cmd.starts_with("migrate replay") {
            // migrate replay [pages=<n>]
            let rest = cmd.strip_prefix("migrate replay").unwrap_or("").trim();
            let mut pages: usize = 0;
            for tok in rest.split_whitespace() {
                if let Some(v) = tok.strip_prefix("pages=") { let _ = v.parse::<usize>().map(|n| pages = n); continue; }
            }
            crate::migrate::replay_to_buffer(system_table, pages);
            continue;
        }
        if cmd.starts_with("migrate resend ") {
            // migrate resend from=<seq> [count=<n>] [compress] [sink=console|null|buffer]
            let rest = &cmd[15..].trim();
            let mut from: Option<u32> = None; let mut count: usize = 0; let mut compress = false; let mut sink = crate::migrate::ExportSink::Null;
            for tok in rest.split_whitespace() {
                if let Some(v) = tok.strip_prefix("from=") { from = v.parse::<u32>().ok(); continue; }
                if let Some(v) = tok.strip_prefix("count=") { let _ = v.parse::<usize>().map(|n| count = n); continue; }
                if tok.eq_ignore_ascii_case("compress") { compress = true; continue; }
                if let Some(v) = tok.strip_prefix("sink=") {
                    sink = if v.eq_ignore_ascii_case("console") { crate::migrate::ExportSink::Console }
                    else if v.eq_ignore_ascii_case("buffer") { crate::migrate::ExportSink::Buffer }
                    else if v.eq_ignore_ascii_case("snp") { crate::migrate::ExportSink::Snp }
                    else { crate::migrate::ExportSink::Null };
                    continue;
                }
            }
            if let Some(f) = from {
                let (frames, bytes) = crate::migrate::resend_from(system_table, f, count, compress, sink);
                let stdout = system_table.stdout();
                let mut buf = [0u8; 96]; let mut i = 0;
                for &b in b"migrate: resent frames=" { buf[i] = b; i += 1; }
                i += crate::firmware::acpi::u32_to_dec(frames as u32, &mut buf[i..]);
                for &b in b" bytes=" { buf[i] = b; i += 1; }
                i += crate::firmware::acpi::u32_to_dec(bytes as u32, &mut buf[i..]);
                buf[i] = b'\r'; i += 1; buf[i] = b'\n'; i += 1;
                let _ = stdout.write_str(core::str::from_utf8(&buf[..i]).unwrap_or("\r\n"));
                continue;
            }
            let _ = system_table.stdout().write_str("usage: migrate resend from=<seq> [count=<n>] [compress] [sink=console|null|buffer]\r\n");
            continue;
        }
        if cmd.starts_with("migrate ctrl ") {
            // migrate ctrl ack <seq> [sink=...] | migrate ctrl nak <seq> [sink=...]
            let rest = &cmd[13..].trim();
            let mut parts = rest.split_whitespace();
            if let (Some(kind), Some(seq_s)) = (parts.next(), parts.next()) {
                if let Ok(seq) = seq_s.parse::<u32>() {
                    let mut sink = crate::migrate::ExportSink::Buffer;
                    for tok in parts {
                        if let Some(v) = tok.strip_prefix("sink=") {
                            sink = if v.eq_ignore_ascii_case("console") { crate::migrate::ExportSink::Console }
                                   else if v.eq_ignore_ascii_case("null") { crate::migrate::ExportSink::Null }
                                   else if v.eq_ignore_ascii_case("snp") { crate::migrate::ExportSink::Snp }
                                   else { crate::migrate::ExportSink::Buffer };
                        }
                    }
                    crate::migrate::send_ctrl(system_table, kind.eq_ignore_ascii_case("ack"), seq, sink);
                    continue;
                }
            }
            let _ = system_table.stdout().write_str("usage: migrate ctrl [ack|nak] <seq> [sink=console|null|buffer]\r\n");
            continue;
        }
        if cmd.starts_with("migrate net ") {
            // migrate net mac [get|set xx:xx:xx:xx:xx:xx]
            // migrate net mtu [get|set <n>]
            // migrate net ether [get|set <hex>]
            let rest = &cmd[12..].trim();
            if rest.starts_with("mac ") {
                let sub = &rest[4..].trim();
                if sub.eq_ignore_ascii_case("get") {
                    let mac = crate::migrate::net_get_dest_mac();
                    let mut out = [0u8; 64]; let mut n = 0;
                    let lang2 = crate::i18n::detect_lang(system_table);
                    for &b in crate::i18n::t(lang2, crate::i18n::key::MIG_NET_MAC_PREFIX).as_bytes() { out[n] = b; n += 1; }
                    for i in 0..6 {
                        n += crate::util::format::u64_hex(mac[i] as u64, &mut out[n..]);
                        if i != 5 { out[n] = b':'; n += 1; }
                    }
                    out[n] = b'\r'; n += 1; out[n] = b'\n'; n += 1;
                    let _ = system_table.stdout().write_str(core::str::from_utf8(&out[..n]).unwrap_or("\r\n"));
                    continue;
                }
                if let Some(v) = sub.strip_prefix("set ") {
                    let mut mac = [0u8;6];
                    let mut ok = true; let mut idx = 0;
                    for part in v.split(':') {
                        if idx >= 6 { ok = false; break; }
                        if let Ok(byte) = u8::from_str_radix(part.trim_start_matches("0x"), 16) { mac[idx] = byte; idx += 1; } else { ok = false; break; }
                    }
                    if ok && idx == 6 { crate::migrate::net_set_dest_mac(mac); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_CFG_SET).inc(); let lang2 = crate::i18n::detect_lang(system_table); let _ = system_table.stdout().write_str(crate::i18n::t(lang2, crate::i18n::key::MIG_NET_MAC_UPDATED)); }
                    else { let lang2 = crate::i18n::detect_lang(system_table); let _ = system_table.stdout().write_str(crate::i18n::t(lang2, crate::i18n::key::MIG_NET_MAC_USAGE)); }
                    continue;
                }
                { let lang2 = crate::i18n::detect_lang(system_table); let _ = system_table.stdout().write_str(crate::i18n::t(lang2, crate::i18n::key::MIG_NET_MAC_USAGE)); }
                continue;
            }
            if rest.starts_with("mtu ") {
                let sub = &rest[4..].trim();
                if sub.eq_ignore_ascii_case("get") {
                    let mtu = crate::migrate::net_get_mtu();
                    let mut out = [0u8; 48]; let mut n = 0;
                    let lang2 = crate::i18n::detect_lang(system_table);
                    for &b in crate::i18n::t(lang2, crate::i18n::key::MIG_NET_MTU_PREFIX).as_bytes() { out[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(mtu as u32, &mut out[n..]);
                    out[n] = b'\r'; n += 1; out[n] = b'\n'; n += 1;
                    let _ = system_table.stdout().write_str(core::str::from_utf8(&out[..n]).unwrap_or("\r\n"));
                    continue;
                }
                if let Some(v) = sub.strip_prefix("set ") {
                    if let Ok(n) = v.trim().parse::<usize>() { crate::migrate::net_set_mtu(n); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_CFG_SET).inc(); let lang2 = crate::i18n::detect_lang(system_table); let _ = system_table.stdout().write_str(crate::i18n::t(lang2, crate::i18n::key::MIG_NET_MTU_UPDATED)); continue; }
                }
                { let lang2 = crate::i18n::detect_lang(system_table); let _ = system_table.stdout().write_str(crate::i18n::t(lang2, crate::i18n::key::MIG_NET_MTU_USAGE)); }
                continue;
            }
            if rest.starts_with("ether ") {
                let sub = &rest[6..].trim();
                if sub.eq_ignore_ascii_case("get") {
                    let et = crate::migrate::net_get_ethertype();
                    let mut out = [0u8; 48]; let mut n = 0; let lang2 = crate::i18n::detect_lang(system_table);
                    for &b in crate::i18n::t(lang2, crate::i18n::key::MIG_NET_ETHER_PREFIX).as_bytes() { out[n] = b; n += 1; }
                    n += crate::util::format::u64_hex(et as u64, &mut out[n..]);
                    out[n] = b'\r'; n += 1; out[n] = b'\n'; n += 1;
                    let _ = system_table.stdout().write_str(core::str::from_utf8(&out[..n]).unwrap_or("\r\n"));
                    continue;
                }
                if let Some(v) = sub.strip_prefix("set ") {
                    if let Ok(n) = u16::from_str_radix(v.trim_start_matches("0x"), 16) { crate::migrate::net_set_ethertype(n); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_CFG_SET).inc(); let lang2 = crate::i18n::detect_lang(system_table); let _ = system_table.stdout().write_str(crate::i18n::t(lang2, crate::i18n::key::MIG_NET_ETHER_UPDATED)); continue; }
                }
                { let lang2 = crate::i18n::detect_lang(system_table); let _ = system_table.stdout().write_str(crate::i18n::t(lang2, crate::i18n::key::MIG_NET_ETHER_USAGE)); }
                continue;
            }
            { let lang2 = crate::i18n::detect_lang(system_table); let _ = system_table.stdout().write_str(crate::i18n::t(lang2, crate::i18n::key::MIG_NET_USAGE)); }
            continue;
        }
        if cmd.starts_with("migrate handle-ctrl") {
            // migrate handle-ctrl [limit=<n>]
            let rest = cmd.strip_prefix("migrate handle-ctrl").unwrap_or("").trim();
            let mut limit: usize = 0;
            for tok in rest.split_whitespace() { if let Some(v) = tok.strip_prefix("limit=") { let _ = v.parse::<usize>().map(|n| limit = n); } }
            crate::migrate::chan_handle_ctrl(system_table, limit);
            continue;
        }
        if cmd.starts_with("migrate virtio poll") {
            // migrate virtio poll [cycles=<n>] [sleep=<us>] [ctrl] [verify] [empty=<n>]
            let rest = cmd.strip_prefix("migrate virtio poll").unwrap_or("").trim();
            let mut cycles: usize = 0; // 0=infinite
            let mut sleep_us: usize = 0;
            let mut do_ctrl = false; let mut do_verify = false; let mut empty_limit: usize = 0;
            for tok in rest.split_whitespace() {
                if let Some(v) = tok.strip_prefix("cycles=") { let _ = v.parse::<usize>().map(|n| cycles = n); continue; }
                if let Some(v) = tok.strip_prefix("sleep=") { let _ = v.parse::<usize>().map(|n| sleep_us = n); continue; }
                if let Some(v) = tok.strip_prefix("empty=") { let _ = v.parse::<usize>().map(|n| empty_limit = n); continue; }
                if tok.eq_ignore_ascii_case("ctrl") { do_ctrl = true; continue; }
                if tok.eq_ignore_ascii_case("verify") { do_verify = true; continue; }
            }
            crate::migrate::virtio_poll_ex(system_table, cycles, sleep_us, do_ctrl, do_verify, empty_limit);
            continue;
        }
        if cmd.starts_with("migrate snp poll") {
            // migrate snp poll [cycles=<n>] [sleep=<us>] [ctrl] [verify] [empty=<n>]
            let rest = cmd.strip_prefix("migrate snp poll").unwrap_or("").trim();
            let mut cycles: usize = 0; // 0=infinite
            let mut sleep_us: usize = 0;
            let mut do_ctrl = false; let mut do_verify = false; let mut empty_limit: usize = 0;
            for tok in rest.split_whitespace() {
                if let Some(v) = tok.strip_prefix("cycles=") { let _ = v.parse::<usize>().map(|n| cycles = n); continue; }
                if let Some(v) = tok.strip_prefix("sleep=") { let _ = v.parse::<usize>().map(|n| sleep_us = n); continue; }
                if let Some(v) = tok.strip_prefix("empty=") { let _ = v.parse::<usize>().map(|n| empty_limit = n); continue; }
                if tok.eq_ignore_ascii_case("ctrl") { do_ctrl = true; continue; }
                if tok.eq_ignore_ascii_case("verify") { do_verify = true; continue; }
            }
            crate::migrate::snp_poll_ex(system_table, cycles, sleep_us, do_ctrl, do_verify, empty_limit);
            continue;
        }
        if cmd.eq_ignore_ascii_case("trace clear") {
            crate::obs::trace::clear();
            let stdout = system_table.stdout();
            let _ = stdout.write_str("trace: cleared\r\n");
            continue;
        }
        if cmd.eq_ignore_ascii_case("metrics") {
            crate::obs::metrics::dump(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("metrics clear") {
            crate::obs::metrics::reset();
            let stdout = system_table.stdout();
            let _ = stdout.write_str("metrics: cleared\r\n");
            continue;
        }
        if cmd.eq_ignore_ascii_case("logs") {
            crate::obs::log::dump(system_table);
            continue;
        }
        if cmd.starts_with("logs filter ") {
            let rest = &cmd[12..].trim();
            let mut lvl: u8 = 0; let mut cat: &str = "";
            for tok in rest.split_whitespace() {
                if let Some(v) = tok.strip_prefix("level=") {
                    if v.eq_ignore_ascii_case("warn") { lvl = 1; }
                    else if v.eq_ignore_ascii_case("error") { lvl = 2; }
                    else { lvl = 0; }
                    continue;
                }
                if let Some(v) = tok.strip_prefix("cat=") { cat = v; continue; }
            }
            crate::obs::log::dump_filtered(system_table, lvl, cat);
            continue;
        }
        if cmd.starts_with("loglevel ") {
            let rest = &cmd[9..].trim();
            if rest.eq_ignore_ascii_case("info") { crate::obs::log::set_min_level_info(); }
            else if rest.eq_ignore_ascii_case("warn") { crate::obs::log::set_min_level_warn(); }
            else if rest.eq_ignore_ascii_case("error") { crate::obs::log::set_min_level_error(); }
            else { let stdout = system_table.stdout(); let _ = stdout.write_str("usage: loglevel [info|warn|error]\r\n"); continue; }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("loglevel: updated\r\n");
            continue;
        }
        if cmd.starts_with("dump ") {
            let rest = &cmd[5..].trim();
            if rest.eq_ignore_ascii_case("regs") { crate::diag::dump::dump_regs(system_table); continue; }
            if rest.eq_ignore_ascii_case("idt") { crate::diag::dump::dump_idt(system_table); continue; }
            if rest.eq_ignore_ascii_case("gdt") { crate::diag::dump::dump_gdt(system_table); continue; }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("usage: dump [regs|idt|gdt]\r\n");
            continue;
        }
		if cmd.starts_with("lang ") {
			let rest = &cmd[5..].trim();
			if rest.eq_ignore_ascii_case("en") { i18n::set_lang_override(Some(Lang::En)); }
			else if rest.eq_ignore_ascii_case("ja") { i18n::set_lang_override(Some(Lang::Ja)); }
			else if rest.eq_ignore_ascii_case("zh") { i18n::set_lang_override(Some(Lang::Zh)); }
			else { i18n::set_lang_override(None); }
            // Persist override to UEFI variable for next boot
            i18n::save_lang_override(system_table);
            let stdout = system_table.stdout();
            let _ = stdout.write_str("lang: updated (persisted)\r\n");
			continue;
		}
        if cmd.eq_ignore_ascii_case("sec") {
            crate::diag::security::report_security(system_table);
            continue;
        }
        if cmd.eq_ignore_ascii_case("audit") {
            crate::diag::audit::dump(system_table);
            continue;
        }
        if cmd.starts_with("wdog") {
            let rest = cmd.strip_prefix("wdog").unwrap_or("").trim();
            if rest.is_empty() {
                crate::diag::watchdog::report(system_table);
                continue;
            }
            if rest.eq_ignore_ascii_case("off") {
                let ok = crate::diag::watchdog::disarm(system_table);
                {
                    let stdout = system_table.stdout();
                    let _ = stdout.write_str(if ok { "watchdog disarmed\r\n" } else { "watchdog disarm failed\r\n" });
                }
                continue;
            }
            if let Ok(secs) = rest.parse::<usize>() {
                let ok = crate::diag::watchdog::arm(system_table, secs);
                {
                    let stdout = system_table.stdout();
                    let _ = stdout.write_str(if ok { "watchdog armed\r\n" } else { "watchdog arm failed\r\n" });
                }
                continue;
            }
            {
                let stdout = system_table.stdout();
                let _ = stdout.write_str("usage: wdog [off|<seconds>]\r\n");
            }
            continue;
        }
        if cmd.eq_ignore_ascii_case("pci") {
            crate::iommu::report_pci_endpoints(system_table);
            continue;
        }
        if cmd.starts_with("pci class ") {
            let rest = &cmd[10..].trim();
            let mut parts = rest.split_whitespace();
            let parse_num = |s: &str| -> Option<u32> { if let Some(h) = s.strip_prefix("0x") { u32::from_str_radix(h, 16).ok() } else { s.parse::<u32>().ok() } };
            if let (Some(ccs), Some(scs)) = (parts.next(), parts.next()) {
                if let (Some(cc), Some(sc)) = (parse_num(ccs), parse_num(scs)) {
                    // Simple acknowledgment line
                    let stdout = system_table.stdout();
                    let mut buf = [0u8; 64]; let mut n = 0;
                    for &b in b"filter: class=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(cc, &mut buf[n..]); buf[n] = b'/'; n += 1;
                    n += crate::firmware::acpi::u32_to_dec(sc, &mut buf[n..]); buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                    // Full filtered enumeration
                    crate::iommu::report_pci_by_class(system_table, cc as u8, sc as u8);
                    continue;
                }
            }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("usage: pci class <class> <subclass>\r\n");
            continue;
        }
        if cmd.starts_with("pci find ") {
            let rest = &cmd[9..].trim();
            let mut vid: Option<u16> = None; let mut did: Option<u16> = None;
            for tok in rest.split_whitespace() {
                if let Some(v) = tok.strip_prefix("vid=") { vid = u16::from_str_radix(v.trim_start_matches("0x"), 16).ok(); continue; }
                if let Some(v) = tok.strip_prefix("did=") { did = u16::from_str_radix(v.trim_start_matches("0x"), 16).ok(); continue; }
            }
            if let Some(mcfg_hdr) = crate::firmware::acpi::find_mcfg(system_table) {
                crate::firmware::acpi::mcfg_for_each_allocation_from(|a| {
                    let mut bus = a.start_bus;
                    while bus <= a.end_bus {
                        for dev in 0u8..32u8 { for func in 0u8..8u8 {
                            let cfg = crate::iommu::ecam_fn_base(a.base_address, a.start_bus, bus, dev, func);
                            let v = crate::iommu::mmio_read16(cfg + 0x00);
                            if v == 0xFFFF { continue; }
                            let d = crate::iommu::mmio_read16(cfg + 0x02);
                            if let Some(w) = vid { if v != w { continue; } }
                            if let Some(w) = did { if d != w { continue; } }
                            let stdout = system_table.stdout();
                            let mut buf = [0u8; 96]; let mut n = 0;
                            for &b in b"PCI: seg=" { buf[n] = b; n += 1; }
                            n += crate::firmware::acpi::u32_to_dec(a.pci_segment as u32, &mut buf[n..]);
                            for &b in b" b=" { buf[n] = b; n += 1; }
                            n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
                            for &b in b" d=" { buf[n] = b; n += 1; }
                            n += crate::firmware::acpi::u32_to_dec(dev as u32, &mut buf[n..]);
                            for &b in b" f=" { buf[n] = b; n += 1; }
                            n += crate::firmware::acpi::u32_to_dec(func as u32, &mut buf[n..]);
                            for &b in b" vid=0x" { buf[n] = b; n += 1; }
                            n += crate::util::format::u64_hex(v as u64, &mut buf[n..]);
                            for &b in b" did=0x" { buf[n] = b; n += 1; }
                            n += crate::util::format::u64_hex(d as u64, &mut buf[n..]);
                            buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                            let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                        } }
                        if bus == 0xFF { break; }
                        bus = bus.saturating_add(1);
                    }
                }, mcfg_hdr);
            }
            continue;
        }
        if cmd.eq_ignore_ascii_case("time") || cmd.eq_ignore_ascii_case("time show") {
            let hz = crate::time::tsc_hz();
            let stdout = system_table.stdout();
            let mut buf = [0u8; 64]; let mut n = 0;
            for &b in b"time: tsc_hz=" { buf[n] = b; n += 1; }
            n += crate::firmware::acpi::u32_to_dec((hz / 1_000_000) as u32, &mut buf[n..]);
            for &b in b" MHz\r\n" { buf[n] = b; n += 1; }
            let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
            continue;
        }
        if cmd.starts_with("time wait ") {
            // time wait <usec> [busy|stall]
            let rest = &cmd[10..].trim();
            let mut parts = rest.split_whitespace();
            if let Some(us_s) = parts.next() {
                if let Ok(usec) = us_s.parse::<u64>() {
                    let mode = parts.next().unwrap_or("busy");
                    if mode.eq_ignore_ascii_case("stall") {
                        let _ = system_table.boot_services().stall(usec as usize);
                    } else {
                        let hz = crate::time::tsc_hz();
                        crate::time::busy_wait_tsc(system_table, usec, hz);
                    }
                    let stdout = system_table.stdout();
                    let _ = stdout.write_str("time: wait done\r\n");
                    continue;
                }
            }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("usage: time wait <usec> [busy|stall]\r\n");
            continue;
        }
        if cmd.eq_ignore_ascii_case("vm") {
            // Create a tiny VM object and print its id, try start (VMX smoke paths)
            let vm = crate::hv::vm::Vm::create(system_table, crate::hv::vm::VmConfig { memory_bytes: 64 << 20, vcpu_count: 1 });
            let _ = crate::hv::vm::register_vm(&vm);
            let mut vcpu = crate::hv::vcpu::Vcpu::new(0);
            vcpu.start();
            vm.start(system_table);
            let stdout = system_table.stdout();
            let mut out = [0u8; 96]; let mut n = 0;
            for &b in b"VM created id=" { out[n] = b; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(vm.id.0 as u32, &mut out[n..]);
            for &b in b" vcpu0=" { out[n] = b; n += 1; }
            let s = match vcpu.state { crate::hv::vcpu::VcpuState::Created => b"created", crate::hv::vcpu::VcpuState::Running => b"running", crate::hv::vcpu::VcpuState::Stopped => b"stopped" };
            for &b in s { out[n] = b; n += 1; }
            out[n] = b'\r'; n += 1; out[n] = b'\n'; n += 1;
            let _ = stdout.write_str(core::str::from_utf8(&out[..n]).unwrap_or("\r\n"));
            vm.stop();
            vm.destroy();
            continue;
        }
        if cmd.eq_ignore_ascii_case("vm list") {
            let stdout = system_table.stdout();
            crate::hv::vm::list_vms(|info| {
                let mut out = [0u8; 128]; let mut n = 0;
                for &b in b"vm: id=" { out[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(info.id as u32, &mut out[n..]);
                for &b in b" vendor=" { out[n] = b; n += 1; }
                let v: &[u8] = match info.vendor { crate::hv::vm::HvVendor::Intel => b"intel", crate::hv::vm::HvVendor::Amd => b"amd", crate::hv::vm::HvVendor::Unknown => b"unknown" };
                for &b in v { out[n] = b; n += 1; }
                for &b in b" pml4=0x" { out[n] = b; n += 1; }
                n += crate::util::format::u64_hex(info.pml4_phys, &mut out[n..]);
                for &b in b" mem=0x" { out[n] = b; n += 1; }
                n += crate::util::format::u64_hex(info.memory_bytes, &mut out[n..]);
                out[n] = b'\r'; n += 1; out[n] = b'\n'; n += 1;
                let _ = stdout.write_str(core::str::from_utf8(&out[..n]).unwrap_or("\r\n"));
            });
            continue;
        }
        if cmd.eq_ignore_ascii_case("vm pause") {
            let vm = crate::hv::vm::Vm::create(system_table, crate::hv::vm::VmConfig { memory_bytes: 64 << 20, vcpu_count: 1 });
            vm.pause();
            let _ = system_table.stdout().write_str("vm paused (trace event)\r\n");
            continue;
        }
        if cmd.eq_ignore_ascii_case("vm resume") {
            let vm = crate::hv::vm::Vm::create(system_table, crate::hv::vm::VmConfig { memory_bytes: 64 << 20, vcpu_count: 1 });
            vm.resume();
            let _ = system_table.stdout().write_str("vm resumed (trace event)\r\n");
            continue;
        }
        if cmd.starts_with("vm ") {
            let rest = &cmd[3..];
            if rest.eq_ignore_ascii_case("new") {
                let vm = crate::hv::vm::Vm::create(system_table, crate::hv::vm::VmConfig { memory_bytes: 256 << 20, vcpu_count: 1 });
            let _ = crate::hv::vm::register_vm(&vm);
                let stdout = system_table.stdout();
                let mut out = [0u8; 64]; let mut n = 0;
                for &b in b"vm id=" { out[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(vm.id.0 as u32, &mut out[n..]);
                out[n] = b'\r'; n += 1; out[n] = b'\n'; n += 1;
                let _ = stdout.write_str(core::str::from_utf8(&out[..n]).unwrap_or("\r\n"));
                continue;
            }
            if rest.eq_ignore_ascii_case("start") {
                let vm = crate::hv::vm::Vm::create(system_table, crate::hv::vm::VmConfig { memory_bytes: 256 << 20, vcpu_count: 1 });
            let _ = crate::hv::vm::register_vm(&vm);
                let mut vcpu = crate::hv::vcpu::Vcpu::new(0);
                vcpu.start();
                vm.start(system_table);
                let stdout = system_table.stdout();
                let _ = stdout.write_str("vm started\r\n");
                continue;
            }
            let stdout = system_table.stdout();
            let _ = stdout.write_str("usage: vm | vm new | vm start\r\n");
            continue;
        }
        // Unknown
        let stdout = system_table.stdout();
        let _ = stdout.write_str("Unknown command\r\n");
    }
}


