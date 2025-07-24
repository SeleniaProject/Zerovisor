//! Zerovisor SDK library (Task 16.2)
//! Provides convenient Rust API for interacting with Zerovisor management
//! endpoint over HTTP+JSON.

use anyhow::Result;
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct VmInfo {
    pub id: u32,
    pub name: String,
    pub state: String,
    pub vcpus: u32,
    pub memory: u64,
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
}

impl Client {
    /// Create new client pointing at management API (e.g., http://127.0.0.1:8080)
    pub fn new(base: impl Into<String>) -> Self {
        Self { http: reqwest::Client::new(), base: base.into() }
    }

    /// List all VMs on target hypervisor.
    pub async fn list_vms(&self) -> Result<Vec<VmInfo>> {
        let res = self.http.get(format!("{}/v1/vms", self.base)).send().await?;
        let vms = res.json::<Vec<VmInfo>>().await?;
        Ok(vms)
    }

    /// Start a VM by id.
    pub async fn start_vm(&self, id: u32) -> Result<()> {
        self.http.post(format!("{}/v1/vms/{}/start", self.base, id)).send().await?.error_for_status()?;
        Ok(())
    }

    /// Stop a VM by id.
    pub async fn stop_vm(&self, id: u32) -> Result<()> {
        self.http.post(format!("{}/v1/vms/{}/stop", self.base, id)).send().await?.error_for_status()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn client_base_url(url in r"http://[a-z]{1,8}\.local(:[0-9]{1,5})?") {
            let c = Client::new(url.clone());
            prop_assert!(c.base.starts_with("http"));
        }
    }
} 