//! FeatureRegistry – runtime toggleable feature activation (Task)
//! Allows registering hypervisor features that can be enabled or disabled at runtime.

#![allow(dead_code)]

extern crate alloc;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use spin::Mutex;

pub trait Feature: Send + Sync {
    /// Activate feature (idempotent)
    fn enable(&self) -> Result<(), FeatureError>;
    /// Deactivate feature (idempotent)
    fn disable(&self) -> Result<(), FeatureError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureError { NotFound, AlreadyExists, Internal }

pub struct FeatureRegistry {
    features: Mutex<BTreeMap<&'static str, Box<dyn Feature>>>,
}

impl FeatureRegistry {
    pub const fn new() -> Self { Self { features: Mutex::new(BTreeMap::new()) } }

    pub fn register_feature(&self, name: &'static str, feature: Box<dyn Feature>) -> Result<(), FeatureError> {
        let mut map = self.features.lock();
        if map.contains_key(name) { return Err(FeatureError::AlreadyExists); }
        map.insert(name, feature).ok();
        Ok(())
    }

    pub fn enable_feature(&self, name: &str) -> Result<(), FeatureError> {
        let map = self.features.lock();
        map.get(name).ok_or(FeatureError::NotFound)?.enable()
    }

    pub fn disable_feature(&self, name: &str) -> Result<(), FeatureError> {
        let map = self.features.lock();
        map.get(name).ok_or(FeatureError::NotFound)?.disable()
    }
}

static REGISTRY: FeatureRegistry = FeatureRegistry::new();
pub fn global() -> &'static FeatureRegistry { &REGISTRY } 