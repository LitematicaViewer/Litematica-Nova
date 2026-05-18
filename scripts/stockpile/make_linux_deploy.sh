#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/stockpile/make_linux_deploy.sh --zip <project.stockpile.zip> --out <deploy-dir>

The script expects a Linux release binary at:
  tools/viewer-core/target/release/litematica_core

Build it first on Linux with:
  scripts/stockpile/build_linux_release.sh
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

BINARY="${REPO_ROOT}/tools/viewer-core/target/release/litematica_core"
if [[ ! -f "${ZIP_PATH}" ]]; then
  echo "stockpile zip not found: ${ZIP_PATH}" >&2
  exit 1
fi
if [[ ! -x "${BINARY}" ]]; then
  echo "Linux litematica_core not found or not executable: ${BINARY}" >&2
  echo "Run scripts/stockpile/build_linux_release.sh on Linux first." >&2
  exit 1
fi

rm -rf "${OUT_DIR}"
mkdir -p "${OUT_DIR}/data"

ZIP_NAME="$(basename -- "${ZIP_PATH}")"
cp "${BINARY}" "${OUT_DIR}/litematica_core"
cp "${ZIP_PATH}" "${OUT_DIR}/${ZIP_NAME}"
chmod +x "${OUT_DIR}/litematica_core"

cat > "${OUT_DIR}/run-stockpile.sh" <<EOF
#!/usr/bin/env bash
set -euo pipefail

cd -- "\$(dirname -- "\${BASH_SOURCE[0]}")"

export LBA_DATA_ROOT="\${LBA_DATA_ROOT:-./data}"
BIND="\${BIND:-0.0.0.0:8787}"
ZIP="./${ZIP_NAME}"

echo "Stockpile ZIP: \${ZIP}"
echo "Data root: \${LBA_DATA_ROOT}"
echo "Bind: \${BIND}"
echo "Local URL: http://127.0.0.1:\${BIND##*:}"
echo "Public URL: http://<server-ip>:\${BIND##*:}"

exec ./litematica_core stockpile serve --zip "\${ZIP}" --bind "\${BIND}"
EOF
chmod +x "${OUT_DIR}/run-stockpile.sh"

cat > "${OUT_DIR}/litematica-stockpile.service.example" <<EOF
[Unit]
Description=Litematica-BA Stockpile
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=__STOCKPILE_DIR__
Environment=LBA_DATA_ROOT=__STOCKPILE_DIR__/data
Environment=BIND=0.0.0.0:8787
ExecStart=__STOCKPILE_DIR__/litematica_core stockpile serve --zip __STOCKPILE_DIR__/${ZIP_NAME} --bind 0.0.0.0:8787
Restart=on-failure
RestartSec=3

[Install]
WantedBy=multi-user.target
EOF

cat > "${OUT_DIR}/install-systemd.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
STOCKPILE_DIR="$(pwd)"

if [[ "${EUID}" -ne 0 ]]; then
  echo "install-systemd.sh must be run as root." >&2
  exit 1
fi

sed "s#__STOCKPILE_DIR__#${STOCKPILE_DIR}#g" litematica-stockpile.service.example > /etc/systemd/system/litematica-stockpile.service
chmod 0644 /etc/systemd/system/litematica-stockpile.service
systemctl daemon-reload

cat <<'MSG'
Installed /etc/systemd/system/litematica-stockpile.service and reloaded systemd.

This script does not enable or start the service automatically.
Start manually:
  systemctl start litematica-stockpile

Enable after boot only if you want long-running service:
  systemctl enable litematica-stockpile
MSG
EOF
chmod +x "${OUT_DIR}/install-systemd.sh"

cat > "${OUT_DIR}/README.md" <<EOF
# Stockpile Linux Deploy Package

This package runs a Litematica-BA stockpile multiplayer service.

## Files

- \`litematica_core\`: Linux backend binary.
- \`${ZIP_NAME}\`: stockpile web ZIP.
- \`data/\`: runtime data root. SQLite sessions are created under \`data/stockpile/sessions/\`.
- \`run-stockpile.sh\`: foreground runner for testing.
- \`install-systemd.sh\`: optional systemd installer. It does not start or enable the service.
- \`litematica-stockpile.service.example\`: example systemd unit.

## Run

\`\`\`bash
chmod +x ./litematica_core ./run-stockpile.sh
./run-stockpile.sh
\`\`\`

Override bind address:

\`\`\`bash
BIND=127.0.0.1:8787 ./run-stockpile.sh
\`\`\`

Open:

\`\`\`text
http://<server-ip>:8787/
http://<server-ip>:8787/admin
\`\`\`

If you expose \`0.0.0.0:8787\` to the public internet, configure an access password first.
EOF

echo "Created stockpile deploy package:"
echo "${OUT_DIR}"
