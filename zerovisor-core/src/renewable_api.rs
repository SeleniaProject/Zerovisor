//! Renewable Energy Coordination API
//!
//! Each node reports the percentage of its power that comes from renewable
//! sources (0–100%). This value is broadcast cluster-wide (ClusterMsg::Custom
//! discriminator 0xCB) so that scheduling and power-aware modules can prefer
//! greener nodes when carbon intensity is tied.
//!
//! All comments in English.

#![allow(dead_code)]

extern crate alloc;
use core::sync::atomic::{AtomicU8, Ordering};
use spin::Once;
use crate::cluster::{ClusterManager, NodeId};

static PERCENT_RENEWABLE: AtomicU8 = AtomicU8::new(0);
static INIT: Once<()> = Once::new();

pub fn init() { INIT.call_once(|| {}); }

/// Update local renewable percentage and broadcast to peers.
pub fn update_local(percent: u8) {
    PERCENT_RENEWABLE.store(percent, Ordering::Relaxed);
    let payload = [percent];
    let msg = crate::fault::Msg::Custom(0xCB, &payload);
    ClusterManager::global().broadcast(&msg);
}

/// Handle cluster message.
pub fn on_msg(src: NodeId, percent: u8) {
    // For now store only local node; extension could track per-node map.
    if src == ClusterManager::global().leader().unwrap_or(NodeId(0)) {
        PERCENT_RENEWABLE.store(percent, Ordering::Relaxed);
    }
}

/// Current local renewable percentage.
pub fn local_percent() -> u8 { PERCENT_RENEWABLE.load(Ordering::Relaxed) } 