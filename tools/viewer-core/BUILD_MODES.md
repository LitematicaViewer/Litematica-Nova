# viewer-core build modes

## Formal default line

- Default mode: `normal`
- Stable defaults kept on:
  - `opaque_full_cube` fast path
  - writer pipeline
  - headless prebuild
- Default mode keeps these experimental paths off unless explicitly selected:
  - compact cache v2
  - `non_occluding_full_cube`
  - `half_slab`
  - `stair_half`
  - `carpet`

## Experimental line

- Opt-in mode: `fast_experimental`
- Entry:
  - UI render page mode selector
  - env `LBA_VIEWER_BUILD_MODE=fast_experimental`
- This mode enables the current experimental cache/build switches together and stays outside the formal baseline gate.

## Layer boundaries

- Input / parse:
  - `src/nbt.rs`
  - `src/visual.rs`
  - `src/analyze.rs`
- Build:
  - `src/mesh.rs`
  - `src/build_mode.rs`
- Cache / runtime:
  - `src/storage.rs`
  - `src/native_viewer.rs`
  - `src/cli.rs`

## Performance gate

- Formal baseline cases:
  - `danjing`
  - `all_items_a`
  - `all_items_b`
- Baseline file:
  - `baselines/render_build_baseline.json`
- Check command:
  - `python tools/viewer-core/scripts/check_render_build_baseline.py <summary.tsv>`
