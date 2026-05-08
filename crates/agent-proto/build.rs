// Generate Rust bindings from `proto/agent.proto` at build time.
// `tonic-build` emits the gRPC service trait + the prost-derived
// message types. Output is gathered under `OUT_DIR` and re-exported
// by `src/lib.rs`.

fn main() {
    let proto = "proto/agent.proto";
    println!("cargo:rerun-if-changed={proto}");
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&[proto], &["proto"])
        .expect("compile agent.proto");
}
