#!/bin/sh
# proc installer script (POSIX sh: FreeBSD and NetBSD have no bash by default)
# Usage: curl -fsSL https://raw.githubusercontent.com/yazeed/proc-cli/main/install.sh | sh

set -e

REPO="yazeed/proc-cli"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
BINARY_NAME="proc"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

info() { printf '%b[INFO]%b %s\n' "$GREEN" "$NC" "$1"; }
warn() { printf '%b[WARN]%b %s\n' "$YELLOW" "$NC" "$1"; }
error() { printf '%b[ERROR]%b %s\n' "$RED" "$NC" "$1" >&2; exit 1; }

# Detect OS and architecture
detect_platform() {
    case "$(uname -s)" in
        Darwin) os="darwin" ;;
        Linux) os="linux" ;;
        FreeBSD) os="freebsd" ;;
        NetBSD) os="netbsd" ;;
        MINGW*|MSYS*|CYGWIN*) os="windows" ;;
        *) error "Unsupported operating system: $(uname -s)" ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64) arch="x86_64" ;;
        arm64|aarch64|evbarm) arch="aarch64" ;;
        *) error "Unsupported architecture: $(uname -m)" ;;
    esac

    # Releases have x86_64 binaries only for the BSDs
    case "${os}-${arch}" in
        freebsd-aarch64|netbsd-aarch64)
            error "No ${os} ARM64 binary in the releases; build it with: cargo install proc-cli" ;;
    esac

    echo "${os}-${arch}"
}

# Get latest release version
get_latest_version() {
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | \
        grep '"tag_name":' | \
        sed -E 's/.*"([^"]+)".*/\1/'
}

# Download and install
install_proc() {
    platform=$(detect_platform)
    info "Detected platform: ${platform}"

    version=$(get_latest_version)
    if [ -z "$version" ]; then
        error "Failed to get latest version"
    fi
    info "Latest version: ${version}"

    # Construct download URL
    if [ "$platform" = "windows-x86_64" ]; then
        url="https://github.com/${REPO}/releases/download/${version}/proc-${platform}.exe"
    else
        url="https://github.com/${REPO}/releases/download/${version}/proc-${platform}"
    fi

    info "Downloading from: ${url}"

    tmp_dir=$(mktemp -d)
    trap 'rm -rf "$tmp_dir"' EXIT

    if ! curl -fsSL "${url}" -o "${tmp_dir}/${BINARY_NAME}"; then
        error "Failed to download proc"
    fi

    chmod +x "${tmp_dir}/${BINARY_NAME}"

    # Install
    if [ -w "$INSTALL_DIR" ]; then
        mv "${tmp_dir}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
    else
        info "Installing to ${INSTALL_DIR} requires sudo..."
        sudo mv "${tmp_dir}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
    fi

    info "Successfully installed proc to ${INSTALL_DIR}/${BINARY_NAME}"

    # Verify installation
    if command -v proc > /dev/null 2>&1; then
        echo ""
        info "Installation complete!"
        proc --version
        echo ""
        info "Run 'proc --help' to get started"
    else
        warn "proc installed but not found in PATH"
        warn "Add ${INSTALL_DIR} to your PATH or run: ${INSTALL_DIR}/${BINARY_NAME}"
    fi
}

# Main
main() {
    echo ""
    echo "  ╔═══════════════════════════════════╗"
    echo "  ║      proc installer               ║"
    echo "  ║  Semantic Process Management CLI  ║"
    echo "  ╚═══════════════════════════════════╝"
    echo ""

    # Check for required tools
    command -v curl > /dev/null 2>&1 || error "curl is required but not installed"

    install_proc
}

main "$@"
