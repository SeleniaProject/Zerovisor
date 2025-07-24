use std::io::Result;

fn main() -> Result<()> {
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .compile(&["proto/runtime.proto"], &["proto"])?;
    println!("cargo:rerun-if-changed=proto/runtime.proto");
    Ok(())
} 