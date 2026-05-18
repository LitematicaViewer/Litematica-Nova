# Stockpile Web Export Handoff

This handoff covers the stockpile web export pipeline: `litematica_core` builds self-contained web packages from a `.litematic`, while the lightweight `stockpile_server` only serves an already exported package and coordinates multiplayer state through SQLite.

## Roles

- `litematica_core`: parses `.litematic`, reads recipe/item/icon/i18n caches, builds `single` or `multi` `.stockpile.zip` packages, initializes deploy-time SQLite config, and manages local session export/import/reset utilities.
- `stockpile_server`: serves an extracted `multi` package, hosts static files, exposes collaboration/auth/admin APIs, and writes `db/stockpile.sqlite`.
- Deployment machines do not need source code, Rust, `litematica_core`, recipe cache, Minecraft client jars, or the original `.litematic`.

## A. Build and export stockpile web packages

Run export commands from the repository root after building or syncing `bin\viewer-backend\litematica_core.exe`.

### Single offline package

```powershell
bin\viewer-backend\litematica_core.exe stockpile export-zip --input tools\viewer-core\tests\fixtures\stats_water_fixture.litematic --output data\stockpile\exports\project-single.stockpile.zip --minecraft-version 1.21.10 --mode single
```

`single` packages are static/offline. After unzipping, users can open `index.html` directly. State is stored in the browser's localStorage, so it is useful for one person or preview, not shared multiplayer coordination.

### Multi runnable package

Passwords are written at build time through stdin. With both access and admin passwords enabled, line 1 is the access password and line 2 is the admin password:

```powershell
"access-pass`nadmin-pass`n" | bin\viewer-backend\litematica_core.exe stockpile export-zip --input tools\viewer-core\tests\fixtures\stats_water_fixture.litematic --output data\stockpile\exports\project-linux.stockpile.zip --minecraft-version 1.21.10 --mode multi --target linux-x64 --access-password-stdin --admin-password-stdin --whitelist-file users.txt --allow-guest-readonly false --admin-page-enabled true
```

Supported `--target` values:

- `windows-x64`
- `linux-x64`
- `macos-x64`
- `macos-arm64`
- `all`

`--target` may be passed more than once. If omitted in `multi` mode, the exporter uses the current platform default target. On Windows that is `windows-x64`. `--target all` requires all four real server binaries to be present.

Key export options:

- `--access-password-stdin`: reads the access password from stdin and stores only the hash in SQLite.
- `--admin-password-stdin`: reads the admin password from stdin and stores only the hash in SQLite.
- `--whitelist-file <users.txt>`: imports one allowed user id per non-empty line.
- `--allow-guest-readonly true|false`: lets unauthenticated/non-whitelisted users read without writing when true.
- `--admin-page-enabled true|false`: enables or disables `/admin` in the package config.

### What the ZIP contains

A `single` package contains the static web app and precomputed data:

```text
index.html
assets/app.css
assets/app.js
assets/icons/*.png
data/manifest.json
data/materials.json
data/recipe_status.json
data/recipe_trees.json
data/icons.json
data/item_names.json
data/i18n.json
```

A `multi` package includes all `single` files plus deploy runtime files:

```text
db/stockpile.sqlite
server/<target>/stockpile_server[.exe]
server/<target>/start.cmd or start.sh
README.txt
```

The deploy SQLite database is initialized during export with stockpile config, password hashes, whitelist rows, schema metadata, and the package hash.

## B. Recipe cache, item names, icons, and i18n

Check cache state:

```powershell
bin\viewer-backend\litematica_core.exe stockpile recipe-status --minecraft-version 1.21.10
```

Fetch recipe data:

```powershell
bin\viewer-backend\litematica_core.exe stockpile recipe-fetch --minecraft-version 1.21.10
```

Recipe cache lives under:

```text
data/cache/recipes/minecraft_<version>/
```

