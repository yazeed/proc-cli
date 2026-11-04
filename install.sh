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

# The latest release's tag, read from where github.com/<repo>/releases/latest
# redirects (.../releases/tag/v2.3.0). The API's limit for requests without a
# token, 60 an hour per address, made the installer fail on shared networks
get_latest_version() {
    latest=$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
        "https://github.com/${REPO}/releases/latest") || return 0
    tag=${latest##*/}
    case "$tag" in
        v[0-9]*) echo "$tag" ;;
    esac
}

# A file's SHA-256, with the tool the system has: sha256sum (Linux, Git Bash
# on Windows), shasum (macOS), sha256 (FreeBSD and NetBSD, in the base
# system) or openssl. Prints nothing when there is none
sha256_of() {
    if command -v sha256sum > /dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif command -v shasum > /dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    elif command -v sha256 > /dev/null 2>&1; then
        sha256 -q "$1"
    elif command -v openssl > /dev/null 2>&1; then
        openssl dgst -sha256 -r "$1" | cut -d ' ' -f 1
    fi
}

# Check the downloaded file against the release's checksums.txt, so a
# corrupted or altered download is never installed
verify_checksum() {
    file="$1"
    asset="$2"
    sums="${tmp_dir}/checksums.txt"
    if ! curl -fsSL "https://github.com/${REPO}/releases/download/${version}/checksums.txt" -o "$sums"; then
        error "Failed to download checksums.txt for ${version}"
    fi
    want=$(awk -v asset="$asset" '$2 == asset { print $1 }' "$sums")
    [ -n "$want" ] || error "checksums.txt for ${version} has no entry for ${asset}"
    got=$(sha256_of "$file")
    [ -n "$got" ] || error "No SHA-256 tool found (sha256sum, shasum, sha256 or openssl) to verify the download"
    if [ "$got" != "$want" ]; then
        error "Checksum mismatch for ${asset}: expected ${want}, got ${got}. Nothing was installed"
    fi
    info "Checksum verified (SHA-256)"
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
        asset="proc-${platform}.exe"
    else
        asset="proc-${platform}"
    fi
    url="https://github.com/${REPO}/releases/download/${version}/${asset}"

    info "Downloading from: ${url}"

    tmp_dir=$(mktemp -d)
    trap 'rm -rf "$tmp_dir"' EXIT

    if ! curl -fsSL "${url}" -o "${tmp_dir}/${BINARY_NAME}"; then
        error "Failed to download proc"
    fi

    verify_checksum "${tmp_dir}/${BINARY_NAME}" "$asset"

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
