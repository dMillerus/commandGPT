.PHONY: build test clean install release help

# Default target
help:
	@echo "CommandGPT Build System"
	@echo ""
	@echo "Available targets:"
	@echo "  build     - Build debug version"
	@echo "  release   - Build optimized release version"
	@echo "  test      - Run tests"
	@echo "  clean     - Clean build artifacts"
	@echo "  install   - Install to /usr/local/bin"
	@echo "  check     - Run cargo check and clippy"
	@echo "  format    - Format code with rustfmt"

# Development build
build:
	cargo build

# Release build optimized for Apple Silicon
release:
	./build.sh

# Run tests
test:
	cargo test

# Clean build artifacts
clean:
	cargo clean
	rm -f target/release/commandgpt-universal

# Install to system
install: release
	@if [ -f target/aarch64-apple-darwin/release/commandgpt ]; then \
		echo "Installing commandgpt to /usr/local/bin..."; \
		sudo cp target/aarch64-apple-darwin/release/commandgpt /usr/local/bin/; \
		echo "✅ Installation complete!"; \
	else \
		echo "❌ Build first with 'make release'"; \
		exit 1; \
	fi

# Code quality checks
check:
	cargo check
	cargo clippy -- -D warnings

# Format code
format:
	cargo fmt

# Run in development mode
run:
	cargo run

# Show binary info
info:
	@if [ -f target/aarch64-apple-darwin/release/commandgpt ]; then \
		echo "📁 Binary: target/aarch64-apple-darwin/release/commandgpt"; \
		echo "📏 Size: $$(du -h target/aarch64-apple-darwin/release/commandgpt | cut -f1)"; \
		echo "🏗️  Type: $$(file target/aarch64-apple-darwin/release/commandgpt)"; \
	else \
		echo "❌ Binary not found. Run 'make release' first."; \
	fi
