# Cleanup Policy

This document defines low-risk disk cleanup rules for maintainers. It is about generated files, caches, and reversible quarantine only. It is not a refactor plan.

## Current UI status

- The only active desktop UI is `desktop-js/`.
- The old PySide6 package `src/litematicaba/` has been deleted.
- The old alternate UI directory `desktop-ui/` has been deleted.
- The old script directory `script/` has been deleted.
- The active tooling directory is `scripts/`; do not confuse it with the deleted `script/` directory.

## Safe to delete

These are generated or cache outputs and can be removed when reclaiming disk space:

- `desktop-js/node_modules/`
- `desktop-js/dist/`
- `desktop-js/src-tauri/target/`
- `tools/viewer-core/target/`
- `.tmp/`
- `diagnostics/bundle_*`
- `__pycache__/`, `.pytest_cache/`, `.mypy_cache/`, `.ruff_cache/`, `.cache/`
- `.vite/`, `.turbo/`, `.next/`, `coverage/`, `build/`
- `*.tmp`, `*.temp`

Keep `diagnostics/README.md`; only remove generated `bundle_*` directories.

## Do not delete or move

Do not delete or move these during disk cleanup:

- `desktop-js/`
- `tools/viewer-core/`
- `data/projection-library/`
- `data/ai-projection/`
- `data/minecraft_blockstates/overrides/`
- current BlockState DB files unless the task is explicitly DB regeneration
- `bin/viewer-backend/`
- `docs/`
- `scripts/`
- `MAINTAINERS.md`
- `README.md`, package manifests, Cargo manifests, Tauri config
- projection library index/schema files
- AI prompt/config, validator, normalizer, or apply bridge source
- viewer-core mesh, full mode, native viewer, and cache protocol source

The deleted legacy paths above should be restored only from the cleanup backup if a maintainer explicitly needs historical comparison.

## Quarantine only

If a file looks like a debug artifact or fixture but might still be useful for regression work, move it outside the repository instead of deleting it.

Use:

```text
C:\Users\27232\CodexBackups\<cleanup_backup>\quarantine\
```

Quarantine candidates:

- top-level `_full_mode_*`
- top-level `_verify_*`
- top-level `debug`, `trace`, `probe`, `fixture`, `baseline`, `repro`, or `test` artifacts
- top-level `.png`, `.log`, and loose `.litematic` samples not in `data/projection-library/`
- uncertain screenshots, JSON files, or generated layouts

Before quarantine, record the original path and reference scan result in `cleanup_manifest.json`.

## Restore from quarantine

To restore one file, copy it back to the same relative path from:

```text
C:\Users\27232\CodexBackups\<cleanup_backup>\quarantine\<relative-path>
```

Example:

```powershell
Copy-Item "C:\Users\27232\CodexBackups\<cleanup_backup>\quarantine\_verify_example.png" "C:\Users\27232\Documents\Litematica-BA\_verify_example.png"
```

To restore everything, copy the quarantine directory contents back into the repository root, preserving relative paths.

## Reinstall and rebuild

After deleting `node_modules`:

```powershell
cd C:\Users\27232\Documents\Litematica-BA\desktop-js
npm install
```

Rebuild the desktop frontend:

```powershell
cd C:\Users\27232\Documents\Litematica-BA\desktop-js
npm run build
```

Recreate Rust build outputs:

```powershell
cd C:\Users\27232\Documents\Litematica-BA\desktop-js\src-tauri
cargo check

cd C:\Users\27232\Documents\Litematica-BA\tools\viewer-core
cargo build --release --bin litematica_core
cargo build --release --bin litematica_native_viewer
```

## Regenerate diagnostics

```powershell
cd C:\Users\27232\Documents\Litematica-BA
python scripts\export_diagnostics.py
```

## Required cleanup records

Every destructive cleanup should create:

- `cleanup_manifest.json`
- `cleanup_report.md`
- a workspace-external backup under `C:\Users\27232\CodexBackups\`
- `BACKUP_MANIFEST.json`, backup log, and `cleanup_plan_before.json` inside the backup

Do not put backups or quarantine directories inside the repository.
