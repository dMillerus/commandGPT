#!/bin/bash
set -e

echo "🔨 Building commandGPT for Apple Silicon..."

# Clean previous builds
cargo clean

# Build for Apple Silicon with optimizations
cargo build --release --target aarch64-apple-darwin

echo "✅ Build complete!"
echo "📁 Binary location: target/aarch64-apple-darwin/release/commandgpt"
echo "📏 Binary size: $(du -h target/aarch64-apple-darwin/release/commandgpt | cut -f1)"

# Optional: Create universal binary if building on Intel Mac
if [[ $(uname -m) == "x86_64" ]]; then
    echo "🔄 Creating universal binary..."
    
    # Build for Intel as well
    cargo build --release --target x86_64-apple-darwin
    
    # Create universal binary
    lipo -create \
        target/x86_64-apple-darwin/release/commandgpt \
        target/aarch64-apple-darwin/release/commandgpt \
        -output target/release/commandgpt-universal
    
    echo "✅ Universal binary created: target/release/commandgpt-universal"
    echo "📏 Universal binary size: $(du -h target/release/commandgpt-universal | cut -f1)"
fi
