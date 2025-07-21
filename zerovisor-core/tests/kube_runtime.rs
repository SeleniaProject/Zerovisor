//! Kubernetes CRI runtime unit tests

extern crate std;
use zerovisor_core::kube_runtime::{handle_run_pod, PodConfig, register_vm_ops, VmOps};
use std::collections::BTreeMap;
use zerovisor_hal::virtualization::{VmConfig, VmHandle};

struct DummyOps;
impl VmOps for DummyOps {
    fn create_vm(&self, _cfg: &VmConfig) -> Result<VmHandle, zerovisor_core::ZerovisorError> { Ok(42) }
    fn start_vm(&self, _h: VmHandle) -> Result<(), zerovisor_core::ZerovisorError> { Ok(()) }
    fn stop_vm(&self, _h: VmHandle) -> Result<(), zerovisor_core::ZerovisorError> { Ok(()) }
}

#[test]
fn run_pod_succeeds() {
    static OPS: DummyOps = DummyOps;
    register_vm_ops(&OPS);
    let cfg = PodConfig {
        uid: "pod123".into(),
        name: "nginx".into(),
        namespace: "default".into(),
        annotations: BTreeMap::new(),
        image: "nginx:latest".into(),
        cpu_millis: 500,
        mem_bytes: 64 * 1024 * 1024,
    };
    handle_run_pod(cfg).expect("run pod");
} 