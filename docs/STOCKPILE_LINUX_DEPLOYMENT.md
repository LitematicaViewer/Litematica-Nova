# Stockpile Linux Deployment

`stockpile.zip` can be opened directly for static preview, but static preview only stores state in the browser. Multiplayer sync, SQLite sessions, passwords, whitelist, admin APIs, and `/admin` require the lightweight `stockpile_server` backend:

```bash
./stockpile_server --root /path/to/unzipped-stockpile --bind 0.0.0.0:8787
```

## What To Upload

For full multiplayer collaboration export a multi package locally:

```powershell
bin\viewer-backend\litematica_core.exe stockpile export-zip --input <file.litematic> --output data\stockpile\exports\project.stockpile.zip --minecraft-version 1.21.10 --mode multi
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

If the package does not already contain `server/linux-x64/stockpile_server`, build it on Linux from the repository root:

```bash
cargo build --release --bin stockpile_server
```

Copy the binary into the unzipped package:

```bash
cp tools/viewer-core/target/release/stockpile_server /path/to/unzipped-stockpile/server/linux-x64/stockpile_server
chmod +x /path/to/unzipped-stockpile/server/linux-x64/stockpile_server
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

If you bind to `0.0.0.0:8787`, set at least an access password:

```bash
TODO: password bootstrap for unpacked stockpile_server packages is planned.
```

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
