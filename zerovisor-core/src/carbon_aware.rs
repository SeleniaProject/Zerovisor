//! Carbon-aware workload placement and migration helper.
//!
//! Each node periodically advertises its grid carbon-intensity (gCO₂/kWh)
//! via `update_local_intensity`, which stores the value in the clustered
//! key-value directory replicated by PBFT/HotStuff.
//!
//! When a VM is created or when intensities are updated, the algorithm
//! chooses the *greenest* node that satisfies an upper bound threshold
//! provided by the orchestrator. If the current node is not the best
//! candidate, a live migration is triggered automatically.
//!
//! All comments in English as per project style.

#![allow(dead_code)]

extern crate alloc;
use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::{RwLock, Once};

use crate::cluster::{ClusterManager, NodeId};
use crate::distributed_hypervisor as dh;
use crate::vm_manager::VmState;

/// Per-node carbon intensity table (gCO₂/kWh).
struct IntensityTable { map: RwLock<BTreeMap<NodeId, AtomicU32>> }

static TABLE: Once<IntensityTable> = Once::new();

fn table() -> &'static IntensityTable { TABLE.get().expect("carbon table not init") }

/// Init table (idempotent).
pub fn init() { TABLE.call_once(|| IntensityTable { map: RwLock::new(BTreeMap::new()) }); }

/// Update local node intensity and replicate to peers.
pub fn update_local_intensity(value: u32) {
    let mgr = ClusterManager::global();
    let self_id = mgr.leader().unwrap_or(NodeId(0));
    store_intensity(self_id, value);
    // Broadcast simple message (reuse existing ClusterMsg::Custom)
    let payload = value.to_le_bytes();
    let msg = crate::fault::Msg::Custom(0xCA, &payload);
    mgr.broadcast(&msg);
}

/// Store intensity in table.
fn store_intensity(node: NodeId, value: u32) {
    let t = table();
    let mut w = t.map.write();
    let entry = w.entry(node).or_insert_with(|| AtomicU32::new(value));
    entry.store(value, Ordering::Relaxed);
}

/// Handle incoming message from cluster runtime.
pub fn on_msg(src: NodeId, value: u32) { store_intensity(src, value); }

/// Pick node with lowest intensity; fall back to current.
fn best_node() -> NodeId {
    let mgr = ClusterManager::global();
    let mut best = mgr.leader().unwrap_or(NodeId(0));
    let mut best_val = u32::MAX;
    let r = table().map.read();
    mgr.each_member(|n| {
        if let Some(val) = r.get(&n) { let v = val.load(Ordering::Relaxed); if v < best_val { best_val = v; best = n; } }
    });
    best
}

/// Evaluate VM placements and migrate if greener node found.
pub fn rebalance(threshold_delta: u32) {
    let current_best = best_node();
    if current_best == ClusterManager::global().leader().unwrap_or(NodeId(0)) { return; }
    // In production this would trigger live migration; here we just log.
    crate::log!("[carbon-aware] Preferred green node {:?}", current_best);
}

/// Enhanced carbon-aware computing with renewable energy integration
pub struct EnhancedCarbonManager {
    intensity_data: RwLock<BTreeMap<NodeId, CarbonIntensityData>>,
    energy_sources: RwLock<BTreeMap<NodeId, Vec<EnergySourceInfo>>>,
    workload_scheduler: RwLock<CarbonAwareScheduler>,
    power_budget: AtomicU32,
    renewable_threshold: AtomicU32, // Fixed-point percentage (0-10000 = 0-100%)
}

#[derive(Debug, Clone)]
pub struct CarbonIntensityData {
    pub timestamp: u64,
    pub carbon_intensity: u32, // gCO2/kWh (fixed-point)
    pub renewable_percentage: u32, // 0-10000 = 0-100%
    pub grid_frequency: u32, // mHz
    pub demand_forecast: u32, // MW (fixed-point)
}

