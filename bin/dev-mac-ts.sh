#!/usr/bin/env bash
set -u

APP_EXIT=1
EXIT_REASON="Unknown failure"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." 2>/dev/null && pwd)"
if [ -z "$ROOT" ]; then
    echo "[dev-mac-ui] Cannot resolve project root."
    exit 1
fi

UI_ROOT="$ROOT/src/litematicanova/ui"
UI_DEV_HOME="${LITEMATICA_NOVA_UI_DEV_HOME:-$ROOT/.tmp/ui-dev-env}"
NPM_CONFIG_CACHE="$UI_DEV_HOME/npm-cache"
STAMP_FILE="$UI_DEV_HOME/package.stamp"

pause_if_interactive() {
    if [ "${DEV_MAC_UI_NO_PAUSE:-}" != "1" ] && [ -t 0 ]; then
        printf "Press Enter to continue..."
        read -r _ || true
    fi
}

fail() {
    echo
    echo "[dev-mac-ui] $EXIT_REASON"
    echo "[dev-mac-ui] Exit code: $APP_EXIT"
    echo "[dev-mac-ui] To close automatically, set DEV_MAC_UI_NO_PAUSE=1."
    pause_if_interactive
    exit "$APP_EXIT"
}

succeed() {
    echo
    echo "[dev-mac-ui] $EXIT_REASON"
    echo "[dev-mac-ui] To close automatically, set DEV_MAC_UI_NO_PAUSE=1."
    pause_if_interactive
    exit 0
}

file_stamp_entry() {
    local file="$1"
    local name
    name="$(basename "$file")"
    if stat -f "%m=%z" "$file" >/dev/null 2>&1; then
        printf "%s=%s" "$name" "$(stat -f "%m=%z" "$file")"
        return
    fi
    if stat -c "%Y=%s" "$file" >/dev/null 2>&1; then
        printf "%s=%s" "$name" "$(stat -c "%Y=%s" "$file")"
        return
    fi
    printf "%s=%s=%s" "$name" "$(date -r "$file" +%s 2>/dev/null || echo 0)" "$(wc -c < "$file" 2>/dev/null || echo 0)"
}

if [ ! -f "$UI_ROOT/package.json" ]; then
    EXIT_REASON="Missing UI package: $UI_ROOT/package.json"
    fail
fi

mkdir -p "$UI_DEV_HOME" "$NPM_CONFIG_CACHE" || {
    EXIT_REASON="Cannot create UI dev environment: $UI_DEV_HOME"
    fail
}

if ! command -v node >/dev/null 2>&1; then
    EXIT_REASON="node not found. Install Node.js 18+ and try again."
    fail
fi

if ! command -v npm >/dev/null 2>&1; then
    EXIT_REASON="npm not found. Install Node.js 18+ and try again."
    fail
fi

if ! command -v cargo >/dev/null 2>&1; then
    if [ -x "$HOME/.cargo/bin/cargo" ]; then
        export PATH="$HOME/.cargo/bin:$PATH"
    fi
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "[dev-mac-ui] cargo not found. Rust is required for the Tauri desktop UI."
    echo "[dev-mac-ui] Install Rust from https://rustup.rs/ or add ~/.cargo/bin to PATH."
    EXIT_REASON="cargo not found."
    fail
fi

SYNC_KEY=""
for file in "$UI_ROOT/package.json" "$UI_ROOT/package-lock.json"; do
    if [ -f "$file" ]; then
        SYNC_KEY="$SYNC_KEY|$(file_stamp_entry "$file")"
    fi
done

CURRENT_KEY=""
if [ -f "$STAMP_FILE" ]; then
    CURRENT_KEY="$(cat "$STAMP_FILE")"
fi

NEED_SYNC=1
if [ -x "$UI_ROOT/node_modules/.bin/tauri" ] && [ -x "$UI_ROOT/node_modules/.bin/vite" ] && [ "$CURRENT_KEY" = "$SYNC_KEY" ]; then
    NEED_SYNC=0
fi

cd "$UI_ROOT" || {
    EXIT_REASON="Cannot enter UI root: $UI_ROOT"
    fail
}

export NPM_CONFIG_CACHE

if [ "$NEED_SYNC" = "1" ]; then
    echo "[dev-mac-ui] Syncing npm dependencies into \"$UI_ROOT/node_modules\" ..."
    npm install --no-audit --no-fund
    APP_EXIT=$?
    if [ "$APP_EXIT" -ne 0 ]; then
        EXIT_REASON="npm install failed."
        fail
    fi
    printf "%s\n" "$SYNC_KEY" > "$STAMP_FILE"
else
    echo "[dev-mac-ui] Reusing existing npm dependencies in \"$UI_ROOT/node_modules\"."
fi

export PATH="$UI_ROOT/node_modules/.bin:$PATH"
export TAURI_DEV_HOST="127.0.0.1"
export TAURI_ROOT="$ROOT/src/litematicanova/platform/tauri"
export CARGO_TARGET_DIR="$TAURI_ROOT/target"

echo "[dev-mac-ui] Starting Tauri + React development shell ..."
echo "[dev-mac-ui] UI root: $UI_ROOT"
npm run tauri:dev
APP_EXIT=$?

if [ "$APP_EXIT" -eq 0 ]; then
    EXIT_REASON="UI dev command exited with code 0."
    succeed
fi

EXIT_REASON="UI dev command exited with error $APP_EXIT."
fail
