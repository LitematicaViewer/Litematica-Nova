#!/usr/bin/env bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
DESKTOP_DIR="${REPO_ROOT}/desktop-nova"
VIEWER_BACKEND_DIR="${REPO_ROOT}/bin/viewer-backend"

log() {
    printf '[dev-mac-ts] %s\n' "$1"
}

fail() {
    printf '\n[dev-mac-ts] Startup failed. See the messages above.\n' >&2
    exit 1
}

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf '[dev-mac-ts] Missing required command: %s\n' "$1" >&2
        fail
    fi
}

trap fail ERR

if [ ! -f "${DESKTOP_DIR}/package.json" ]; then
    log "Missing desktop-nova package.json."
    fail
fi

if [ ! -x "${VIEWER_BACKEND_DIR}/litematica_core.exe" ]; then
    log "Missing executable ${VIEWER_BACKEND_DIR}/litematica_core.exe"
    log "Run bin/build-mac-ts.sh first."
    fail
fi

if [ ! -x "${VIEWER_BACKEND_DIR}/litematica_native_viewer.exe" ]; then
    log "Missing executable ${VIEWER_BACKEND_DIR}/litematica_native_viewer.exe"
    log "Run bin/build-mac-ts.sh first."
    fail
fi

require_command npm

cd "${DESKTOP_DIR}"

if [ ! -d node_modules ]; then
    npm install
fi

log "Starting Tauri development window..."
set +e
npm run tauri:dev
exit_code=$?
set -e

printf '\n[dev-mac-ts] Tauri dev process exited with code %s.\n' "${exit_code}"
exit "${exit_code}"
