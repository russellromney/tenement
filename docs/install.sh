#!/bin/bash
# tenement installer
# Usage: curl -LsSf https://tenement.dev/install.sh | sh

set -e

REPO="russellromney/tenement"
INSTALL_DIR="${TENEMENT_INSTALL_DIR:-$HOME/.tenement}"
BIN_DIR="${TENEMENT_BIN_DIR:-$INSTALL_DIR/bin}"

# Detect platform
detect_platform() {
    local os arch

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)  os="linux" ;;
        Darwin) os="macos" ;;
        *)      echo "Unsupported OS: $os" && exit 1 ;;
    esac

    case "$arch" in
        x86_64)  arch="x86_64" ;;
        aarch64) arch="aarch64" ;;
        arm64)   arch="aarch64" ;;
        *)       echo "Unsupported architecture: $arch" && exit 1 ;;
    esac

    echo "${os}-${arch}"
}

# Get latest release version
get_latest_version() {
    curl -sL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | \
        grep '"tag_name":' | \
        sed -E 's/.*"([^"]+)".*/\1/' || echo ""
}

# Try to install from GitHub releases
install_from_release() {
    local platform version download_url archive

    platform="$(detect_platform)"
    version="$(get_latest_version)"

    if [ -z "$version" ]; then
        return 1
    fi

    echo "  Platform: $platform"
    echo "  Version:  $version"

    # Create directories
    mkdir -p "$BIN_DIR"

    # Download binary
    archive="tenement-${platform}.tar.gz"
    download_url="https://github.com/${REPO}/releases/download/${version}/${archive}"

    echo "  Downloading from: $download_url"

    if curl -LsSf "$download_url" 2>/dev/null | tar xz -C "$BIN_DIR" 2>/dev/null; then
        chmod +x "$BIN_DIR/tenement"
        return 0
    else
        return 1
    fi
}

# Install via cargo
install_from_cargo() {
    if ! command -v cargo &> /dev/null; then
        echo "Error: cargo not found. Install Rust from https://rustup.rs"
        exit 1
    fi

    echo "  Building from source with cargo..."
    cargo install tenement --bin tenement
}

main() {
    echo "Installing tenement..."
    echo ""

    # Try GitHub release first, fall back to cargo
    if install_from_release; then
        echo ""
        echo "tenement installed to: $BIN_DIR/tenement"
        echo ""

        # Check if bin dir is in PATH
        case ":$PATH:" in
            *":$BIN_DIR:"*)
                echo "Run 'tenement --help' to get started!"
                ;;
            *)
                echo "Add the following to your shell config (.bashrc, .zshrc, etc.):"
                echo ""
                echo "  export PATH=\"$BIN_DIR:\$PATH\""
                echo ""
                echo "Then run 'tenement --help' to get started!"
                ;;
        esac
    else
        echo "  No prebuilt binary found, building from source..."
        echo ""
        install_from_cargo
        echo ""
        echo "tenement installed! Run 'tenement --help' to get started."
    fi
}

main
