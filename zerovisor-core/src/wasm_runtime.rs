//! WebAssembly runtime integration stub (Task 11.2)
//! Uses `wasmi` interpreter in no_std mode for sandbox execution.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;
use wasmi::{Engine, Store, Module, Instance, Value, Linker};
use core::fmt::Debug;

#[derive(Debug)]
pub enum WasmError { Compile, Instantiate, Runtime }

/// Simple sandbox instance wrapper
pub struct WasmSandbox {
    engine: Engine,
    store: Store<()>,
    instance: Instance,
}

impl WasmSandbox {
    pub fn new(wasm_bytes: &[u8]) -> Result<Self, WasmError> {
        let engine = Engine::default();
        let mut store = Store::new(&engine, ());
        let module = Module::new(&engine, wasm_bytes).map_err(|_| WasmError::Compile)?;
        let linker = Linker::new(&engine);
        let instance = linker.instantiate(&mut store, &module).and_then(|prep| prep.start(&mut store)).map_err(|_| WasmError::Instantiate)?;
        Ok(Self { engine, store, instance })
    }

    /// Call exported function with no params/returns
    pub fn call_void(&mut self, name: &str) -> Result<(), WasmError> {
        let func = self.instance.get_func(&mut self.store, name).ok_or(WasmError::Runtime)?;
        func.call(&mut self.store, &[], &mut []).map_err(|_| WasmError::Runtime)
    }
} 