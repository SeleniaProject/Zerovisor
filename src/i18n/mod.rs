//! Minimal i18n message resolver.
//!
//! This module provides a tiny message table for English and Japanese with a
//! stable set of keys used by the bootstrap. It avoids allocations and keeps
//! string lifetimes static for UEFI text output.

/// Supported languages.
#[derive(Clone, Copy, Debug)]
pub enum Lang {
    En,
    Ja,
}

/// Select the language heuristically. In UEFI we do not query locale yet; we
/// default to English to maximize compatibility.
#[inline(always)]
pub fn detect_lang() -> Lang {
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
            _ => "\r\n",
        },
    }
}


