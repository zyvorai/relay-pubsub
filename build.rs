fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    let include = protoc_bin_vendored::include_path()?;
    std::env::set_var("PROTOC", protoc);

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto/google/pubsub/v1/pubsub.proto"], &["proto", include.to_str().unwrap()])?;

    println!("cargo:rerun-if-changed=proto/google/pubsub/v1/pubsub.proto");
    Ok(())
}
