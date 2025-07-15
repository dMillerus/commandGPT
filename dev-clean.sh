#!/bin/bash

# CommandGPT Development Clean Script
# Removes old installations and builds fresh version for testing

set -e  # Exit on any error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_step() {
    echo -e "${BLUE}🔹 $1${NC}"
}

print_success() {
    echo -e "${GREEN}✅ $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠️  $1${NC}"
}

print_error() {
    echo -e "${RED}❌ $1${NC}"
}

print_header() {
    echo -e "${BLUE}"
    echo "=================================================="
    echo "  CommandGPT Development Clean Script"
    echo "=================================================="
    echo -e "${NC}"
}

# Parse command line arguments
KEEP_CONFIG=false
SKIP_BUILD=false
VERBOSE=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --keep-config)
            KEEP_CONFIG=true
            shift
            ;;
        --skip-build)
            SKIP_BUILD=true
            shift
            ;;
        --verbose)
            VERBOSE=true
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --keep-config    Don't remove ~/.commandgpt configuration"
            echo "  --skip-build     Only clean, don't rebuild"
            echo "  --verbose        Show detailed output"
            echo "  -h, --help       Show this help message"
            echo ""
            echo "This script will:"
            echo "  1. Clean all build artifacts"
            echo "  2. Remove installed binaries"
            echo "  3. Optionally backup and remove config"
            echo "  4. Build fresh version from source"
            echo "  5. Install and verify new version"
            exit 0
            ;;
        *)
            print_error "Unknown option: $1"
            echo "Use --help for usage information"
            exit 1
            ;;
    esac
done

print_header

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ] || [ ! -f "Makefile" ]; then
    print_error "This script must be run from the CommandGPT project root directory"
    print_error "Looking for Cargo.toml and Makefile..."
    exit 1
fi

print_success "Found Cargo.toml and Makefile - proceeding with clean"

# Step 1: Clean build artifacts
print_step "Cleaning build artifacts..."

if $VERBOSE; then
    echo "Running: cargo clean"
fi
cargo clean

# Remove any universal binaries
if [ -f "target/release/commandgpt-universal" ]; then
    rm -f target/release/commandgpt-universal
    print_success "Removed universal binary"
fi

# Remove target directory completely for thorough clean
if [ -d "target" ]; then
    rm -rf target/
    print_success "Removed entire target directory"
fi

# Step 2: Remove installed binaries
print_step "Removing installed binaries..."

# Check common installation locations
REMOVED_COUNT=0

# /usr/local/bin (most common)
if [ -f "/usr/local/bin/commandgpt" ]; then
    if $VERBOSE; then
        echo "Removing: /usr/local/bin/commandgpt"
    fi
    sudo rm -f /usr/local/bin/commandgpt
    REMOVED_COUNT=$((REMOVED_COUNT + 1))
fi

# ~/.cargo/bin (cargo install location)
if [ -f "$HOME/.cargo/bin/commandgpt" ]; then
    if $VERBOSE; then
        echo "Removing: $HOME/.cargo/bin/commandgpt"
    fi
    rm -f "$HOME/.cargo/bin/commandgpt"
    REMOVED_COUNT=$((REMOVED_COUNT + 1))
fi

# /opt/homebrew/bin (Homebrew on Apple Silicon)
if [ -f "/opt/homebrew/bin/commandgpt" ]; then
    if $VERBOSE; then
        echo "Removing: /opt/homebrew/bin/commandgpt"
    fi
    sudo rm -f "/opt/homebrew/bin/commandgpt"
    REMOVED_COUNT=$((REMOVED_COUNT + 1))
fi

# /usr/bin (system location)
if [ -f "/usr/bin/commandgpt" ]; then
    if $VERBOSE; then
        echo "Removing: /usr/bin/commandgpt"
    fi
    sudo rm -f "/usr/bin/commandgpt"
    REMOVED_COUNT=$((REMOVED_COUNT + 1))
fi

if [ $REMOVED_COUNT -gt 0 ]; then
    print_success "Removed $REMOVED_COUNT old binary installation(s)"
else
    print_success "No old binaries found to remove"
fi

# Step 3: Handle configuration
if [ ! $KEEP_CONFIG = true ]; then
    print_step "Handling configuration directory..."
    
    if [ -d "$HOME/.commandgpt" ]; then
        # Create backup with timestamp
        BACKUP_DIR="$HOME/.commandgpt.backup.$(date +%Y%m%d_%H%M%S)"
        cp -r "$HOME/.commandgpt" "$BACKUP_DIR"
        print_success "Backed up config to: $BACKUP_DIR"
        
        # Remove current config
        rm -rf "$HOME/.commandgpt"
        print_success "Removed current configuration directory"
        
        print_warning "Configuration removed - you'll need to reconfigure your API key"
    else
        print_success "No configuration directory found"
    fi
else
    print_success "Keeping existing configuration (--keep-config specified)"
fi

# Step 4: Clear shell caches
print_step "Clearing shell caches..."

# Clear command hash table
hash -r 2>/dev/null || true
print_success "Cleared shell command cache"

# Step 5: Build fresh version (unless skipped)
if [ ! $SKIP_BUILD = true ]; then
    print_step "Building fresh version from source..."
    
    # Use Makefile for consistent build process
    if $VERBOSE; then
        echo "Running: make clean"
        make clean
        echo "Running: make release"
        make release
    else
        make clean > /dev/null 2>&1
        make release > /dev/null 2>&1
    fi
    
    print_success "Fresh build completed"
    
    # Step 6: Install new version
    print_step "Installing new version..."
    
    if $VERBOSE; then
        echo "Running: make install"
        make install
    else
        make install > /dev/null 2>&1
    fi
    
    print_success "Installation completed"
    
    # Step 7: Verify installation
    print_step "Verifying installation..."
    
    # Check if command is available
    if command -v commandgpt > /dev/null 2>&1; then
        INSTALL_PATH=$(which commandgpt)
        print_success "CommandGPT found at: $INSTALL_PATH"
        
        # Show file info
        if $VERBOSE; then
            ls -la "$INSTALL_PATH"
        fi
        
        # Test version
        VERSION_OUTPUT=$(commandgpt --version 2>/dev/null || echo "Unable to get version")
        print_success "Version: $VERSION_OUTPUT"
        
        # Show binary info with make info if available
        if $VERBOSE; then
            echo ""
            echo "Binary information:"
            make info 2>/dev/null || echo "Make info not available"
        fi
        
    else
        print_error "CommandGPT not found in PATH after installation"
        print_error "You may need to restart your shell or check your PATH"
        exit 1
    fi
else
    print_success "Skipping build and install (--skip-build specified)"
fi

# Final summary
echo ""
echo -e "${GREEN}=================================================="
echo "  Clean and rebuild completed successfully!"
echo "==================================================${NC}"
echo ""
echo "Summary of actions:"
echo "  ✅ Cleaned all build artifacts"
echo "  ✅ Removed old binary installations"
if [ ! $KEEP_CONFIG = true ]; then
    echo "  ✅ Backed up and removed configuration"
else
    echo "  ✅ Kept existing configuration"
fi
echo "  ✅ Cleared shell caches"
if [ ! $SKIP_BUILD = true ]; then
    echo "  ✅ Built fresh version from source"
    echo "  ✅ Installed new version"
    echo "  ✅ Verified installation"
fi
echo ""

if [ ! $KEEP_CONFIG = true ]; then
    print_warning "Remember to reconfigure your API key:"
    echo "  commandgpt config set-key"
fi

echo ""
print_success "You're now running a fresh build of CommandGPT!"
