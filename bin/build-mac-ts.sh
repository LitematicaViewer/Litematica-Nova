#!/usr/bin/env bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
DESKTOP_DIR="${REPO_ROOT}/desktop-nova"
VIEWER_CORE_DIR="${REPO_ROOT}/tools/viewer-core"
VIEWER_BACKEND_DIR="${REPO_ROOT}/bin/viewer-backend"

log() {
    printf '[build-mac-ts] %s\n' "$1"
}

fail() {
    printf '\n[build-mac-ts] Build failed. See the messages above.\n' >&2
    exit 1
}

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf '[build-mac-ts] Missing required command: %s\n' "$1" >&2
        fail
    fi
}

trap fail ERR

log "Repo: ${REPO_ROOT}"

if [ ! -f "${DESKTOP_DIR}/package.json" ]; then
    log "Missing desktop-nova package.json."
    fail
fi

if [ ! -f "${VIEWER_CORE_DIR}/Cargo.toml" ]; then
    log "Missing viewer-core Cargo.toml."
    fail
fi

require_command cargo
require_command npm

log "Building Rust backend binaries..."
(
    cd "${VIEWER_CORE_DIR}"
    cargo build --release --bin litematica_core
    cargo build --release --bin litematica_native_viewer
)

log "Syncing backend binaries..."
mkdir -p "${VIEWER_BACKEND_DIR}"
cp -f "${VIEWER_CORE_DIR}/target/release/litematica_core" "${VIEWER_BACKEND_DIR}/litematica_core.exe"
cp -f "${VIEWER_CORE_DIR}/target/release/litematica_native_viewer" "${VIEWER_BACKEND_DIR}/litematica_native_viewer.exe"
chmod +x "${VIEWER_BACKEND_DIR}/litematica_core.exe" "${VIEWER_BACKEND_DIR}/litematica_native_viewer.exe"

log "Preparing desktop-nova dependencies..."
(
    cd "${DESKTOP_DIR}"
    if [ ! -d node_modules ]; then
        npm install
    fi

    log "Checking layer boundaries..."
    npm run check:layers

    log "Building Tauri desktop app..."
    npm run tauri:build
)

log "Build completed. Opening development window..."
exec /usr/bin/env bash "${SCRIPT_DIR}/dev-mac-ts.sh"
