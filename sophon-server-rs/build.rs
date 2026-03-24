fn main() {
    // If PROTOC env var is not set, try to find protoc in PATH.
    // The build-sophon.sh script sets PROTOC=../bin/protoc before running cargo.
    prost_build::compile_protos(
        &["proto/manifest.proto", "proto/manifest_ldiff.proto"],
        &["proto/"],
    )
    .expect("Failed to compile protobuf schemas. Make sure protoc is available (set PROTOC env var).");
}
