fn main() {
    tonic_build::compile_protos("proto/colink.proto")
        .unwrap_or_else(|e| panic!("Failed to compile protos {:?}", e));
    prost_build::compile_protos(&["proto/colink_registry.proto"], &["proto/"])
        .unwrap_or_else(|e| panic!("Failed to compile protos {:?}", e));
}
