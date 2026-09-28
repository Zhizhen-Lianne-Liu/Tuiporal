use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = PathBuf::from("proto/temporal-api");
    let proto_file = proto_root.join("temporal/api/workflowservice/v1/service.proto");

    if !proto_file.exists() {
        return Err("Temporal API protos missing; run `git submodule update --init`".into());
    }

    // Configure tonic-build
    tonic_build::configure()
        .build_server(false) // We only need client code
        .build_client(true)
        .include_file("temporal.rs") // Generate the nested package module tree in OUT_DIR
        .compile_protos(
            &[proto_file],
            &[proto_root], // Include path for imports
        )?;

    // Tell Cargo to rerun this build script if proto files change
    println!("cargo:rerun-if-changed=proto/");

    Ok(())
}