`recipe-fetch` resolves the official Minecraft client jar through Mojang metadata, extracts recipe JSON, and writes local cache files. `export-zip` does not fetch during deployment; it uses the local cache to build `data/recipe_status.json` and `data/recipe_trees.json`.

Item names and icons are also precomputed before packaging:

- names: `data/cache/item-names/minecraft_<version>/en_us.json` and `zh_cn.json`
- icons: `data/cache/item-icons/minecraft_<version>/icons/`

During export, only the materials and recipe-tree nodes needed by the project are copied into `data/item_names.json`, `data/icons.json`, `data/i18n.json`, and `assets/icons/*.png`. Missing icons get a fallback PNG and missing names are marked in the exported status payloads. Generated cache directories under `data/cache/` are local artifacts and must not be committed.

## C. Session and SQLite

Local session utilities operate on the stockpile session database for a ZIP or on JSON session exports:

```powershell
bin\viewer-backend\litematica_core.exe stockpile session-info --zip data\stockpile\exports\project.stockpile.zip
bin\viewer-backend\litematica_core.exe stockpile session-export --zip data\stockpile\exports\project.stockpile.zip --output data\stockpile\sessions\project.state.json
bin\viewer-backend\litematica_core.exe stockpile session-import --zip data\stockpile\exports\project.stockpile.zip --input data\stockpile\sessions\project.state.json
bin\viewer-backend\litematica_core.exe stockpile session-import --zip data\stockpile\exports\project.stockpile.zip --input data\stockpile\sessions\project.state.json --replace
bin\viewer-backend\litematica_core.exe stockpile session-reset --zip data\stockpile\exports\project.stockpile.zip --yes
```

`session-info` reports session/database status. `session-export` writes claims/participants/session data to JSON. `session-import` validates schema and ZIP hash, then merges by default or replaces claims with `--replace`. `session-reset --yes` clears claims, keeps participants, and rebinds the session database to the current ZIP hash.

In an extracted `multi` package, the live server state is:

```text
db/stockpile.sqlite
```

That file stores claims, participants, config, password hashes, whitelist rows, admin material notes/locks, audit log, schema version, and ZIP/project hash metadata. The deployment server no longer parses `.litematic`; all project material, recipe, icon, item-name, and i18n data has already been exported into JSON.

## D. `stockpile_server`

Serve an extracted package:

```powershell
bin\viewer-backend\stockpile_server.exe --root <unzipped-stockpile-dir> --bind 127.0.0.1:8787
```

The standalone server also accepts the generated-script form:

```powershell
bin\viewer-backend\stockpile_server.exe serve --root <unzipped-stockpile-dir> --bind 127.0.0.1:8787
```

Generated package launchers:

- Windows: double-click or run `server\windows-x64\start.cmd`
- Linux: `chmod +x server/linux-x64/start.sh server/linux-x64/stockpile_server` then `./server/linux-x64/start.sh`
- macOS x64: `chmod +x server/macos-x64/start.sh server/macos-x64/stockpile_server` then `./server/macos-x64/start.sh`
- macOS arm64: `chmod +x server/macos-arm64/start.sh server/macos-arm64/stockpile_server` then `./server/macos-arm64/start.sh`

The server is intentionally small: static file serving, JSON API, auth sessions, rate-limited login, whitelist/admin/config APIs, audit log, and SQLite persistence. It does not need `litematica_core`, source code, Rust, recipe cache, Minecraft jars, native viewer assets, or `.litematic` files.

## E. Permissions and admin

Access controls are initialized during `export-zip --mode multi`:

- access password: gates project/state reads when guest readonly is disabled and gates writes when access auth is required.
- admin password: grants `/admin` and admin APIs.
- whitelist: limits write access to listed user ids unless the user is admin.
- readonly guest: when enabled, unauthenticated or non-whitelisted users can view but cannot write.
- admin page: `/admin`, controlled by `--admin-page-enabled`.

Auth/session notes:

