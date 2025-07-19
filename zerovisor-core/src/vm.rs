//! Zerovisor virtual machine subsystem (Task 6.1)
//!
//! 本モジュールは仮想マシンの生成・削除・状態管理を担当します。
//! 仕様は `.kiro/specs/zerovisor-hypervisor/design.md` および
//! `requirements.md` に準拠し、以下の責務を負います。
//!
//! 1. VM 設定 (`VmConfig`) の妥当性検証
//! 2. 物理メモリ・EPT 構造体など必要リソースの割り当て/解放
//! 3. VM ライフサイクル状態 (Created/Running/Stopped/Destroyed) の管理
//! 4. ハイパーバイザ統計 (`monitor`, `security`) との連携
//!
//! **設計要件**
//! * Rust `no_std` + `alloc` 環境
//! * マルチコア同時 VM 操作を考慮したロック設計 (spin::Mutex)
//! * 将来的に NUMA 最適化・ライブマイグレーションを追加できる
//!
//! **注意**: VM 実行自体は HAL の `VirtualizationEngine` が担当します。
//! 当モジュールは *管理プレーン* を実装します。

#![allow(dead_code)]

extern crate alloc;

use alloc::{collections::BTreeMap, string::String, vec::Vec};
use core::cmp;
use spin::Mutex;

use zerovisor_hal::memory::{self, PhysicalAddress};
use crate::memory::AllocFlags;
use zerovisor_hal::virtualization::*;
use zerovisor_hal::HalError;
use crate::memory as hv_mem;
use crate::security::{self, SecurityEvent};
use crate::ZerovisorError;

/// VM ライフサイクル状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    Created,
    Running,
    Stopped,
    Destroyed,
}

/// VM 管理エラー
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmError {
    InvalidConfig,
    DuplicateId,
    ResourceExhausted,
    Hal(HalError),
    Memory(hv_mem::MemoryError),
    NotFound,
    InvalidState,
}

impl From<HalError> for VmError { fn from(e: HalError) -> Self { VmError::Hal(e) } }
impl From<hv_mem::MemoryError> for VmError { fn from(e: hv_mem::MemoryError) -> Self { VmError::Memory(e) } }
impl From<VmError> for ZerovisorError {
    fn from(err: VmError) -> Self {
        match err {
            VmError::Hal(e) => ZerovisorError::HalError(e),
            VmError::Memory(_) | VmError::ResourceExhausted => ZerovisorError::ResourceExhausted,
            _ => ZerovisorError::InvalidConfiguration,
        }
    }
}

/// 内部 VM 表現
struct VmEntry {
    cfg: VmConfig,
    state: VmState,
    /// 先頭物理フレーム
    mem_base: PhysicalAddress,
    /// 割り当てページ数
    pages: usize,
    /// Per-VM 統計
    stats: VmStats,
}

/// グローバル VM レジストリ
static VMS: Mutex<BTreeMap<VmHandle, VmEntry>> = Mutex::new(BTreeMap::new());

// --------------------------------------------------------------------------
// 公開 API
// --------------------------------------------------------------------------

/// サブシステム初期化
pub fn init() -> Result<(), ZerovisorError> {
    // 現在は特別な初期化不要だが将来の拡張に備え用意
    Ok(())
}

/// VM を生成し、リソースを割り当てる
pub fn create_vm(cfg: &VmConfig) -> Result<VmHandle, VmError> {
    validate_config(cfg)?;
    let mut map = VMS.lock();
    if map.contains_key(&cfg.id) {
        return Err(VmError::DuplicateId);
    }

    // 1. メモリ割り当て (ページ境界に切り上げ)
    let page_size = 4096;
    let pages = ((cfg.memory_size as usize) + page_size - 1) / page_size;
    let mem_base = hv_mem::allocate_pages(pages, AllocFlags::CONTIGUOUS)?;

    // 2. HAL で VM 作成
    // NOTE: VirtualizationEngine はコールサイト (VmManager) が保持する。
    // ここではメモリ管理のみ行う。

    // 3. レジストリ登録
    map.insert(cfg.id, VmEntry {
        cfg: cfg.clone(),
        state: VmState::Created,
        mem_base,
        pages,
        stats: VmStats::default(),
    });

    // 4. Security log
    security::record_event(SecurityEvent::PerfWarning { avg_latency_ns: 0 }); // placeholder

    Ok(cfg.id)
}

/// VM を破棄しリソースを解放
pub fn destroy_vm(handle: VmHandle) -> Result<(), VmError> {
    let mut map = VMS.lock();
    let entry = map.remove(&handle).ok_or(VmError::NotFound)?;

    // メモリ解放
    hv_mem::free_pages(entry.mem_base, entry.pages)?;

    Ok(())
}

/// VM 状態を取得
pub fn get_state(handle: VmHandle) -> Option<VmState> {
    VMS.lock().get(&handle).map(|e| e.state)
}

/// VM 統計を取得 (読み取り専用コピー)
pub fn get_stats(handle: VmHandle) -> Option<VmStats> {
    VMS.lock().get(&handle).map(|e| e.stats.clone())
}

// --------------------------------------------------------------------------
// 内部ヘルパ
// --------------------------------------------------------------------------

/// 設定妥当性検証
fn validate_config(cfg: &VmConfig) -> Result<(), VmError> {
    // 名前は null 終端されている想定。最小 1byte
    if cfg.name[0] == 0 {
        return Err(VmError::InvalidConfig);
    }

    // メモリサイズ 1MiB 以上 512GiB 以下
    const MIN_SIZE: u64 = 1 * 1024 * 1024;
    const MAX_SIZE: u64 = 512 * 1024 * 1024 * 1024;
    if !(MIN_SIZE..=MAX_SIZE).contains(&cfg.memory_size) {
        return Err(VmError::InvalidConfig);
    }

    // vCPU 数 1〜256
    if !(1..=256).contains(&cfg.vcpu_count) {
        return Err(VmError::InvalidConfig);
    }

    // セキュリティレベルと機能フラグの組み合わせをチェック
    if cfg.security_level == SecurityLevel::Maximum && !cfg.features.contains(VirtualizationFeatures::QUANTUM_SECURITY) {
        return Err(VmError::InvalidConfig);
    }

    Ok(())
}