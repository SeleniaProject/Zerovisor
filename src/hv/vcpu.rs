#![allow(dead_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcpuState { Created, Running, Stopped }

#[derive(Debug)]
pub struct Vcpu {
    pub id: u32,
    pub state: VcpuState,
}

impl Vcpu {
    pub fn new(id: u32) -> Self { Self { id, state: VcpuState::Created } }
    pub fn start(&mut self) {
        self.state = VcpuState::Running;
        crate::obs::metrics::VCPU_STARTED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        crate::obs::trace::emit(crate::obs::trace::Event::VmStart(self.id as u64));
    }
    pub fn stop(&mut self) {
        self.state = VcpuState::Stopped;
        crate::obs::metrics::VCPU_STOPPED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        crate::obs::trace::emit(crate::obs::trace::Event::VmStop(self.id as u64));
    }
}


