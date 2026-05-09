# viewer-core 构建模式

## 正式默认线

- 默认模式：`normal`
- 默认启用：
  - `opaque_full_cube` 快速路径；
  - writer pipeline；
  - headless prebuild。
- 默认不启用实验路径，除非用户明确选择：
  - compact cache v2；
  - `non_occluding_full_cube`；
  - `half_slab`；
  - `stair_half`；
  - `carpet`。

## 实验线

- 可选模式：`fast_experimental`
- 入口：
  - desktop-nova 渲染页模式选择；
  - 环境变量 `LBA_VIEWER_BUILD_MODE=fast_experimental`。
- 该模式会组合启用当前实验 cache/build 开关，不属于正式默认基线。

## 层级边界

- 输入和解析：
  - `src/nbt.rs`
  - `src/visual.rs`
  - `src/analyze.rs`
- 构建：
  - `src/mesh.rs`
  - `src/build_mode.rs`
- cache 和运行时：
  - `src/storage.rs`
  - `src/native_viewer.rs`
  - `src/cli.rs`

## 性能门槛

修改构建模式或 cache 路径后至少运行：

```powershell
cd tools\viewer-core
cargo fmt
cargo check
cargo test
```

如果修改影响运行时 exe，需要重新构建并复制到：

```text
bin/viewer-backend/
```

