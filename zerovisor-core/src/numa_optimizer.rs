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
}

impl NumaOptimizer {
    pub fn new() -> Self {
        Self { topo: NumaTopology::detect(), affinity: spin::Mutex::new(HashMap::new()) }
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
        candidate
    }

    /// Migrate VM memory to a target node (stubbed).
    pub fn migrate_vm_memory(&self, vm: VmHandle, target: NumaNode) -> Result<(), MigrationError> {
        // TODO: integrate with zerovisor-core::migration module
        self.affinity.lock().insert(vm, target);
        Ok(())
    }

    pub fn node_of(&self, vm: VmHandle) -> Option<NumaNode> { self.affinity.lock().get(&vm).cloned() }
}

static OPTIMIZER: Once<NumaOptimizer> = Once::new();

pub fn init() { OPTIMIZER.call_once(|| NumaOptimizer::new()); }

pub fn optimizer() -> &'static NumaOptimizer { OPTIMIZER.get().expect("NUMA optimizer not initialized") } 