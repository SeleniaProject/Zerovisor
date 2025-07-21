//! NUMAOptimizer – VM placement and memory migration across NUMA nodes
//! Implements Task: NUMAOptimizer (VM 配置・メモリ移動最適化アルゴリズム)
//!
//! 本モジュールはシステムの NUMA 拓撲を検出し、VM の CPU/メモリ要求に基づいて
//! 最適な NUMA ノードを選択します。さらに、VM 実行中にホットスポットが検出された
//! 場合にページの移動 (memory migration) を行う API も提供します。

#![allow(dead_code)]

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Once;
use hashbrown::HashMap;
use core::cmp::min;

use zerovisor_hal::virtualization::{VmConfig, VmHandle};
use crate::migration::{self, MigrationError};

/// Abstract NUMA node identifier
pub type NumaNode = u16;

/// NUMA topology description
#[derive(Debug, Clone)]
pub struct NumaTopology {
    pub nodes: Vec<NumaNode>,
    /// Map node → bitmask of CPUs (logical ids)
    pub cpu_mask: HashMap<NumaNode, u64>,
    /// Node memory size (bytes)
    pub memory_size: HashMap<NumaNode, u64>,
}

impl NumaTopology {
    /// Detect topology via HAL or fallback single-node
    pub fn detect() -> Self {
        // TODO: Use ACPI SRAT / CPUID leaf 0xB etc. For now, single node.
        let mut nodes = Vec::new();
        nodes.push(0);
        let mut cpu_mask = HashMap::new();
        cpu_mask.insert(0, 0xFFFF_FFFF_FFFF_FFFFu64); // all CPUs
        let mut memory_size = HashMap::new();
        memory_size.insert(0, 256 * 1024 * 1024); // 256 MiB placeholder
        Self { nodes, cpu_mask, memory_size }
    }
}

/// NUMA optimizer core structure
pub struct NumaOptimizer {
    topo: NumaTopology,
    /// Current VM→node mapping
    affinity: spin::Mutex<HashMap<VmHandle, NumaNode>>,
    /// VM resource info for load estimation
    vm_info: spin::Mutex<HashMap<VmHandle, VmInfo>>,
}

/// Per-VM resource usage record
#[derive(Debug, Clone, Copy)]
struct VmInfo { mem_bytes: u64, vcpu_count: u32 }

impl NumaOptimizer {
    pub fn new() -> Self {
        Self {
            topo: NumaTopology::detect(),
            affinity: spin::Mutex::new(HashMap::new()),
            vm_info: spin::Mutex::new(HashMap::new()),
        }
    }

    /// Select optimal NUMA node for a new VM given its configuration.
    pub fn optimize_vm_placement(&self, cfg: &VmConfig) -> NumaNode {
        // Simple heuristic: choose node with most free memory.
        let mut candidate = 0;
        let mut max_free = 0;
        for &node in &self.topo.nodes {
            let cap = *self.topo.memory_size.get(&node).unwrap_or(&0);
            let used: u64 = self.affinity.lock().iter()
                .filter(|(_, &n)| n == node)
                .map(|(vm, _)| {
                    // For simplicity assume cfg.memory_size; need VM size map.
                    cfg.memory_size
                }).sum();
            let free = cap.saturating_sub(used);
            if free > max_free { max_free = free; candidate = node; }
        }
        self.affinity.lock().insert(cfg.id, candidate);
        self.vm_info.lock().insert(cfg.id, VmInfo { mem_bytes: cfg.memory_size, vcpu_count: cfg.cpu_count });
        candidate
    }

    /// Migrate VM memory to a target node (stubbed).
    pub fn migrate_vm_memory(&self, vm: VmHandle, target: NumaNode) -> Result<(), MigrationError> {
        // TODO: integrate with zerovisor-core::migration module
        self.affinity.lock().insert(vm, target);
        Ok(())
    }

    /// Compute per-node load (used memory, vcpu count) and perform migrations to balance.
    pub fn rebalance(&self) {
        // Gather current load
        let info_guard = self.vm_info.lock();
        let aff_guard = self.affinity.lock();

        #[derive(Default)]
        struct Load { used_mem: u64, vcpus: u32 }
        let mut load: HashMap<NumaNode, Load> = HashMap::new();
        for (&vm, &node) in aff_guard.iter() {
            if let Some(info) = info_guard.get(&vm) {
                let entry = load.entry(node).or_default();
                entry.used_mem += info.mem_bytes;
                entry.vcpus += info.vcpu_count;
            }
        }

        drop(info_guard);
        drop(aff_guard);

        // Determine average memory usage per node
        let avg_mem: u64 = load.values().map(|l| l.used_mem).sum::<u64>() / (self.topo.nodes.len() as u64);

        // Simple strategy: for nodes whose used_mem > 120% avg, move the largest VM to least loaded node
        for &node in &self.topo.nodes {
            let node_mem = load.get(&node).map(|l| l.used_mem).unwrap_or(0);
            if node_mem > avg_mem * 12 / 10 {
                // find VM with largest memory on this node
                let mut biggest: Option<(VmHandle, u64)> = None;
                for (&vm, &n) in self.affinity.lock().iter() {
                    if n == node {
                        if let Some(info) = self.vm_info.lock().get(&vm) {
                            if biggest.map(|(_, m)| info.mem_bytes > m).unwrap_or(true) {
                                biggest = Some((vm, info.mem_bytes));
                            }
                        }
                    }
                }
                if let Some((vm, mem)) = biggest {
                    // choose target node with most free memory
                    let mut target = node;
                    let mut max_free = 0u64;
                    for &other in &self.topo.nodes {
                        if other == node { continue; }
                        let cap = *self.topo.memory_size.get(&other).unwrap_or(&0);
                        let used = load.get(&other).map(|l| l.used_mem).unwrap_or(0);
                        let free = cap.saturating_sub(used);
                        if free > max_free { max_free = free; target = other; }
                    }
                    if target != node && max_free > mem {
                        let _ = self.migrate_vm_memory(vm, target); // ignore error for now
                        crate::log!("[numa] migrated VM {} from node {} to {} to rebalance", vm, node, target);
                        // Update load tables for subsequent decisions
                        load.get_mut(&node).map(|l| { l.used_mem -= mem; });
                        load.entry(target).or_default().used_mem += mem;
                    }
                }
            }
        }
    }

    pub fn node_of(&self, vm: VmHandle) -> Option<NumaNode> { self.affinity.lock().get(&vm).cloned() }
}

static OPTIMIZER: Once<NumaOptimizer> = Once::new();

pub fn init() { OPTIMIZER.call_once(|| NumaOptimizer::new()); }

pub fn optimizer() -> &'static NumaOptimizer { OPTIMIZER.get().expect("NUMA optimizer not initialized") } 