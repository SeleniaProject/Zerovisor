//! WebAssembly runtime integration stub (Task 11.2)
//! Uses `wasmi` interpreter in no_std mode for sandbox execution.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;
use wasmi::{Engine, Store, Module, Instance, Value, Linker, Config, Caller, Memory, MemoryType, Limits, Trap};
use core::fmt::Debug;

#[derive(Debug)]
pub enum WasmError {
    Compile,
    Instantiate,
    Runtime,
    Validation,
    OutOfFuel,
    MemoryLimitExceeded,
}

/// Runtime configuration for a sandbox instance.
#[derive(Debug, Clone, Copy)]
pub struct WasmVmConfig {
    /// Maximum linear memory size in bytes.
    pub memory_limit: u64,
    /// Initial fuel allotment (roughly instructions executed).
    pub fuel_limit: u64,
}

impl Default for WasmVmConfig {
    fn default() -> Self {
        Self { memory_limit: 32 * 1024 * 1024, /* 32 MiB */ fuel_limit: 5_000_000 }
    }
}

/// Validate a guest module to reject dangerous constructs.
fn validate_module(engine: &Engine, bytes: &[u8]) -> Result<(), WasmError> {
    Module::validate(engine, bytes).map_err(|_| WasmError::Validation)
}

/// Very small subset of WASI: proc_exit & fd_write to a hypervisor log sink.
fn populate_minimal_wasi(linker: &mut Linker<()>) -> Result<(), WasmError> {
    // proc_exit simply traps so the hypervisor can reclaim the VM.
    linker.func_wrap("wasi_snapshot_preview1", "proc_exit", |status: u32| {
        let msg = match status {
            0 => "guest exit with status 0",
            _ => "guest exit with error",
        };
        crate::log!("WASM proc_exit: {}", msg);
        Err(Trap::i32_exit(status as i32))
    }).map_err(|_| WasmError::Runtime)?;

    // fd_write just discards bytes after copying from guest memory.
    linker.func_wrap(
        "wasi_snapshot_preview1",
        "fd_write",
        |mut caller: Caller<'_, ()>, _fd: u32, iovs_ptr: u32, iovs_len: u32, nwritten_ptr: u32| -> Result<u32, Trap> {
            // Very naive: assume one memory, linear.
            let memory = caller
                .get_export("memory")
                .and_then(|e| e.into_memory())
                .ok_or_else(|| Trap::new("no memory"))?;

            // Copy data out (no fault detection):
            let mut total = 0u32;
            for i in 0..iovs_len {
                let base_ptr = iovs_ptr + i * 8; // struct {offset:u32, len:u32}
                let offset: u32 = memory.read(&caller, base_ptr as usize).map_err(|_| Trap::new("read"))?;
                let len: u32 = memory.read(&caller, (base_ptr + 4) as usize).map_err(|_| Trap::new("read"))?;
                total = total.saturating_add(len);
            }
            // write nwritten
            memory.write(&mut caller, nwritten_ptr as usize, &total.to_le_bytes()).map_err(|_| Trap::new("write"))?;
            Ok(0) // ERRNO_SUCCESS
        },
    ).map_err(|_| WasmError::Runtime)?;

    Ok(())
}

/// Simple sandbox instance wrapper
pub struct WasmSandbox {
    engine: Engine,
    store: Store<()>,
    instance: Instance,
    cfg: WasmVmConfig,
}

impl WasmSandbox {
    pub fn new(wasm_bytes: &[u8], cfg: WasmVmConfig) -> Result<Self, WasmError> {
        // Configure engine with fuel consumption enabled.
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);

        validate_module(&engine, wasm_bytes)?;

        let mut store = Store::new(&engine, ());
        store.add_fuel(cfg.fuel_limit).map_err(|_| WasmError::Runtime)?;

        let module = Module::new(&engine, wasm_bytes).map_err(|_| WasmError::Compile)?;
        let mut linker = Linker::new(&engine);
        populate_minimal_wasi(&mut linker)?;

        let instance = linker
            .instantiate(&mut store, &module)
            .and_then(|prep| prep.start(&mut store))
            .map_err(|_| WasmError::Instantiate)?;

        // Enforce memory limit (assume single memory export named "memory").
        if let Some(mem) = instance.get_export(&mut store, "memory").and_then(|e| e.into_memory()) {
            let current = mem.ty(&store).initial();
            let max_pages = ((cfg.memory_limit + 0xFFFF) / 0x10000) as u32; // 64KiB pages
            if current > max_pages {
                return Err(WasmError::MemoryLimitExceeded);
            }
            if mem.grow(&mut store, 0).is_err() {
                // ensure memory exists
            }
        }

        Ok(Self { engine, store, instance, cfg })
    }

    /// Call exported function with no params/returns
    pub fn call_void(&mut self, name: &str) -> Result<(), WasmError> {
        let func = self.instance.get_func(&mut self.store, name).ok_or(WasmError::Runtime)?;
        func.call(&mut self.store, &[], &mut []).map_err(|e| {
            if e.to_string().contains("all fuel consumed") {
                WasmError::OutOfFuel
            } else {
                WasmError::Runtime
            }
        })
    }

    /// Return remaining fuel for monitoring.
    pub fn remaining_fuel(&mut self) -> Result<u64, WasmError> {
        self.store.fuel_consumed().map(|c| self.cfg.fuel_limit - c).map_err(|_| WasmError::Runtime)
    }
} 