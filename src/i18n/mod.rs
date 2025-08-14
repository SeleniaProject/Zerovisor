//! Minimal i18n message resolver.
//!
//! This module provides a tiny message table for English/Japanese/Chinese with a
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
use uefi::table::runtime::VariableVendor;
use uefi::{cstr16};
use core::sync::atomic::{AtomicU8, Ordering};

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

// Optional runtime override (0: auto, 1: en, 2: ja, 3: zh)
static OVERRIDE_LANG: AtomicU8 = AtomicU8::new(0);

#[inline(always)]
pub fn set_lang_override(l: Option<Lang>) {
    let v = match l { None => 0u8, Some(Lang::En) => 1, Some(Lang::Ja) => 2, Some(Lang::Zh) => 3 };
    OVERRIDE_LANG.store(v, Ordering::Relaxed);
}

#[inline(always)]
fn read_lang_override() -> Option<Lang> {
    match OVERRIDE_LANG.load(Ordering::Relaxed) {
        1 => Some(Lang::En),
        2 => Some(Lang::Ja),
        3 => Some(Lang::Zh),
        _ => None,
    }
}

/// Select the language based on UEFI `PlatformLang` variable when available,
/// falling back to English to maximize compatibility.
#[inline(always)]
pub fn detect_lang(system_table: &SystemTable<Boot>) -> Lang {
    // If runtime override set, honor it
    if let Some(ov) = read_lang_override() { return ov; }
    // If persisted override exists in UEFI variable, use it
    if let Some(persist) = read_persisted_override(system_table) { return persist; }
    // Read UEFI global variable: PlatformLang (CHAR8 RFC 3066 like "en-US")
    // Name is a CHAR16 string, vendor GUID is EFI_GLOBAL_VARIABLE:
    // 8BE4DF61-93CA-11D2-AA0D-00E098032B8C
    let rs = system_table.runtime_services();
    let name = cstr16!("PlatformLang");
    let vendor = VariableVendor::GLOBAL_VARIABLE;

    // Use a fixed buffer to avoid dynamic allocation in no_std.
    // The value is a short ASCII token (e.g., "en-US").
    let mut buf = [0u8; 128];
    if let Ok((data, _attrs)) = rs.get_variable(name, &vendor, &mut buf) {
        if let Some(l) = parse_platform_lang_ascii(data) { return l; }
    }

    // Final fallback to English.
    Lang::En
}

fn read_persisted_override(system_table: &SystemTable<Boot>) -> Option<Lang> {
    let rs = system_table.runtime_services();
    let name = cstr16!("ZerovisorLang");
    let vendor = VariableVendor::GLOBAL_VARIABLE;
    let mut buf = [0u8; 16];
    if let Ok((data, _attrs)) = rs.get_variable(name, &vendor, &mut buf) {
        if let Some(l) = parse_platform_lang_ascii(data) { return Some(l); }
        // accept "auto" to clear override
        let s = core::str::from_utf8(data).unwrap_or("");
        if s.starts_with("auto") { return None; }
    }
    None
}

