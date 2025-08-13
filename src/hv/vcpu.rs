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
    pub fn start(&mut self) { self.state = VcpuState::Running; }
    pub fn stop(&mut self) { self.state = VcpuState::Stopped; }
}


