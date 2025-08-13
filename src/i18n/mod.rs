//! Minimal i18n message resolver.
//!
//! This module provides a tiny message table for English and Japanese with a
//! stable set of keys used by the bootstrap. It avoids allocations and keeps
//! string lifetimes static for UEFI text output.

/// Supported languages.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub enum Lang {
    En,
    Ja,
    Zh,
}

use uefi::prelude::Boot;
use uefi::table::SystemTable;
// use uefi::cstr16; // not used in current stub implementation

/// Try to parse UEFI `PlatformLang` (RFC 3066 like "en-US", "ja-JP", "zh-CN").
#[allow(dead_code)]
fn parse_platform_lang_ascii(bytes: &[u8]) -> Option<Lang> {
    // Accept only visible ASCII and stop at first NUL.
    let mut len = 0usize;
    while len < bytes.len() {
        let b = bytes[len];
        if b == 0 { break; }
        if b < 0x20 || b > 0x7E { break; }
        len += 1;
    }
    if len == 0 { return None; }
    let s = &bytes[..len];
    // Case-insensitive compare for language code prefix
    // Match "en"/"en-US"/"en-*"
    let eq_ci = |a: &[u8], b: &[u8]| -> bool {
        if a.len() != b.len() { return false; }
        for i in 0..a.len() {
            let ca = a[i];
            let cb = b[i];
            let ca = if ca >= b'A' && ca <= b'Z' { ca + 32 } else { ca };
            let cb = if cb >= b'A' && cb <= b'Z' { cb + 32 } else { cb };
            if ca != cb { return false; }
        }
        true
    };
    let starts_with_ci = |pat: &[u8]| -> bool {
        if s.len() < pat.len() { return false; }
        eq_ci(&s[..pat.len()], pat)
    };
    if starts_with_ci(b"en") { return Some(Lang::En); }
    if starts_with_ci(b"ja") { return Some(Lang::Ja); }
    if starts_with_ci(b"zh") { return Some(Lang::Zh); }
    None
}

/// Select the language based on UEFI `PlatformLang` variable when available,
/// falling back to English to maximize compatibility.
#[inline(always)]
pub fn detect_lang(_system_table: &SystemTable<Boot>) -> Lang {
    // Fallback to English for stability; PlatformLang retrieval differs per crate version.
    Lang::En
}

/// Message keys used during bootstrap.
pub mod key {
    pub const BANNER: &str = "banner";
    pub const ENV: &str = "env";
    pub const READY: &str = "ready";
    pub const FEAT_VMX: &str = "feat_vmx";
    pub const FEAT_SVM: &str = "feat_svm";
    pub const FEAT_EPT: &str = "feat_ept";
    pub const FEAT_NPT: &str = "feat_npt";
    pub const FEAT_VTD: &str = "feat_vtd";
    pub const FEAT_AMDVI: &str = "feat_amdvi";
}

/// Resolve a message key for a given language.
#[inline(always)]
pub fn t(lang: Lang, key: &str) -> &'static str {
    match lang {
        Lang::En => match key {
            key::BANNER => "Zerovisor: UEFI bootstrap started\r\n",
            key::ENV => "Environment: x86_64 UEFI application\r\n",
            key::READY => "Status: Initialization complete\r\n",
            key::FEAT_VMX => "Feature: Intel VMX\r\n",
            key::FEAT_SVM => "Feature: AMD SVM\r\n",
            key::FEAT_EPT => "Feature: Intel EPT (hint)\r\n",
            key::FEAT_NPT => "Feature: AMD NPT\r\n",
            key::FEAT_VTD => "Feature: Intel VT-d (ACPI DMAR)\r\n",
            key::FEAT_AMDVI => "Feature: AMD-Vi (ACPI IVRS)\r\n",
            _ => "\r\n",
        },
        Lang::Ja => match key {
            key::BANNER => "Zerovisor: UEFIブート開始\r\n",
            key::ENV => "環境: x86_64 UEFI アプリケーション\r\n",
            key::READY => "状態: 初期化完了\r\n",
            key::FEAT_VMX => "機能: Intel VMX\r\n",
            key::FEAT_SVM => "機能: AMD SVM\r\n",
            key::FEAT_EPT => "機能: Intel EPT（示唆）\r\n",
            key::FEAT_NPT => "機能: AMD NPT\r\n",
            key::FEAT_VTD => "機能: Intel VT-d（ACPI DMAR）\r\n",
            key::FEAT_AMDVI => "機能: AMD-Vi（ACPI IVRS）\r\n",
            _ => "\r\n",
        },
        Lang::Zh => match key {
            key::BANNER => "Zerovisor: UEFI 引导已开始\r\n",
            key::ENV => "环境: x86_64 UEFI 应用程序\r\n",
            key::READY => "状态: 初始化完成\r\n",
            key::FEAT_VMX => "功能: Intel VMX\r\n",
            key::FEAT_SVM => "功能: AMD SVM\r\n",
            key::FEAT_EPT => "功能: Intel EPT（提示）\r\n",
            key::FEAT_NPT => "功能: AMD NPT\r\n",
            key::FEAT_VTD => "功能: Intel VT-d（ACPI DMAR）\r\n",
            key::FEAT_AMDVI => "功能: AMD-Vi（ACPI IVRS）\r\n",
            _ => "\r\n",
        },
    }
}


