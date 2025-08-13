#![allow(dead_code)]

//! AMD-Vi (IOMMU) minimal discovery stubs.

use uefi::prelude::Boot;
use uefi::table::SystemTable;
use core::fmt::Write as _;

/// Probe for ACPI IVRS table and print a short summary.
pub fn probe_and_report(system_table: &mut SystemTable<Boot>) {
    let lang = crate::i18n::detect_lang(system_table);
    // Resolve header before borrowing stdout to avoid aliasing borrows
    let ivrs = crate::firmware::acpi::find_ivrs(system_table);
    let stdout = system_table.stdout();
    if let Some(hdr) = ivrs {
        crate::firmware::acpi::ivrs_summary(|s| { let _ = stdout.write_str(s); }, hdr);
        crate::firmware::acpi::ivrs_list_entries_from(|s| { let _ = stdout.write_str(s); }, hdr);
    } else {
        let _ = stdout.write_str(crate::i18n::t(lang, crate::i18n::key::IOMMU_AMDV_NONE));
    }
}


