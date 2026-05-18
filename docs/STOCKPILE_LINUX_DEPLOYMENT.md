# Stockpile Linux Deployment

`stockpile.zip` can be opened directly for static preview, but static preview only stores state in the browser. Multiplayer sync, SQLite sessions, passwords, whitelist, admin APIs, and `/admin` require the lightweight `stockpile_server` backend:

```bash
./stockpile_server --root /path/to/unzipped-stockpile --bind 0.0.0.0:8787
```

## What To Upload

For full multiplayer collaboration export a multi package locally:

```powershell
"access-pass`nadmin-pass" | bin\viewer-backend\litematica_core.exe stockpile export-zip --input <file.litematic> --output data\stockpile\exports\project.stockpile.zip --minecraft-version 1.21.10 --mode multi --target linux-x64 --access-password-stdin --admin-password-stdin --whitelist-file users.txt --allow-guest-readonly true --admin-page-enabled true
```

Then upload and unzip `project.stockpile.zip`. The deployment side does not need `.litematic`, recipe cache, Minecraft client jars, Rust source, or `litematica_core`.

## Debian 12 Dependencies

```bash
apt update
apt install -y curl ca-certificates build-essential pkg-config libssl-dev unzip \
  libx11-dev libxi-dev libxcursor-dev libxrandr-dev libxinerama-dev \
  libwayland-dev libxkbcommon-dev libasound2-dev libudev-dev
```

Install Rust if the VPS does not already have `cargo`:

```bash
curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
. "$HOME/.cargo/env"
```

## Build Linux Server Binary

Multi export requires real server binaries for the selected target under `bin/stockpile-server/<platform>/`. Use the `stockpile-server` GitHub Actions workflow or build on each platform and place artifacts at:

```text
bin/stockpile-server/windows-x64/stockpile_server.exe
bin/stockpile-server/linux-x64/stockpile_server
bin/stockpile-server/macos-x64/stockpile_server
bin/stockpile-server/macos-arm64/stockpile_server
```

The real binaries are ignored by Git and must not be committed. On Windows, local builds only produce `windows-x64`; Linux and macOS binaries come from GitHub Actions artifacts or from builds on those platforms. Before exporting a Linux VPS package:

```powershell
scripts\stockpile\verify_stockpile_server_bins.ps1 -Target linux-x64
scripts\stockpile\fetch_stockpile_server_artifacts.ps1 -Target linux-x64
scripts\stockpile\verify_stockpile_server_bins.ps1 -Target linux-x64
```

`fetch_stockpile_server_artifacts.ps1` requires the GitHub CLI (`gh`) and an authenticated account that can read workflow artifacts. If `gh` or permissions are unavailable, download the required target artifact manually from the `stockpile-server` workflow run and place it in the path above. Single packages do not need server binaries. Use multiple `--target` flags or `--target all` only when you want a multi-platform package.

For Linux only, build from the repository root:

```bash
cargo build --release --bin stockpile_server
```

Copy the binary into the fixed export input directory:

```bash
cp tools/viewer-core/target/release/stockpile_server bin/stockpile-server/linux-x64/stockpile_server
chmod +x bin/stockpile-server/linux-x64/stockpile_server
```

## Start For Testing

From the unzipped package:

```bash
chmod +x server/linux-x64/start.sh server/linux-x64/stockpile_server
./server/linux-x64/start.sh
```

The server writes state to:

```text
db/stockpile.sqlite
```

`litematica_core stockpile serve --zip` remains available as a legacy/internal compatibility command, but new deployments should use `stockpile_server`.

## Passwords Before Public Exposure

Passwords, whitelist users, and default config are initialized during `export-zip --mode multi` and stored as hashes/config rows in `db/stockpile.sqlite`. The deployment server does not provide a password bootstrap CLI. To change access rules, use `/admin` or rebuild the package.

## Firewall

For direct public access:

```bash
ufw allow 8787/tcp
ufw status
```

If a panel or tunnel maps public port `30017` to internal port `8787`, expose TCP, not UDP.

## systemd

The multi package is designed to run directly from its platform start script. For a long-running VPS service, create a small systemd unit that runs the same command:

```ini
[Service]
WorkingDirectory=/opt/litematica-stockpile
ExecStart=/opt/litematica-stockpile/server/linux-x64/stockpile_server --root /opt/litematica-stockpile --bind 0.0.0.0:8787
Restart=on-failure
```

## Example VPS SSH

```powershell
ssh -i "C:\Users\27232\Documents\btp-vps-id_rsa\id_rsa.pem" root@154.17.6.144
```

Upload package from Windows:

```powershell
scp -r .\dist\stockpile-deploy root@154.17.6.144:/root/
```
