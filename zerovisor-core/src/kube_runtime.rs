//! kube_runtime – Minimal CRI-compliant runtime API
//! Provides stubs for PodSandbox and Container management so that zerovisor-sdk
//! can integrate with Kubernetes without external shims.
//! All comments are in English as required.

#![allow(dead_code)]

extern crate alloc;
use alloc::string::String;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::time::Duration;
use spin::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};
use crate::microvm;
use crate::ZerovisorError;

#[derive(Clone, Copy)]
struct VmHandleWrapper(Option<zerovisor_hal::virtualization::VmHandle>);

/// Simplified Container ID type
pub type ContainerId = String;
/// Simplified Pod (sandbox) ID type
pub type PodId = String;

/// PodSandbox configuration (subset of CRI)
#[derive(Clone)]
pub struct PodConfig { pub name: String, pub namespace: String }

/// Container configuration (subset)
#[derive(Clone)]
pub struct ContainerConfig {
    pub name: String,
    pub image: String,
    pub cmd: Vec<String>,
    /// CPU quota in milli-cores (1000 = full physical CPU).
    pub cpu_limit_millis: u32,
    /// Memory limit in bytes.
    pub mem_limit_bytes: u64,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self { name: String::new(), image: String::new(), cmd: Vec::new(), cpu_limit_millis: 1000, mem_limit_bytes: 256 * 1024 * 1024 }
    }
}

/// Container statistics (simplified CRI).
#[derive(Clone, Debug)]
pub struct ContainerStats { pub cpu_usage_ns: u64, pub mem_usage_bytes: u64, pub uptime: Duration }

/// Container lifecycle states (subset of CRI spec).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerState { Created, Running, Stopped }

/// CRI error codes
#[derive(Debug)]
pub enum CriError { NotFound, AlreadyExists, RuntimeFailure }

/// Runtime internal state
struct RuntimeState {
    pods: BTreeMap<PodId, PodConfig>,
    containers: BTreeMap<ContainerId, (PodId, ContainerConfig, ContainerState, VmHandleWrapper, u64 /* start ts */)>,
}

/// Main runtime object (singleton)
pub struct KubeRuntime { state: Mutex<RuntimeState> }

impl KubeRuntime {
    pub const fn new() -> Self { Self { state: Mutex::new(RuntimeState { pods: BTreeMap::new(), containers: BTreeMap::new() }) } }

    pub fn create_pod(&self, cfg: PodConfig) -> Result<PodId, CriError> {
        let id = format!("{}-{}", cfg.namespace, cfg.name);
        let mut st = self.state.lock();
        if st.pods.contains_key(&id) { return Err(CriError::AlreadyExists); }
        st.pods.insert(id.clone(), cfg);
        Ok(id)
    }

    pub fn remove_pod(&self, id: &PodId) -> Result<(), CriError> {
        let mut st = self.state.lock();
        st.pods.remove(id).ok_or(CriError::NotFound)?;
        // Remove all containers belonging to pod
        st.containers.retain(|_, (p, _, _, _)| p != id);
        Ok(())
    }

    pub fn create_container(&self, pod: &PodId, cfg: ContainerConfig) -> Result<ContainerId, CriError> {
        let mut st = self.state.lock();
        if !st.pods.contains_key(pod) { return Err(CriError::NotFound); }
        let cid = generate_container_id(pod, &cfg.name);
        st.containers.insert(cid.clone(), (pod.clone(), cfg, ContainerState::Created, VmHandleWrapper(None), 0));
        Ok(cid)
    }

    pub fn start_container(&self, id: &ContainerId) -> Result<(), CriError> {
        let mut st = self.state.lock();
        let entry = st.containers.get_mut(id).ok_or(CriError::NotFound)?;
        if entry.2 == ContainerState::Running { return Ok(()); }
        let start = crate::timer::current_time_ns();
        match microvm::create_fast_micro_vm() {
            Ok(vm) => {
                let end = crate::timer::current_time_ns();
                let latency = end - start;
                crate::monitor::record_cold_start(latency);
                entry.2 = ContainerState::Running;
                entry.3 = VmHandleWrapper(Some(vm));
                entry.4 = end;
                Ok(())
            }
            Err(_) => Err(CriError::RuntimeFailure)
        }
    }

    pub fn stop_container(&self, id: &ContainerId) -> Result<(), CriError> {
        let mut st = self.state.lock();
        let entry = st.containers.get_mut(id).ok_or(CriError::NotFound)?;
        if entry.2 == ContainerState::Stopped { return Ok(()); }
        if let Some(vm) = entry.3.0 {
            if microvm::shutdown_micro_vm(vm).is_err() {
                return Err(CriError::RuntimeFailure);
            }
        }
        entry.2 = ContainerState::Stopped;
        Ok(())
    }

    pub fn remove_container(&self, id: &ContainerId) -> Result<(), CriError> {
        let mut st = self.state.lock();
        let entry = st.containers.remove(id).ok_or(CriError::NotFound)?;
        if let Some(vm) = entry.3.0 {
            let _ = microvm::shutdown_micro_vm(vm);
        }
        Ok(())
    }

    pub fn container_status(&self, id: &ContainerId) -> Result<ContainerState, CriError> {
        let st = self.state.lock();
        Ok(st.containers.get(id).ok_or(CriError::NotFound)?.2)
    }

    /// Obtain container resource usage stats (mock implementation).
    pub fn container_stats(&self, id: &ContainerId) -> Result<ContainerStats, CriError> {
        let st = self.state.lock();
        let (_, cfg, state, _, start_ns) = st.containers.get(id).ok_or(CriError::NotFound)?;
        if *state != ContainerState::Running { return Err(CriError::RuntimeFailure); }
        let uptime = Duration::from_nanos(crate::timer::current_time_ns() - *start_ns);
        // Placeholder: CPU usage proportional to uptime * cpu_limit.
        let cpu_usage = uptime.as_nanos() as u64 * (cfg.cpu_limit_millis as u64) / 1000;
        Ok(ContainerStats { cpu_usage_ns: cpu_usage, mem_usage_bytes: cfg.mem_limit_bytes / 2, uptime })
    }

    /// Retrieve last N log lines (stored in-memory circular buffer; stubbed).
    pub fn container_logs(&self, _id: &ContainerId, _last_n: usize) -> Result<Vec<String>, CriError> {
        Ok(Vec::new())
    }

    pub fn list_pods(&self) -> Vec<PodId> {
        self.state.lock().pods.keys().cloned().collect()
    }

    pub fn list_containers(&self, pod: Option<&PodId>) -> Vec<ContainerId> {
        let st = self.state.lock();
        st.containers.iter()
            .filter(|(_, (p, _, _, _))| pod.map_or(true, |target| p == target))
            .map(|(cid, _)| cid.clone())
            .collect()
    }
}

static RUNTIME: Mutex<Option<KubeRuntime>> = Mutex::new(None);

pub fn global() -> &'static KubeRuntime {
    if RUNTIME.lock().is_none() { *RUNTIME.lock() = Some(KubeRuntime::new()); }
    // SAFETY: we just ensured it's Some
    unsafe { &*(&RUNTIME.lock().as_ref().unwrap() as *const KubeRuntime) }
}

// -------------------------------------------------------------------------------------------------
// Helper for unique container identifiers
// -------------------------------------------------------------------------------------------------

static NEXT_CONTAINER_ID: AtomicU32 = AtomicU32::new(1);

fn generate_container_id(pod: &str, name: &str) -> String {
    let seq = NEXT_CONTAINER_ID.fetch_add(1, Ordering::SeqCst);
    format!("{}-{}-{}", pod, name, seq)
} 