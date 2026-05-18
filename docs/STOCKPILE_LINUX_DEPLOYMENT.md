# Stockpile Linux Deployment

Use this guide for Linux/VPS deployment of an already exported stockpile `multi` package. The deployment side runs only the lightweight `stockpile_server`; it does not need `litematica_core`, Rust, repository source, recipe cache, Minecraft client jars, or the original `.litematic`.

For the full CLI handoff, see `docs/STOCKPILE_HANDOFF.md`.

## What To Upload

Export a Linux-targeted `multi` package locally or in CI:

```powershell
"access-pass`nadmin-pass`n" | bin\viewer-backend\litematica_core.exe stockpile export-zip --input <file.litematic> --output data\stockpile\exports\project-linux.stockpile.zip --minecraft-version 1.21.10 --mode multi --target linux-x64 --access-password-stdin --admin-password-stdin --whitelist-file users.txt --allow-guest-readonly false --admin-page-enabled true
```

Upload only `project-linux.stockpile.zip` to the VPS and unzip it. Do not upload:

- `.litematic` source files
- repository source code
- `litematica_core`
- Rust toolchains or build directories
- `data/cache/`
- local `data/stockpile/` workspaces
- untracked real binaries outside the generated package

The ZIP already contains precomputed materials, recipe trees, item names, icons, i18n payloads, an initialized `db/stockpile.sqlite`, and the selected `server/linux-x64/stockpile_server` binary.

## Prepare Linux Server Binary Before Export

`export-zip --mode multi --target linux-x64` requires a real Linux server binary at:

```text
bin/stockpile-server/linux-x64/stockpile_server
```

Prefer GitHub Actions artifacts from `.github/workflows/stockpile-server.yml`:

```powershell
scripts\stockpile\verify_stockpile_server_bins.ps1 -Target linux-x64
scripts\stockpile\fetch_stockpile_server_artifacts.ps1 -RunId <workflow-run-id> -Target linux-x64
scripts\stockpile\verify_stockpile_server_bins.ps1 -Target linux-x64
```

`fetch_stockpile_server_artifacts.ps1` requires GitHub CLI (`gh`) and artifact read permission. If that is unavailable, manually download the `stockpile-server-linux-x64` artifact and place `stockpile_server` in the path above.

Expected binary input paths for all targets are:

```text
bin/stockpile-server/windows-x64/stockpile_server.exe
bin/stockpile-server/linux-x64/stockpile_server
bin/stockpile-server/macos-x64/stockpile_server
bin/stockpile-server/macos-arm64/stockpile_server
```

Use `--target all` only when all four are present. For a VPS, `--target linux-x64` is usually enough.

## Start On The VPS

After upload and unzip:

```bash
cd /opt/litematica-stockpile
chmod +x server/linux-x64/start.sh server/linux-x64/stockpile_server
BIND=0.0.0.0:8787 ./server/linux-x64/start.sh
```

Or run the binary directly:

```bash
./server/linux-x64/stockpile_server serve --root . --bind 0.0.0.0:8787
```

The standalone server also accepts:

```bash
./server/linux-x64/stockpile_server --root . --bind 0.0.0.0:8787
```

Open the site at the host/port mapped to `8787`.

## SQLite State

Live multiplayer state is stored in:

```text
db/stockpile.sqlite
```

This database stores claims, participants, config, password hashes, whitelist rows, admin notes/locks, audit log, schema version, and project hash metadata. Back up this file if you need to preserve live collaboration state across deployments.

The deployment server does not parse `.litematic`; all project data is served from exported JSON files under `data/`.

## Passwords, Whitelist, Admin, And Rate Limit

Passwords, whitelist users, and default config are initialized during `export-zip --mode multi`:

- `--access-password-stdin` writes the access password hash.
- `--admin-password-stdin` writes the admin password hash.
- `--whitelist-file <users.txt>` imports allowed users.
- `--allow-guest-readonly true|false` controls guest read access.
- `--admin-page-enabled true|false` controls `/admin`.

Failed `/api/auth/access` and `/api/auth/admin` attempts are rate limited. After repeated failures, the server returns `429 Too Many Requests` with `Retry-After`.

To change access rules after deployment, use `/admin` or rebuild/redeploy the package. The deployment server intentionally has no bootstrap CLI for plaintext passwords.

## Firewall And Port Mapping

For direct public access:

```bash
ufw allow 8787/tcp
ufw status
```

If a panel, tunnel, or NAT maps public port `30017` to internal `8787`, expose TCP. UDP is not used.

HTTPS is optional for basic operation. If you place Caddy/Nginx/Cloudflare Tunnel in front, terminate HTTPS at the reverse proxy and forward to the local `stockpile_server`; the server will add `Secure` to session cookies only when the HTTPS forwarding signal comes from a trusted local proxy.

## systemd Example

```ini
[Unit]
Description=Litematica Stockpile
After=network.target

[Service]
WorkingDirectory=/opt/litematica-stockpile
ExecStart=/opt/litematica-stockpile/server/linux-x64/stockpile_server serve --root /opt/litematica-stockpile --bind 0.0.0.0:8787
Restart=on-failure
User=stockpile
Group=stockpile

[Install]
WantedBy=multi-user.target
```

Use a dedicated low-privilege user and keep the package directory writable only where `db/stockpile.sqlite` must be updated.

## Troubleshooting

- Double-clicked HTML is single-user only: use `multi` plus `stockpile_server` for shared state.
- Missing Linux binary: run `verify_stockpile_server_bins.ps1`, then fetch or manually place the artifact.
- Login works locally but not through a proxy: verify the proxy forwards HTTP to the correct bind address/port and preserves cookies.
- Need to move a live session: back up and restore `db/stockpile.sqlite`, or use local `session-export`/`session-import` workflows before packaging.
