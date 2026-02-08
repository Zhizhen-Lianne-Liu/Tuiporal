use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = PathBuf::from("proto/temporal-api");
    let proto_file = proto_root.join("temporal/api/workflowservice/v1/service.proto");

    // Configure tonic-build
    tonic_build::configure()
        .build_server(false) // We only need client code
        .build_client(true)
        .out_dir("src/generated") // Output generated code to src/generated
        .compile_protos(
            &[proto_file],
            &[proto_root], // Include path for imports
        )?;

    // Tell Cargo to rerun this build script if proto files change
    println!("cargo:rerun-if-changed=proto/");

    Ok(())
}
