#!/bin/bash
set -e  # Exit on any error

# Validate hpatchz exists before building
if [ ! -f "./sidecar/hpatchz/hpatchz" ]; then
    echo "[ERROR] hpatchz not found at ./sidecar/hpatchz/hpatchz"
    echo "[ERROR] Cannot build Sophon without hpatchz dependency"
    exit 1
fi

# Download protoc (needed by prost-build at Rust compile time)
echo "[+] Downloading protoc..."
curl -sSL https://github.com/protocolbuffers/protobuf/releases/download/v31.1/protoc-31.1-osx-universal_binary.zip > protobuf.zip
unzip -o -j protobuf.zip bin/protoc -d bin
rm protobuf.zip
chmod +x bin/protoc

# Ensure Rust toolchain is available
if ! command -v cargo &>/dev/null; then
    echo "[+] Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    source "$HOME/.cargo/env"
fi

echo "[+] Building Rust sophon-server..."
pushd sophon-server-rs
PROTOC="$(pwd)/../bin/protoc" cargo build --release
popd

echo "[+] Copying binary to dist..."
mkdir -p sophon_server/dist
cp sophon-server-rs/target/release/sophon-server sophon_server/dist/sophon-server
chmod +x sophon_server/dist/sophon-server

echo "[+] Build complete!"
ls -lh ./sophon_server/dist/sophon-server
