# Litematica-BA

Current mainline:

- `desktop-js/` - active React/Tauri desktop UI.
- `tools/viewer-core/` - active Rust backend, `.litematic` core, cache tools, and native viewer.
- `bin/viewer-backend/` - runtime viewer/core executables used by the app.
- `data/` - current templates, AI projection config, BlockState DB, and projection-library data.
- `scripts/` - current maintenance scripts for assistant index, diagnostics, fixtures, and BlockState DB generation.

Removed legacy paths:

- `desktop-ui/` has been deleted.
- `script/` has been deleted.
- `src/litematicaba/` and the old PySide6 UI have been deleted.
- `launch_ba_ui.bat`, old Python packaging scripts, `pyproject.toml`, and old Python `requirements.txt` have been deleted.

Do not use `python -m litematicaba`; it is no longer a supported entry point.

## Maintainer Docs

Start with:

1. `MAINTAINERS.md`
2. `docs/PROJECT_MAP.md`
3. `docs/FEATURE_ENTRYPOINTS.md`
4. `docs/DATA_FLOW.md`
5. `docs/MODULE_BOUNDARIES.md`
6. `docs/DIAGNOSTICS.md`
7. `docs/CLEANUP_POLICY.md`

## Common Commands

Rebuild the assistant index:

```powershell
python scripts\build_assistant_index.py
```

Export a diagnostic bundle:

```powershell
python scripts\export_diagnostics.py
```

Install and build the desktop JS app:

```powershell
cd desktop-js
npm install
npm run build
```

Check the Tauri backend:

```powershell
cd desktop-js\src-tauri
cargo check
```

Check viewer-core:

```powershell
cd tools\viewer-core
cargo check
```

Run the native viewer executable directly:

```powershell
bin\viewer-backend\litematica_native_viewer.exe "<path-to-file.litematic>" --display-mode=full
```

## Cleanup

Build outputs and caches can be regenerated. Follow `docs/CLEANUP_POLICY.md` before deleting or moving files.

Do not delete or move:

- `desktop-js/`
- `tools/viewer-core/`
- `bin/viewer-backend/`
- `data/`
- `docs/`
- `scripts/`