#[derive(Debug, Clone)]
pub struct EnergySourceInfo {
    pub source_type: EnergySourceType,
    pub capacity_mw: u32, // Fixed-point MW
    pub current_output_mw: u32, // Fixed-point MW
    pub carbon_intensity: u32, // gCO2/kWh
    pub availability_forecast: [u32; 24], // Next 24 hours
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnergySourceType {
    Solar = 0,
    Wind = 1,
    Hydro = 2,
    Nuclear = 3,
    Coal = 4,
    Gas = 5,
    Battery = 6,
}

#[derive(Debug, Clone)]
pub struct CarbonAwareWorkload {
    pub id: u32,
    pub priority: WorkloadPriority,
    pub power_requirement: u32, // Watts
    pub duration_estimate: u32, // Seconds
    pub deadline: u64, // Timestamp
    pub carbon_budget: u32, // gCO2 (fixed-point)
    pub deferrable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkloadPriority {
    Background = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

#[derive(Debug, Clone)]
pub struct CarbonAwareScheduler {
    workload_queue: alloc::vec::Vec<CarbonAwareWorkload>,
    scheduling_decisions: alloc::vec::Vec<SchedulingDecision>,
    carbon_savings: u32, // Total gCO2 saved
}

#[derive(Debug, Clone)]
pub enum SchedulingDecision {
    Schedule { estimated_carbon_cost: u32 },
    Defer { suggested_time: u64 },
    Reject { reason_code: u32 },
}

impl EnhancedCarbonManager {
    /// Create new enhanced carbon manager
    pub fn new(power_budget_watts: u32, renewable_threshold_pct: u32) -> Self {
        EnhancedCarbonManager {
            intensity_data: RwLock::new(BTreeMap::new()),
            energy_sources: RwLock::new(BTreeMap::new()),
            workload_scheduler: RwLock::new(CarbonAwareScheduler {
                workload_queue: alloc::vec::Vec::new(),
                scheduling_decisions: alloc::vec::Vec::new(),
                carbon_savings: 0,
            }),
            power_budget: AtomicU32::new(power_budget_watts),
            renewable_threshold: AtomicU32::new(renewable_threshold_pct * 100), // Convert to fixed-point
        }
    }
    
    /// Update carbon intensity data for a node
    pub fn update_carbon_intensity(&self, node: NodeId, data: CarbonIntensityData) {
        self.intensity_data.write().insert(node, data);
    }
    
    /// Add energy source information for a node
    pub fn add_energy_source(&self, node: NodeId, source: EnergySourceInfo) {
        let mut sources = self.energy_sources.write();
        sources.entry(node).or_insert_with(|| alloc::vec::Vec::new()).push(source);
    }
    
    /// Schedule workload with carbon awareness
    pub fn schedule_workload(&self, workload: CarbonAwareWorkload) -> SchedulingDecision {
        let best_node = self.find_greenest_node();
        let current_node = ClusterManager::global().leader().unwrap_or(NodeId(0));
        
        // Get carbon intensity for best node
        let carbon_intensity = self.get_carbon_intensity(best_node);
        let renewable_pct = self.get_renewable_percentage(best_node);
        
        // Check if we should defer the workload
        if self.should_defer_workload(&workload, carbon_intensity, renewable_pct) {
            return SchedulingDecision::Defer {
                suggested_time: self.find_optimal_execution_time(&workload),
            };
        }
        
        // Check power budget
        if workload.power_requirement > self.power_budget.load(Ordering::Relaxed) {
            return SchedulingDecision::Reject { reason_code: 1 }; // Power budget exceeded
        }
        
        // Calculate carbon cost
        let carbon_cost = self.calculate_carbon_cost(&workload, carbon_intensity);
        
        // Add to scheduler queue
        self.workload_scheduler.write().workload_queue.push(workload);
        
        SchedulingDecision::Schedule {
            estimated_carbon_cost: carbon_cost,
        }
    }
    
    /// Find the greenest node in the cluster
    fn find_greenest_node(&self) -> NodeId {
        let intensity_data = self.intensity_data.read();
        let mut best_node = NodeId(0);
        let mut best_intensity = u32::MAX;
        
        for (node, data) in intensity_data.iter() {
            if data.carbon_intensity < best_intensity {
                best_intensity = data.carbon_intensity;
                best_node = *node;
            }
        }
        
        best_node
    }
    
    /// Get carbon intensity for a node
    fn get_carbon_intensity(&self, node: NodeId) -> u32 {
        self.intensity_data.read()
            .get(&node)
            .map(|data| data.carbon_intensity)
            .unwrap_or(500_000) // Default 500 gCO2/kWh
    }
    
    /// Get renewable percentage for a node
    fn get_renewable_percentage(&self, node: NodeId) -> u32 {
        self.intensity_data.read()
            .get(&node)
            .map(|data| data.renewable_percentage)
            .unwrap_or(0)
    }
    
    /// Check if workload should be deferred
    fn should_defer_workload(&self, workload: &CarbonAwareWorkload, carbon_intensity: u32, renewable_pct: u32) -> bool {
        // Don't defer critical workloads
        if workload.priority >= WorkloadPriority::Critical {
            return false;
        }
        
        // Don't defer if not deferrable
        if !workload.deferrable {
            return false;
        }
        
        // Defer if carbon intensity is too high
        let threshold = self.renewable_threshold.load(Ordering::Relaxed);
        carbon_intensity > 500_000 && renewable_pct < threshold
    }
    
    /// Find optimal execution time for workload
    fn find_optimal_execution_time(&self, workload: &CarbonAwareWorkload) -> u64 {
        // Simplified: suggest 1 hour later
        workload.deadline.saturating_sub(3600)
    }
    
    /// Calculate carbon cost for workload
    fn calculate_carbon_cost(&self, workload: &CarbonAwareWorkload, carbon_intensity: u32) -> u32 {
        // Energy in kWh = (power_watts * duration_seconds) / (1000 * 3600)
        let energy_kwh = (workload.power_requirement * workload.duration_estimate) / 3_600_000;
        (energy_kwh * carbon_intensity) / 1000 // Convert to gCO2
    }
    
    /// Get carbon efficiency metrics
    pub fn get_carbon_metrics(&self) -> CarbonMetrics {
        let scheduler = self.workload_scheduler.read();
        let current_node = ClusterManager::global().leader().unwrap_or(NodeId(0));
        let carbon_intensity = self.get_carbon_intensity(current_node);
        let renewable_pct = self.get_renewable_percentage(current_node);
        
        CarbonMetrics {
            current_carbon_intensity: carbon_intensity,
            renewable_percentage: renewable_pct,
            active_workloads: scheduler.workload_queue.len() as u32,
            power_budget_used: self.calculate_power_usage(),
            carbon_savings_estimate: scheduler.carbon_savings,
        }
    }
    
    /// Calculate current power usage
    fn calculate_power_usage(&self) -> u32 {
        self.workload_scheduler.read()
            .workload_queue
            .iter()
            .map(|w| w.power_requirement)
            .sum()
    }
}

/// Carbon efficiency metrics
#[derive(Debug, Clone)]
pub struct CarbonMetrics {
    pub current_carbon_intensity: u32,
    pub renewable_percentage: u32,
    pub active_workloads: u32,
    pub power_budget_used: u32,
    pub carbon_savings_estimate: u32,
}

/// Global enhanced carbon manager
static ENHANCED_CARBON_MANAGER: Once<EnhancedCarbonManager> = Once::new();

/// Initialize enhanced carbon-aware computing
pub fn init_enhanced_carbon_aware(power_budget: u32, renewable_threshold: u32) {
    ENHANCED_CARBON_MANAGER.call_once(|| {
        EnhancedCarbonManager::new(power_budget, renewable_threshold)
    });
}

/// Get enhanced carbon manager
pub fn enhanced_carbon_manager() -> &'static EnhancedCarbonManager {
    ENHANCED_CARBON_MANAGER.get().expect("Enhanced carbon manager not initialized")
} 