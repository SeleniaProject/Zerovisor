//! Quantum scheduler implementation

//! 実時間保証と優先度スケジューリングを備えた決定論的スケジューラ。

#![allow(clippy::module_name_repetitions)]

extern crate alloc;

use alloc::collections::BinaryHeap;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::cmp::Ordering;
use spin::Mutex;
use core::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use zerovisor_hal::virtualization::{VmHandle, VcpuHandle};

use crate::ZerovisorError;
use crate::security::{self, SecurityEvent};

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
                // まず締切が近い方を優先 (値が小さい)。
                let deadline_cmp = match (self.deadline_ns, other.deadline_ns) {
                    (Some(a), Some(b)) => b.cmp(&a), // smaller deadline first (BinaryHeap is max-heap)
                    (Some(_), None) => Ordering::Greater,
                    (None, Some(_)) => Ordering::Less,
                    (None, None) => Ordering::Equal,
                };
                if deadline_cmp != Ordering::Equal {
                    deadline_cmp
                } else {
                    // 最終 tie-break: VMID と VCPUID で決定論的に順序付け
                    match self.vm.cmp(&other.vm) {
                        Ordering::Equal => self.vcpu.cmp(&other.vcpu),
                        ord => ord,
                    }
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

pub struct ExecStats {
    pub max_ns: u64,
    pub total_ns: u64,
    pub count: u64,
}

// --------------------------------------------------------------------------
// QuantumScheduler
// --------------------------------------------------------------------------

pub struct QuantumScheduler {
    ready_queue: BinaryHeap<SchedEntity>,
    real_time_queue: BinaryHeap<SchedEntity>,
    quantum_ns: u64,
    stats: BTreeMap<(VmHandle, VcpuHandle), ExecStats>,
}

impl QuantumScheduler {
    pub const fn new() -> Self {
        Self {
            ready_queue: BinaryHeap::new(),
            real_time_queue: BinaryHeap::new(),
            quantum_ns: 1_000_000, // 1ms デフォルト
            stats: BTreeMap::new(),
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

    /// チェックして期限を過ぎた RT エンティティがあれば SecurityEvent を発行。
    fn check_rt_deadlines(&mut self) {
        let now = cycles_to_nanoseconds(get_cycle_counter());
        // BinaryHeap なので直接イテレートしづらい。ここではコピーして検査。
        let mut overdue: Vec<SchedEntity> = self.real_time_queue
            .iter()
            .filter(|e| e.deadline_ns.map_or(false, |d| d <= now))
            .cloned()
            .collect();
        for ent in &overdue {
            // 最低優先度を 255 (max) に引き上げて即実行させる。
            let mut ent_mut = *ent;
            ent_mut.priority = 255;
            // 再投入
            self.ready_queue.push(ent_mut);
            // 統計
            DEADLINE_MISSES.fetch_add(1, AtomicOrdering::Relaxed);
            // セキュリティ / RT 警告
            security::record_event(SecurityEvent::RealTimeDeadlineMiss {
                vm: ent.vm,
                vcpu: ent.vcpu,
                deadline_ns: ent.deadline_ns.unwrap_or(0),
                now_ns: now,
            });
        }
        // overdue エントリを RT キューから除去
        self.real_time_queue
            .retain(|e| !overdue.iter().any(|o| o.vcpu == e.vcpu && o.vm == e.vm));
    }

    /// 次に実行すべきエンティティを決定。
    pub fn schedule_next(&mut self) -> Option<SchedEntity> {
        // Measure scheduler latency for WCET analysis
        let start_cycles = get_cycle_counter();

        // デッドライン監視
        self.check_rt_deadlines();

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
        let ent = self.ready_queue.pop();

        // Latency measurement
        let latency_ns = cycles_to_nanoseconds(get_cycle_counter() - start_cycles);
        if latency_ns > MAX_INTERRUPT_LATENCY_NS {
            DEADLINE_MISSES.fetch_add(1, AtomicOrdering::Relaxed);
        }

        ent
    }

    /// 量子が満了した際の処理。
    pub fn handle_quantum_expiry(&mut self, entity: SchedEntity) {
        // RR のようにキューへ戻す。
        self.add_entity(entity);
    }

    /// 量子長を ns 単位で設定。
    pub fn set_quantum_ns(&mut self, ns: u64) { self.quantum_ns = ns; }

    pub fn record_exec_time(&mut self, entity: SchedEntity, exec_ns: u64) {
        let key = (entity.vm, entity.vcpu);
        let entry = self.stats.entry(key).or_insert(ExecStats { max_ns: 0, total_ns: 0, count: 0 });
        if exec_ns > entry.max_ns { entry.max_ns = exec_ns; }
        entry.total_ns += exec_ns;
        entry.count += 1;
    }

    /// Analyze collected WCET statistics and return true if any VCPU exceeds `threshold_ns`.
    pub fn wcet_violations(&self, threshold_ns: u64) -> Vec<(VmHandle, VcpuHandle, u64)> {
        self.stats
            .iter()
            .filter_map(|(&(vm, vcpu), stat)| {
                if stat.max_ns > threshold_ns {
                    Some((vm, vcpu, stat.max_ns))
                } else {
                    None
                }
            })
            .collect()
    }
}

// --------------------------------------------------------------------------
// グローバルスケジューラ
// --------------------------------------------------------------------------

static SCHEDULER: Mutex<QuantumScheduler> = Mutex::new(QuantumScheduler::new());
static DEADLINE_MISSES: AtomicU64 = AtomicU64::new(0);

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

/// 実行時間を記録。
pub fn record_exec_time(entity: SchedEntity, exec_ns: u64) {
    SCHEDULER.lock().record_exec_time(entity, exec_ns);
}

/// リアルタイムデッドラインミス総数を取得。
pub fn deadline_miss_count() -> u64 { DEADLINE_MISSES.load(AtomicOrdering::Relaxed) }

/// 優先度継承 (priority inheritance)。待機 VCPU の priority を一時的に引き上げる。
pub fn inherit_priority(vm: VmHandle, vcpu: VcpuHandle, new_priority: u8) {
    let mut sched = SCHEDULER.lock();
    // 探索: ready_queue および real_time_queue からエンティティを検索し更新。
    let mut updated = false;
    let mut tmp: Vec<SchedEntity> = Vec::new();
    while let Some(ent) = sched.ready_queue.pop() {
        if ent.vm == vm && ent.vcpu == vcpu {
            let mut ent_new = ent;
            if new_priority > ent_new.priority { ent_new.priority = new_priority; }
            tmp.push(ent_new);
            updated = true;
        } else {
            tmp.push(ent);
        }
    }
    for e in tmp { sched.ready_queue.push(e); }
    // 同様に RT キューも
    let mut rt_tmp: Vec<SchedEntity> = Vec::new();
    while let Some(ent) = sched.real_time_queue.pop() {
        if ent.vm == vm && ent.vcpu == vcpu {
            let mut ent_new = ent;
            if new_priority > ent_new.priority { ent_new.priority = new_priority; }
            rt_tmp.push(ent_new);
            updated = true;
        } else { rt_tmp.push(ent); }
    }
    for e in rt_tmp { sched.real_time_queue.push(e); }
    if updated {
        // ログ用に SecurityEvent 出力 (低優先度→高への継承は DoS 対策監査対象)
        security::record_event(SecurityEvent::PerfWarning { avg_latency_ns: 0, wcet_ns: None });
    }
 }

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

const MAX_INTERRUPT_LATENCY_NS: u64 = 1_000; // 1 µs

// Public API wrappers ----------------------------------------------------

/// Return WCET violations aggregated so far.
pub fn wcet_violations(threshold_ns: u64) -> Vec<(VmHandle, VcpuHandle, u64)> {
    SCHEDULER.lock().wcet_violations(threshold_ns)
}
/// Maximum scheduler/interrupt latency target (ns)
pub const MAX_SCHED_LATENCY_NS: u64 = MAX_INTERRUPT_LATENCY_NS;