- Passwords are stored as hashes in SQLite, not plaintext.
- Failed `/api/auth/access` and `/api/auth/admin` login attempts are rate limited by login type plus client key.
- Current lockout behavior is 5 failed attempts in 5 minutes, then 10 minutes locked out with HTTP `429 Too Many Requests` and `Retry-After`.
- Successful login clears the failed-attempt record for that login type/client key.
- Session cookies are `HttpOnly`; `Secure` is added when the request is known to be HTTPS through a trusted local reverse proxy.

Admin capabilities include whitelist edits, material notes/storage locations, material locks, claim cleanup by material/user, password/config actions, and audit visibility.

## F. Deployment

### Windows local package

Export a Windows-targeted `multi` package, unzip it, then run:

```powershell
server\windows-x64\start.cmd
```

Or run the server directly:

```powershell
server\windows-x64\stockpile_server.exe serve --root . --bind 0.0.0.0:8787
```

### Linux/VPS package

Export a Linux-targeted `multi` package locally or in CI, upload only the ZIP, unzip it on the server, and run:

```bash
chmod +x server/linux-x64/start.sh server/linux-x64/stockpile_server
BIND=0.0.0.0:8787 ./server/linux-x64/start.sh
```

For VPS deployment, upload the target-specific `.stockpile.zip`; do not upload source, `.litematic`, `data/cache`, Rust toolchains, or `litematica_core`.

### macOS package

Export `--target macos-x64`, `--target macos-arm64`, or both, unzip, then run the matching script:

```bash
chmod +x server/macos-arm64/start.sh server/macos-arm64/stockpile_server
./server/macos-arm64/start.sh
```

### GitHub Actions artifacts

Cross-platform server binaries are built by `.github/workflows/stockpile-server.yml`. Expected local binary input paths are:

```text
bin/stockpile-server/windows-x64/stockpile_server.exe
bin/stockpile-server/linux-x64/stockpile_server
bin/stockpile-server/macos-x64/stockpile_server
bin/stockpile-server/macos-arm64/stockpile_server
```

Verify and fetch artifacts:

```powershell
scripts\stockpile\verify_stockpile_server_bins.ps1 -Target linux-x64
scripts\stockpile\fetch_stockpile_server_artifacts.ps1 -RunId <workflow-run-id> -Target linux-x64
scripts\stockpile\verify_stockpile_server_bins.ps1 -Target linux-x64
```

`fetch_stockpile_server_artifacts.ps1` requires GitHub CLI (`gh`) and permission to read workflow artifacts. If `gh` is unavailable, download the artifact manually and place the binary in the matching `bin/stockpile-server/<target>/` directory.

## G. FAQ

### Why does double-clicking HTML only support one person?

A static `single` package has no shared backend. Browser state is localStorage on that machine/browser profile, so another player cannot see or update the same claims.

### Why does multiplayer need `stockpile_server`?

Multiplayer needs one shared process to serialize writes, persist claims in SQLite, enforce access rules, serve current state, and expose admin/whitelist/audit APIs.

### Why does deployment not need `litematica_core`?

`litematica_core` does all expensive project work at export time: parse `.litematic`, compute materials, resolve recipes, build recipe trees, collect names/icons/i18n, and initialize SQLite. The deployed package already contains those outputs.

### What if the target platform binary is missing?

Run `scripts\stockpile\verify_stockpile_server_bins.ps1 -Target <target>`. If missing, fetch the GitHub Actions artifact with `scripts\stockpile\fetch_stockpile_server_artifacts.ps1 -RunId <run-id> -Target <target>` or download it manually. Export only the target you need unless you intentionally want `--target all`.

### What should be uploaded to a VPS?

Upload the generated target-specific `multi` `.stockpile.zip`, unzip it, and run the included platform start script. Do not upload source, `data/cache`, `data/stockpile` build workspace, real local binary build outputs outside the package, `.litematic`, or Rust tooling.
