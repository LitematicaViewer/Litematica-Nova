#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/stockpile/make_linux_deploy.sh --zip <project.stockpile.zip> --out <deploy-dir>

This unpacks a multi-mode stockpile ZIP for Linux deployment. If a Linux
stockpile_server binary is available at tools/viewer-core/target/release, it is
copied into server/linux-x64/.
EOF
}

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
ZIP_PATH=""
OUT_DIR=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --zip)
      [[ $# -ge 2 ]] || { echo "missing value after --zip" >&2; exit 1; }
      ZIP_PATH="$2"
      shift 2
      ;;
    --zip=*)
      ZIP_PATH="${1#--zip=}"
      shift
      ;;
    --out)
      [[ $# -ge 2 ]] || { echo "missing value after --out" >&2; exit 1; }
      OUT_DIR="$2"
      shift 2
      ;;
    --out=*)
      OUT_DIR="${1#--out=}"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "${ZIP_PATH}" || -z "${OUT_DIR}" ]]; then
  usage >&2
  exit 1
fi

if [[ "${ZIP_PATH}" != /* ]]; then
  ZIP_PATH="${REPO_ROOT}/${ZIP_PATH}"
fi
if [[ "${OUT_DIR}" != /* ]]; then
  OUT_DIR="${REPO_ROOT}/${OUT_DIR}"
fi
if [[ "${OUT_DIR}" == "/" || "${OUT_DIR}" == "${REPO_ROOT}" ]]; then
  echo "refusing to replace unsafe deploy output directory: ${OUT_DIR}" >&2
  exit 1
fi
if [[ ! -f "${ZIP_PATH}" ]]; then
  echo "stockpile zip not found: ${ZIP_PATH}" >&2
  exit 1
fi
if ! command -v unzip >/dev/null 2>&1; then
  echo "unzip was not found" >&2
  exit 1
fi

rm -rf "${OUT_DIR}"
mkdir -p "${OUT_DIR}"
unzip -q "${ZIP_PATH}" -d "${OUT_DIR}"

BINARY="${REPO_ROOT}/tools/viewer-core/target/release/stockpile_server"
if [[ -x "${BINARY}" ]]; then
  mkdir -p "${OUT_DIR}/server/linux-x64"
  cp "${BINARY}" "${OUT_DIR}/server/linux-x64/stockpile_server"
  chmod +x "${OUT_DIR}/server/linux-x64/stockpile_server"
fi

if [[ -f "${OUT_DIR}/server/linux-x64/start.sh" ]]; then
  chmod +x "${OUT_DIR}/server/linux-x64/start.sh"
fi

echo "Created stockpile deploy directory:"
echo "${OUT_DIR}"
echo "Run:"
echo "  cd ${OUT_DIR}"
echo "  ./server/linux-x64/start.sh"
