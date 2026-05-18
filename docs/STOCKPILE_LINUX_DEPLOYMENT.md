# Stockpile Linux Deployment

`stockpile.zip` can be opened directly for static preview, but static preview only stores state in the browser. Multiplayer sync, SQLite sessions, passwords, whitelist, admin APIs, and `/admin` require the Linux backend:

```bash
./litematica_core stockpile serve --zip project.stockpile.zip --bind 0.0.0.0:8787
```

## What To Upload

For full multiplayer collaboration upload both:

- `project.stockpile.zip`
- Linux `litematica_core`

Do not upload the Windows `litematica_core.exe` to a Debian VPS. Build or copy the Linux binary.

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

## Build Linux Backend

Run on Linux from the repository root:

```bash
scripts/stockpile/build_linux_release.sh
```

The script builds:

```text
tools/viewer-core/target/release/litematica_core
```

## Create Deploy Package

After exporting a stockpile ZIP:

```bash
scripts/stockpile/make_linux_deploy.sh \
  --zip data/stockpile/exports/project.stockpile.zip \
  --out dist/stockpile-deploy
```

Package layout:

```text
stockpile-deploy/
  litematica_core
  project.stockpile.zip
  data/
  run-stockpile.sh
  install-systemd.sh
  litematica-stockpile.service.example
  README.md
```

## Start For Testing

```bash
cd stockpile-deploy
./run-stockpile.sh
```

`run-stockpile.sh` sets:

```bash
LBA_DATA_ROOT=./data
```

SQLite sessions are written to:

```text
data/stockpile/sessions/<zip-stem>.sqlite
```

## Passwords Before Public Exposure

If you bind to `0.0.0.0:8787`, set at least an access password:

```bash
printf '%s' 'change-me' | ./litematica_core stockpile set-access-password \
  --zip project.stockpile.zip \
  --password-stdin
```

Admin password:

```bash
printf '%s' 'change-admin' | ./litematica_core stockpile set-admin-password \
  --zip project.stockpile.zip \
  --password-stdin
```

Optional whitelist:

```bash
./litematica_core stockpile whitelist-add --zip project.stockpile.zip --user Eldon
./litematica_core stockpile config-set --zip project.stockpile.zip --key whitelist_enabled --value true
```

## Firewall

For direct public access:

```bash
ufw allow 8787/tcp
ufw status
```

If a panel or tunnel maps public port `30017` to internal port `8787`, expose TCP, not UDP.

## systemd

The generated package includes `install-systemd.sh`, but it does not start or enable the service automatically.

```bash
cd stockpile-deploy
sudo ./install-systemd.sh
sudo systemctl start litematica-stockpile
```

Enable boot autostart only when wanted:

```bash
sudo systemctl enable litematica-stockpile
```

## Example VPS SSH

```powershell
ssh -i "C:\Users\27232\Documents\btp-vps-id_rsa\id_rsa.pem" root@154.17.6.144
```

Upload package from Windows:

```powershell
scp -r .\dist\stockpile-deploy root@154.17.6.144:/root/
```