pub fn save_lang_override(system_table: &SystemTable<Boot>) {
    let v = OVERRIDE_LANG.load(Ordering::Relaxed);
    let rs = system_table.runtime_services();
    let name = cstr16!("ZerovisorLang");
    let vendor = VariableVendor::GLOBAL_VARIABLE;
    let bytes: &[u8] = match v {
        1 => b"en\0",
        2 => b"ja\0",
        3 => b"zh\0",
        _ => b"auto\0",
    };
    let _ = rs.set_variable(name, &vendor, uefi::table::runtime::VariableAttributes::BOOTSERVICE_ACCESS, bytes);
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
    pub const HPET_PRESENT: &str = "hpet_present";
    pub const HPET_NOT_FOUND: &str = "hpet_not_found";
    pub const SMP_EXPECTED: &str = "smp_expected";
    pub const SMP_OBSERVED: &str = "smp_observed";
    pub const SMP_PM_OK: &str = "smp_pm_ok";
    pub const SMP_PM_NG: &str = "smp_pm_ng";
    pub const SMP_LM_OK: &str = "smp_lm_ok";
    pub const SMP_LM_NG: &str = "smp_lm_ng";
    pub const SMP_LM_COUNT: &str = "smp_lm_count";
    pub const SMP_APIC_BYTE: &str = "smp_apic_byte";
    pub const SMP_AP_IDS: &str = "smp_ap_ids";
    pub const SMP_READY: &str = "smp_ready";
    pub const VIRTIO_SCAN: &str = "virtio_scan";
    pub const VIRTIO_NONE: &str = "virtio_none";
    pub const IOMMU_VTD_NONE: &str = "iommu_vtd_none";
    pub const IOMMU_AMDV_NONE: &str = "iommu_amdv_none";
    pub const VIRTIO_BLK: &str = "virtio_blk";
    pub const VIRTIO_BLK_NONE: &str = "virtio_blk_none";
    pub const VIRTIO_NET: &str = "virtio_net";
    pub const VIRTIO_NET_NONE: &str = "virtio_net_none";
    pub const SEC_WP_ON: &str = "sec_wp_on";
    pub const SEC_WP_OFF: &str = "sec_wp_off";
    pub const SEC_SMEP_ON: &str = "sec_smep_on";
    pub const SEC_SMEP_OFF: &str = "sec_smep_off";
    pub const SEC_SMAP_ON: &str = "sec_smap_on";
    pub const SEC_SMAP_OFF: &str = "sec_smap_off";
    pub const SEC_NXE_ON: &str = "sec_nxe_on";
    pub const SEC_NXE_OFF: &str = "sec_nxe_off";
    pub const SEC_SUMMARY_OK: &str = "sec_summary_ok";
    pub const SEC_SUMMARY_NG: &str = "sec_summary_ng";
    pub const MIG_TRACK_START_OK: &str = "migrate_track_start_ok";
    pub const MIG_TRACK_START_FAIL: &str = "migrate_track_start_fail";
    pub const MIG_TRACK_STOP_OK: &str = "migrate_track_stop_ok";
    pub const MIG_TRACK_STOP_FAIL: &str = "migrate_track_stop_fail";
    pub const MIG_CHAN_NEW_OK: &str = "migrate_chan_new_ok";
    pub const MIG_CHAN_NEW_FAIL: &str = "migrate_chan_new_fail";
    pub const MIG_CHAN_CLEARED: &str = "migrate_chan_cleared";
    pub const MIG_NO_BUFFER: &str = "migrate_no_buffer";
    pub const MIG_NET_MAC_PREFIX: &str = "migrate_net_mac_prefix";
    pub const MIG_NET_MTU_PREFIX: &str = "migrate_net_mtu_prefix";
    pub const MIG_NET_MAC_UPDATED: &str = "migrate_net_mac_updated";
    pub const MIG_NET_MTU_UPDATED: &str = "migrate_net_mtu_updated";
    pub const MIG_NET_USAGE: &str = "migrate_net_usage";
    pub const MIG_NET_MAC_USAGE: &str = "migrate_net_mac_usage";
    pub const MIG_NET_MTU_USAGE: &str = "migrate_net_mtu_usage";
    pub const MIG_NET_ETHER_PREFIX: &str = "migrate_net_ether_prefix";
    pub const MIG_NET_ETHER_UPDATED: &str = "migrate_net_ether_updated";
    pub const MIG_NET_ETHER_USAGE: &str = "migrate_net_ether_usage";
    pub const IOMMU_CFG_SAVED: &str = "iommu_cfg_saved";
    pub const IOMMU_CFG_LOADED: &str = "iommu_cfg_loaded";
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
            key::HPET_PRESENT => "HPET: present, base=0x",
            key::HPET_NOT_FOUND => "HPET: not found\r\n",
            key::SMP_EXPECTED => "SMP: expected CPUs=",
            key::SMP_OBSERVED => "SMP: observed AP IDs=",
            key::SMP_PM_OK => "SMP: AP PM-entry OK\r\n",
            key::SMP_PM_NG => "SMP: AP PM-entry not observed\r\n",
            key::SMP_LM_OK => "SMP: AP LM-entry OK\r\n",
            key::SMP_LM_NG => "SMP: AP LM-entry not observed\r\n",
            key::SMP_LM_COUNT => "SMP: AP LM-count=",
            key::SMP_APIC_BYTE => "SMP: AP APIC-ID(byte)=",
            key::SMP_AP_IDS => "SMP: AP IDs=",
            key::SMP_READY => "SMP: AP READY=",
            key::VIRTIO_SCAN => "VirtIO: scanning ECAM segments\r\n",
            key::VIRTIO_NONE => "VirtIO: no devices found\r\n",
            key::VIRTIO_BLK => "VirtIO-blk: capacity=",
            key::VIRTIO_BLK_NONE => "VirtIO-blk: not found\r\n",
            key::VIRTIO_NET => "VirtIO-net: present\r\n",
            key::VIRTIO_NET_NONE => "VirtIO-net: not found\r\n",
            key::IOMMU_VTD_NONE => "VT-d: DMAR not found\r\n",
            key::IOMMU_AMDV_NONE => "AMD-Vi: IVRS not found\r\n",
            key::SEC_WP_ON => "Security: CR0.WP=ON\r\n",
            key::SEC_WP_OFF => "Security: CR0.WP=OFF\r\n",
            key::SEC_SMEP_ON => "Security: CR4.SMEP=ON\r\n",
            key::SEC_SMEP_OFF => "Security: CR4.SMEP=OFF\r\n",
            key::SEC_SMAP_ON => "Security: CR4.SMAP=ON\r\n",
            key::SEC_SMAP_OFF => "Security: CR4.SMAP=OFF\r\n",
            key::SEC_NXE_ON => "Security: EFER.NXE=ON\r\n",
            key::SEC_NXE_OFF => "Security: EFER.NXE=OFF\r\n",
            key::SEC_SUMMARY_OK => "Security: protections OK (WP/SMEP/SMAP/NXE)\r\n",
            key::SEC_SUMMARY_NG => "Security: protections NOT fully enabled\r\n",
            key::MIG_TRACK_START_OK => "migrate: tracking started\r\n",
            key::MIG_TRACK_START_FAIL => "migrate: start failed\r\n",
            key::MIG_TRACK_STOP_OK => "migrate: tracking stopped\r\n",
            key::MIG_TRACK_STOP_FAIL => "migrate: stop failed\r\n",
            key::MIG_CHAN_NEW_OK => "migrate: chan new ok\r\n",
            key::MIG_CHAN_NEW_FAIL => "migrate: chan new failed\r\n",
            key::MIG_CHAN_CLEARED => "migrate: chan cleared\r\n",
            key::MIG_NO_BUFFER => "migrate: no buffer\r\n",
            key::MIG_NET_MAC_PREFIX => "net: mac=",
            key::MIG_NET_MTU_PREFIX => "net: mtu=",
            key::MIG_NET_MAC_UPDATED => "net: mac updated\r\n",
            key::MIG_NET_MTU_UPDATED => "net: mtu updated\r\n",
            key::MIG_NET_USAGE => "usage: migrate net [mac|mtu] ...\r\n",
            key::MIG_NET_MAC_USAGE => "usage: migrate net mac [get|set xx:xx:xx:xx:xx:xx]\r\n",
            key::MIG_NET_MTU_USAGE => "usage: migrate net mtu [get|set <n>]\r\n",
            key::MIG_NET_ETHER_PREFIX => "net: ether=0x",
            key::MIG_NET_ETHER_UPDATED => "net: ether updated\r\n",
            key::MIG_NET_ETHER_USAGE => "usage: migrate net ether [get|set <hex>]\r\n",
            key::IOMMU_CFG_SAVED => "iommu: cfg saved\r\n",
            key::IOMMU_CFG_LOADED => "iommu: cfg loaded\r\n",
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
            key::HPET_PRESENT => "HPET: 検出 base=0x",
            key::HPET_NOT_FOUND => "HPET: 見つかりません\r\n",
            key::SMP_EXPECTED => "SMP: 期待CPU数=",
            key::SMP_OBSERVED => "SMP: 観測AP ID数=",
            key::SMP_PM_OK => "SMP: AP 保護モード到達 OK\r\n",
            key::SMP_PM_NG => "SMP: AP 保護モード未到達\r\n",
            key::SMP_LM_OK => "SMP: AP 長モード到達 OK\r\n",
            key::SMP_LM_NG => "SMP: AP 長モード未到達\r\n",
            key::SMP_LM_COUNT => "SMP: AP 長モード回数=",
            key::SMP_APIC_BYTE => "SMP: AP APIC-ID(下位1B)=",
            key::SMP_AP_IDS => "SMP: AP ID配列=",
            key::SMP_READY => "SMP: AP READY=",
            key::VIRTIO_SCAN => "VirtIO: ECAMセグメントを走査中\r\n",
            key::VIRTIO_NONE => "VirtIO: デバイスが見つかりません\r\n",
            key::VIRTIO_BLK => "VirtIO-blk: 容量=",
            key::VIRTIO_BLK_NONE => "VirtIO-blk: 見つかりません\r\n",
            key::VIRTIO_NET => "VirtIO-net: 検出\r\n",
            key::VIRTIO_NET_NONE => "VirtIO-net: 見つかりません\r\n",
            key::IOMMU_VTD_NONE => "VT-d: DMARが見つかりません\r\n",
            key::IOMMU_AMDV_NONE => "AMD-Vi: IVRSが見つかりません\r\n",
            key::SEC_WP_ON => "セキュリティ: CR0.WP=有効\r\n",
            key::SEC_WP_OFF => "セキュリティ: CR0.WP=無効\r\n",
            key::SEC_SMEP_ON => "セキュリティ: CR4.SMEP=有効\r\n",
            key::SEC_SMEP_OFF => "セキュリティ: CR4.SMEP=無効\r\n",
            key::SEC_SMAP_ON => "セキュリティ: CR4.SMAP=有効\r\n",
            key::SEC_SMAP_OFF => "セキュリティ: CR4.SMAP=無効\r\n",
            key::SEC_NXE_ON => "セキュリティ: EFER.NXE=有効\r\n",
            key::SEC_NXE_OFF => "セキュリティ: EFER.NXE=無効\r\n",
            key::SEC_SUMMARY_OK => "セキュリティ: 保護は有効（WP/SMEP/SMAP/NXE）\r\n",
            key::SEC_SUMMARY_NG => "セキュリティ: 保護が十分ではありません\r\n",
            key::MIG_TRACK_START_OK => "migrate: 追跡を開始しました\r\n",
            key::MIG_TRACK_START_FAIL => "migrate: 開始に失敗しました\r\n",
            key::MIG_TRACK_STOP_OK => "migrate: 追跡を停止しました\r\n",
            key::MIG_TRACK_STOP_FAIL => "migrate: 停止に失敗しました\r\n",
            key::MIG_CHAN_NEW_OK => "migrate: チャネル作成に成功\r\n",
            key::MIG_CHAN_NEW_FAIL => "migrate: チャネル作成に失敗\r\n",
            key::MIG_CHAN_CLEARED => "migrate: チャネルをクリアしました\r\n",
            key::MIG_NO_BUFFER => "migrate: バッファがありません\r\n",
            key::MIG_NET_MAC_PREFIX => "net: MAC=",
            key::MIG_NET_MTU_PREFIX => "net: MTU=",
            key::MIG_NET_MAC_UPDATED => "net: MACを更新しました\r\n",
            key::MIG_NET_MTU_UPDATED => "net: MTUを更新しました\r\n",
            key::MIG_NET_USAGE => "usage: migrate net [mac|mtu] ...\r\n",
            key::MIG_NET_MAC_USAGE => "usage: migrate net mac [get|set xx:xx:xx:xx:xx:xx]\r\n",
            key::MIG_NET_MTU_USAGE => "usage: migrate net mtu [get|set <n>]\r\n",
            key::MIG_NET_ETHER_PREFIX => "net: EtherType=0x",
            key::MIG_NET_ETHER_UPDATED => "net: EtherTypeを更新しました\r\n",
            key::MIG_NET_ETHER_USAGE => "usage: migrate net ether [get|set <hex>]\r\n",
            key::IOMMU_CFG_SAVED => "iommu: 設定を保存しました\r\n",
            key::IOMMU_CFG_LOADED => "iommu: 設定を読み込みました\r\n",
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
            key::HPET_PRESENT => "HPET: 已检测 base=0x",
            key::HPET_NOT_FOUND => "HPET: 未找到\r\n",
            key::SMP_EXPECTED => "SMP: 预期CPU数=",
            key::SMP_OBSERVED => "SMP: 已观测AP ID数=",
            key::SMP_PM_OK => "SMP: AP 保护模式就绪\r\n",
            key::SMP_PM_NG => "SMP: AP 保护模式未就绪\r\n",
            key::SMP_LM_OK => "SMP: AP 长模式就绪\r\n",
            key::SMP_LM_NG => "SMP: AP 长模式未就绪\r\n",
            key::SMP_LM_COUNT => "SMP: AP 长模式计数=",
            key::SMP_APIC_BYTE => "SMP: AP APIC-ID(低1字节)=",
            key::SMP_AP_IDS => "SMP: AP ID列表=",
            key::SMP_READY => "SMP: AP READY=",
            key::VIRTIO_SCAN => "VirtIO: 正在扫描ECAM段\r\n",
            key::VIRTIO_NONE => "VirtIO: 未找到设备\r\n",
            key::VIRTIO_BLK => "VirtIO-blk: 容量=",
            key::VIRTIO_BLK_NONE => "VirtIO-blk: 未找到\r\n",
            key::VIRTIO_NET => "VirtIO-net: 已检测\r\n",
            key::VIRTIO_NET_NONE => "VirtIO-net: 未找到\r\n",
            key::IOMMU_VTD_NONE => "VT-d: 未找到DMAR\r\n",
            key::IOMMU_AMDV_NONE => "AMD-Vi: 未找到IVRS\r\n",
            key::SEC_WP_ON => "安全: CR0.WP=启用\r\n",
            key::SEC_WP_OFF => "安全: CR0.WP=未启用\r\n",
            key::SEC_SMEP_ON => "安全: CR4.SMEP=启用\r\n",
            key::SEC_SMEP_OFF => "安全: CR4.SMEP=未启用\r\n",
            key::SEC_SMAP_ON => "安全: CR4.SMAP=启用\r\n",
            key::SEC_SMAP_OFF => "安全: CR4.SMAP=未启用\r\n",
            key::SEC_NXE_ON => "安全: EFER.NXE=启用\r\n",
            key::SEC_NXE_OFF => "安全: EFER.NXE=未启用\r\n",
            key::SEC_SUMMARY_OK => "安全: 保护正常（WP/SMEP/SMAP/NXE）\r\n",
            key::SEC_SUMMARY_NG => "安全: 保护未完全启用\r\n",
            key::MIG_TRACK_START_OK => "migrate: 已开始跟踪\r\n",
            key::MIG_TRACK_START_FAIL => "migrate: 启动失败\r\n",
            key::MIG_TRACK_STOP_OK => "migrate: 已停止跟踪\r\n",
            key::MIG_TRACK_STOP_FAIL => "migrate: 停止失败\r\n",
            key::MIG_CHAN_NEW_OK => "migrate: 通道创建成功\r\n",
            key::MIG_CHAN_NEW_FAIL => "migrate: 通道创建失败\r\n",
            key::MIG_CHAN_CLEARED => "migrate: 通道已清空\r\n",
            key::MIG_NO_BUFFER => "migrate: 无缓冲区\r\n",
            key::MIG_NET_MAC_PREFIX => "net: MAC=",
            key::MIG_NET_MTU_PREFIX => "net: MTU=",
            key::MIG_NET_MAC_UPDATED => "net: 已更新MAC\r\n",
            key::MIG_NET_MTU_UPDATED => "net: 已更新MTU\r\n",
            key::MIG_NET_USAGE => "usage: migrate net [mac|mtu] ...\r\n",
            key::MIG_NET_MAC_USAGE => "usage: migrate net mac [get|set xx:xx:xx:xx:xx:xx]\r\n",
            key::MIG_NET_MTU_USAGE => "usage: migrate net mtu [get|set <n>]\r\n",
            key::MIG_NET_ETHER_PREFIX => "net: EtherType=0x",
            key::MIG_NET_ETHER_UPDATED => "net: 已更新EtherType\r\n",
            key::MIG_NET_ETHER_USAGE => "usage: migrate net ether [get|set <hex>]\r\n",
            key::IOMMU_CFG_SAVED => "iommu: 已保存配置\r\n",
            key::IOMMU_CFG_LOADED => "iommu: 已加载配置\r\n",
            _ => "\r\n",
        },
    }
}


