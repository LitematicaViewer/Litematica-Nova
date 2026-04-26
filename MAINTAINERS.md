# Litematica-BA Maintainer Entry

This file is a maintenance entry point. It is not product documentation.

## Current mainline

- Active desktop UI: `desktop-js/`.
- Active Rust core and native viewer: `tools/viewer-core/`.
- Runtime viewer binaries used by the app: `bin/viewer-backend/`.
- Main data/config area: `data/`.
- Removed legacy paths: `src/litematicaba/`, `desktop-ui/`, and `script/`.
- `scripts/` remains the active maintenance tooling directory.

## Read first

1. `docs/PROJECT_MAP.md` - project map and current mainline.
2. `docs/FEATURE_ENTRYPOINTS.md` - feature entry points and call chains.
3. `docs/DATA_FLOW.md` - data lifecycle and generated/cache areas.
4. `docs/MODULE_BOUNDARIES.md` - safe edit zones and regression requirements.
5. `docs/DIAGNOSTICS.md` - diagnostic bundle export and reading guide.

## Common commands

```powershell
python scripts\build_assistant_index.py
python scripts\query_assistant_index.py "desktop-js"
python scripts\export_diagnostics.py

cd desktop-js
npm run build

cd src-tauri
cargo check
```

## Do not hand-edit

- Generated BlockState DB outputs under `data/minecraft_blockstates/*.json`; update overrides/scripts and regenerate instead.
- Diagnostic bundles under `diagnostics/bundle_*`; regenerate with `python scripts\export_diagnostics.py`.
- Build output such as `desktop-js/dist/`, `desktop-js/node_modules/`, and Rust `target/` directories.
- Runtime binaries in `bin/viewer-backend/` unless they are copied from a verified `tools/viewer-core` release build.
- Cache files and manifests unless the task is explicitly cache maintenance.

## Cleanup / Disk Space

For disk cleanup, follow `docs/CLEANUP_POLICY.md`. Build/cache outputs may be deleted, but uncertain debug fixtures and loose root-level regression artifacts should be moved to a workspace-external quarantine instead of being permanently deleted. The old PySide6 UI, `desktop-ui/`, and old `script/` tree have already been removed.

## AI/Codex maintenance workflow

1. Read this file and the docs listed above.
2. Rebuild/query the assistant index before broad code search.
3. Locate the feature entry point before editing.
4. Keep UI/service/core boundaries intact.
5. Do not change viewer-core mesh, full mode, cache protocol, AI plan semantics, or blockstate generation while fixing unrelated issues.
6. Run the smallest relevant regression checks, plus build/check commands when paths or shared contracts change.

## Top-level cleanup notes

- The repository currently contains many root-level debug fixtures, screenshots, and logs from renderer investigations. Treat them as generated/debug artifacts unless a doc or test references them.
- No core directory should be moved during low-risk structure cleanup: keep `desktop-js/`, `tools/viewer-core/`, `data/`, and `bin/viewer-backend/` stable.
- For uncertain areas, add local README notes first; do not perform large moves or deletes.
