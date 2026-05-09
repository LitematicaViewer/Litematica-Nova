# desktop-nova 状态

本文记录 `desktop-nova` 当前状态，避免后续维护时把历史入口、本地参考仓库和新主线混在一起。

## 当前方向

- `desktop-nova/` 是新的桌面端主线和功能基准。
- `.external/Litematica-Nova-rpr-ba` 只作为 Nova 视觉和主题参考，不上传。
- 历史桌面入口已从 Git 跟踪中排除，不再作为维护入口。
- 后端能力通过 `desktop-nova` 的 service/adapter 调用 `bin/viewer-backend/` 里的可执行文件。
- 用户态数据默认写入 `%AppData%\Litematica-BA\desktop-nova`。

## 已接入能力

- Tauri/React 应用壳。
- Nova 四套主题和主题持久化。
- AppData 用户配置目录。
- AppData 投影库 `projection-library/js_library.json`。
- 本地 `.litematic` 选择、analyze、入库。
- RedenMC 在线投影库：
  - 搜索；
  - 详情；
  - 普通附件下载；
  - 参数化下载尺寸校验；
  - 下载到 AppData；
  - 下载后 analyze；
  - 加入投影库。
- 属性页：
  - metadata 基础展示和编辑；
  - 保存、另存、恢复；
  - 区域名编辑；
  - 弹窗 viewer 入口。
- 统计和材料：
  - 结构统计；
  - 整个投影、区域、层级材料列表；
  - 可选统计容器内物品；
  - CSV 导出，UTF-8 BOM，列为 `名称,数字,统计数据`。
- 渲染页：
  - 渲染模式选择；
  - 3D cache 构建；
  - progress 轮询；
  - 静态预览；
  - 嵌入 viewer；
  - 弹窗 viewer；
  - 切页隐藏嵌入式 native 子窗口。
- 分层页：
  - 读取真实 cache/layer 数据。
- 替换页：
  - From/To 方块；
  - dry-run/apply。
- 生成页：
  - 模板/plan 生成；
  - dry-run/apply；
  - AI plan 导入；
  - API 对话入口。
- 选项页：
  - 主题选择；
  - AppData 配置目录；
  - 后端和数据检查；
  - 预览模式；
  - AI provider/base URL/model/API key 设置。

## 后端契约

UI 调用以下运行时二进制：

- `bin/viewer-backend/litematica_core.exe`
- `bin/viewer-backend/litematica_native_viewer.exe`

修改 `tools/viewer-core` 后，需要重新构建并复制对应 exe。

常规检查：

```powershell
cd desktop-nova
npm run build

cd src-tauri
cargo check

cd ..\..\tools\viewer-core
cargo fmt
cargo check
cargo test
cargo build --release --bin litematica_core
Copy-Item target\release\litematica_core.exe ..\..\bin\viewer-backend\litematica_core.exe -Force
```

如果修改了原生 viewer，还要构建并同步：

```powershell
cargo build --release --bin litematica_native_viewer
Copy-Item target\release\litematica_native_viewer.exe ..\..\bin\viewer-backend\litematica_native_viewer.exe -Force
```

## 未完成或需要继续验收

- 部分页面仍需要完整 UTF-8 中文清理。
- 部分页面仍需要视觉审计，去掉旧 inline style 对 Nova 视觉的干扰。
- 属性页嵌入式预览还没有完整恢复；目前弹窗/static preview 路径更稳。
- RedenMC 外部网盘/HTML 附件会按真实错误拒绝导入，不应伪装成 `.litematic`。
- RedenMC 本地缓存还没有长期离线索引和失效策略。
- 容器内物品统计已支持常见 NBT 结构和递归 `Items`，但还需要更多真实 fixture 验证箱子、潜影盒、漏斗、熔炉、酿造台、营火、饰纹陶罐和 mod 容器。
- 材料导出目前只有 CSV；TSV/TXT 和可配置单位还没做。
- 分层页高级 hover、BlockEntity 详情面板未完成。
- 嵌入式 viewer 是 Windows HWND 路径，仍对 DPI、bounds、页面可见性、native 进程状态敏感。
- cache 持久化策略还没最终确定，旧状态可能指向缺失或不兼容 cache。
- AI provider 目前只保证当前 mock/OpenAI-compatible 路径；其它 provider 不视为稳定。
- AI plan 导入只填表，不自动 dry-run/apply。
- API Key 不得进入 localStorage、普通配置、日志、prompt 或文档。

## 工作区规则

- 保留 `desktop-nova/`，它是当前 UI 主线。
- 不提交 `.external/`。
- 不提交 AppData 下载物。
- 不提交本地验证 PNG。
- 不手改生成出的 exe；从 Rust release build 复制。
- 不清理无关 warning，不回退无关改动。

