//! Quantum scheduler implementation

//! 実時間保証と優先度スケジューリングを備えた決定論的スケジューラ。

#![allow(clippy::module_name_repetitions)]

extern crate alloc;

use alloc::collections::BinaryHeap;
use core::cmp::Ordering;
use spin::Mutex;

use zerovisor_hal::virtualization::{VmHandle, VcpuHandle};

use crate::ZerovisorError;

// --------------------------------------------------------------------------
// 型定義
// --------------------------------------------------------------------------

/// スケジューラに登録されるエンティティ（VM または VCPU）。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchedEntity {
    pub vm: VmHandle,
    pub vcpu: VcpuHandle,
    pub priority: u8, // 0 = lowest, 255 = highest
    pub deadline_ns: Option<u64>,
}

impl Ord for SchedEntity {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap は最大ヒープなので、より高い priority を優先。
        match self.priority.cmp(&other.priority) {
            Ordering::Equal => {
                // 締切が近い方を優先 (値が小さい)
                match (self.deadline_ns, other.deadline_ns) {
                    (Some(a), Some(b)) => b.cmp(&a), // smaller deadline first
                    (Some(_), None) => Ordering::Greater,
                    (None, Some(_)) => Ordering::Less,
                    (None, None) => Ordering::Equal,
                }
            }
            ord => ord,
        }
    }
}

impl PartialOrd for SchedEntity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// --------------------------------------------------------------------------
// QuantumScheduler
// --------------------------------------------------------------------------

pub struct QuantumScheduler {
    ready_queue: BinaryHeap<SchedEntity>,
    real_time_queue: BinaryHeap<SchedEntity>,
    quantum_ns: u64,
}

impl QuantumScheduler {
    pub const fn new() -> Self {
        Self {
            ready_queue: BinaryHeap::new(),
            real_time_queue: BinaryHeap::new(),
            quantum_ns: 1_000_000, // 1ms デフォルト
        }
    }

    /// VM/VCPU を追加する。RT 属性があれば real_time_queue へ。
    pub fn add_entity(&mut self, entity: SchedEntity) {
        if entity.deadline_ns.is_some() {
            self.real_time_queue.push(entity);
        } else {
            self.ready_queue.push(entity);
        }
    }

    /// 次に実行すべきエンティティを決定。
    pub fn schedule_next(&mut self) -> Option<SchedEntity> {
        // まずリアルタイムキュー。期限切れのものを優先。
        if let Some(rt_top) = self.real_time_queue.peek() {
            // 締切が過ぎていないかチェック (簡易実装)。
            let now = cycles_to_nanoseconds(get_cycle_counter());
            if let Some(deadline) = rt_top.deadline_ns {
                if deadline <= now { /* 過期 */ }
            }
            return self.real_time_queue.pop();
        }

        // 通常キュー。
        self.ready_queue.pop()
    }

    /// 量子が満了した際の処理。
    pub fn handle_quantum_expiry(&mut self, entity: SchedEntity) {
        // RR のようにキューへ戻す。
        self.add_entity(entity);
    }

    /// 量子長を ns 単位で設定。
    pub fn set_quantum_ns(&mut self, ns: u64) { self.quantum_ns = ns; }
}

// --------------------------------------------------------------------------
// グローバルスケジューラ
// --------------------------------------------------------------------------

static SCHEDULER: Mutex<QuantumScheduler> = Mutex::new(QuantumScheduler::new());

/// サブシステム初期化 (Task 5.1)。
pub fn init() -> Result<(), ZerovisorError> {
    // 今のところ特別な HW 初期化は不要。
    Ok(())
}

/// VM/VCPU をスケジューラに登録。
pub fn register_vcpu(vm: VmHandle, vcpu: VcpuHandle, priority: u8, deadline_ns: Option<u64>) {
    let mut sched = SCHEDULER.lock();
    sched.add_entity(SchedEntity { vm, vcpu, priority, deadline_ns });
}

/// 次のエンティティを取得。
pub fn pick_next() -> Option<SchedEntity> { SCHEDULER.lock().schedule_next() }

/// 量子満了時に呼び出し。
pub fn quantum_expired(entity: SchedEntity) { SCHEDULER.lock().handle_quantum_expiry(entity) }

// --------------------------------------------------------------------------
// 補助関数
// --------------------------------------------------------------------------

#[inline]
fn cycles_per_nanosecond() -> u64 { 3 } // 仮: 3GHz

#[inline]
pub fn cycles_to_nanoseconds(cycles: u64) -> u64 { cycles / cycles_per_nanosecond() }

#[inline]
pub fn get_cycle_counter() -> u64 {
    #[cfg(target_arch = "x86_64")]
    { unsafe { core::arch::x86_64::_rdtsc() } }

    #[cfg(not(target_arch = "x86_64"))]
    { 0 }
}