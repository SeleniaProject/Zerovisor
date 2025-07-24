use std::net::SocketAddr;
use tonic::{transport::Server, Request, Response, Status};
use tonic::codegen::Arc;
use zvi_sdk::Client;

// Generated proto code
mod runtime {
    tonic::include_proto!("runtime.v1alpha2");
}

use runtime::runtime_service_server::{RuntimeService, RuntimeServiceServer};
use runtime::*;

#[derive(Clone)]
struct CriService {
    hv_client: Arc<Client>,
}

#[tonic::async_trait]
impl RuntimeService for CriService {
    async fn create_pod_sandbox(&self, request: Request<CreatePodSandboxRequest>) -> Result<Response<CreatePodSandboxResponse>, Status> {
        let meta = request.into_inner().metadata.unwrap_or_default();
        // For demo, map pod UID to VM id (hash modulo u32::MAX).
        let uid_hash = crc32fast::hash(meta.uid.as_bytes());
        // Start VM via Hypervisor management API.
        self.hv_client.start_vm(uid_hash).await.map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(CreatePodSandboxResponse { pod_sandbox_id: format!("{}", uid_hash) }))
    }

    async fn stop_pod_sandbox(&self, request: Request<StopPodSandboxRequest>) -> Result<Response<StopPodSandboxResponse>, Status> {
        let id: u32 = request.into_inner().pod_sandbox_id.parse().map_err(|_| Status::invalid_argument("invalid id"))?;
        self.hv_client.stop_vm(id).await.map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(StopPodSandboxResponse {}))
    }

    async fn remove_pod_sandbox(&self, request: Request<RemovePodSandboxRequest>) -> Result<Response<RemovePodSandboxResponse>, Status> {
        let id: u32 = request.into_inner().pod_sandbox_id.parse().map_err(|_| Status::invalid_argument("invalid id"))?;
        // Hypervisor API reuse stop then nothing else; treat as success.
        self.hv_client.stop_vm(id).await.ok();
        Ok(Response::new(RemovePodSandboxResponse {}))
    }

    async fn create_container(&self, request: Request<CreateContainerRequest>) -> Result<Response<CreateContainerResponse>, Status> {
        let req = request.into_inner();
        // For micro-VM world each pod == VM, so container id == pod id + "-0"
        Ok(Response::new(CreateContainerResponse { container_id: format!("{}-0", req.pod_sandbox_id) }))
    }

    async fn start_container(&self, _request: Request<StartContainerRequest>) -> Result<Response<StartContainerResponse>, Status> {
        // Container already running inside micro-VM.
        Ok(Response::new(StartContainerResponse {}))
    }

    async fn stop_container(&self, _request: Request<StopContainerRequest>) -> Result<Response<StopContainerResponse>, Status> {
        // Not implemented per-single container.
        Ok(Response::new(StopContainerResponse {}))
    }

    async fn remove_container(&self, _request: Request<RemoveContainerRequest>) -> Result<Response<RemoveContainerResponse>, Status> {
        Ok(Response::new(RemoveContainerResponse {}))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = "0.0.0.0:50051".parse()?;
    println!("CRI gRPC server listening on {}", addr);

    let hv_client = Arc::new(Client::new("http://127.0.0.1:8080"));
    let svc = CriService { hv_client };

    Server::builder()
        .add_service(RuntimeServiceServer::new(svc))
        .serve(addr)
        .await?;
    Ok(())
} 