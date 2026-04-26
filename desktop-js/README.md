# Litematica BA JS Desktop UI

这是项目并行的全新 JS/Tauri 前端 UI 壳，基于 React + TypeScript + Vite 构建。
它的主要目标是提供一个更加现代化、轻量的前端界面，同时完美复用已有的 Rust `viewer-core`。

## 主要特点
- **当前主线**: JS/Tauri UI 是当前唯一桌面 UI。旧 PySide6 UI 已删除，不再作为平行入口维护。
- **极简主题**: 删繁就简，仅保留原汁原味的 Metro10 和 Minecraft 主题，保证清晰的交互体验。
- **无缝后端**: 通过 Tauri Command 直接拉起 `bin/viewer-backend/` 下的已编译 exe，无任何侵入性修改。

## 安装与开发
1. 进入目录:
   ```powershell
   cd desktop-js
   ```
2. 安装依赖:
   ```powershell
   npm install
   ```
3. 启动开发模式:
   ```powershell
   npm run tauri dev
   ```

## 构建
如果需要构建独立可执行文件:
```powershell
npm run tauri build
```

## 当前完成状态
- [x] 主题系统 (Metro10, Minecraft)
- [x] 投影文件选择 (Library Page)
- [x] 基础属性查看 (Properties Page)
- [x] 方块替换页 (完整闭环: Dry-run -> Apply -> 本地化 DB)
- [x] 启动外部 native_viewer
- [ ] 独立的材质管理器
