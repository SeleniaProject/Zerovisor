//! zvi-cli: Command-line client for Zerovisor (Task 16.2)

use clap::{Parser, Subcommand};
use anyhow::Result;
use zvi_sdk::Client;

#[derive(Parser)]
#[command(author, version, about = "Zerovisor CLI tool")]
struct Cli {
    /// Management API endpoint (default: http://127.0.0.1:8080)
    #[arg(short, long, default_value_t = String::from("http://127.0.0.1:8080"))]
    endpoint: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List virtual machines
    List,
    /// Start VM
    Start { id: u32 },
    /// Stop VM
    Stop { id: u32 },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = Client::new(cli.endpoint);
    match cli.command {
        Commands::List => {
            let vms = client.list_vms().await?;
            for vm in vms {
                println!("{:<4} {:<16} {:<8} vCPUs:{} Mem:{}MB", vm.id, vm.name, vm.state, vm.vcpus, vm.memory/1024/1024);
            }
        }
        Commands::Start { id } => {
            client.start_vm(id).await?;
            println!("VM {} started", id);
        }
        Commands::Stop { id } => {
            client.stop_vm(id).await?;
            println!("VM {} stopped", id);
        }
    }
    Ok(())
} 