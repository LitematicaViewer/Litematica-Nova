#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
VIEWER_CORE_DIR="${REPO_ROOT}/tools/viewer-core"

if ! command -v rustc >/dev/null 2>&1 || ! command -v cargo >/dev/null 2>&1; then
  cat >&2 <<'EOF'
rustc/cargo was not found.

Install Rust first, then rerun this script:
  curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
  . "$HOME/.cargo/env"

On Debian 12, install build tools before building:
  apt update
  apt install -y curl ca-certificates build-essential pkg-config libssl-dev unzip \
    libx11-dev libxi-dev libxcursor-dev libxrandr-dev libxinerama-dev \
    libwayland-dev libxkbcommon-dev libasound2-dev libudev-dev
EOF
  exit 1
fi

if [[ ! -f "${VIEWER_CORE_DIR}/Cargo.toml" ]]; then
  echo "viewer-core Cargo.toml not found: ${VIEWER_CORE_DIR}/Cargo.toml" >&2
  exit 1
fi

cd "${VIEWER_CORE_DIR}"
cargo build --release --bin stockpile_server

BINARY="${VIEWER_CORE_DIR}/target/release/stockpile_server"
if [[ ! -x "${BINARY}" ]]; then
  echo "Linux stockpile_server was not produced at: ${BINARY}" >&2
  exit 1
fi

echo "Linux stockpile_server:"
echo "${BINARY}"

OUT="${REPO_ROOT}/bin/stockpile-server/linux-x64/stockpile_server"
mkdir -p "$(dirname -- "${OUT}")"
cp "${BINARY}" "${OUT}"
chmod +x "${OUT}"
echo "Copied to:"
echo "${OUT}"
