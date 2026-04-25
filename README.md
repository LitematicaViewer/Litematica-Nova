# Litematica Blueprint Assistant

Litematica Blueprint Assistant 是一个面向 Minecraft `.litematic` 投影文件的本地查看、分析与渲染工具。项目包含 Python 桌面界面、Rust 原生 viewer backend，以及用于 Full Mode V2 渲染一致性验证的 fixture 工具。

## 当前重点

- 读取 `.litematic` 投影并解析 NBT / block state / region 数据。
- 提供材料统计、投影查看和 Full Mode 渲染预览。
- Full Mode V2 使用 Rust viewer-core 解析 blockstate / model / texture，并逐步按 block family 收口渲染 fidelity。
- 当前保留 piston family 的专用调试 fixture 与可视化编号模式，用于继续定位 `piston` / `sticky_piston` / `piston_head` 的细节问题。

## 目录结构

```text
bin/viewer-backend/        已构建的 viewer backend 可执行文件
desktop-ui/                桌面 UI 相关资源
scripts/                   fixture 与辅助脚本
src/litematicaba/          Python 应用代码
third_party/render-assets/ Full Mode V2 使用的渲染资源
tools/viewer-core/         Rust viewer-core 源码
_full_mode_piston*.litematic
_full_mode_piston*_layout.txt
                            piston 调试投影与说明文件
```

## 运行桌面应用

首次准备开发环境：

```powershell
cd <repo>
.\install.bat
```

默认会把开发环境建在工作区外：

- `%LOCALAPPDATA%\Litematica-BA\dev-env\venv`
- `%LOCALAPPDATA%\Litematica-BA\dev-env\pip-cache`

如需覆盖路径，可在运行前设置：

```powershell
$env:LBA_DEV_HOME='D:\LBA\dev-env'
```

准备好后启动桌面应用：

```powershell
cd <repo>
python -m litematicaba
```

也可以使用根目录的启动脚本：

```powershell
cd <repo>
.\launch_ba_ui.bat
```

`bin\dev-win-qt.bat` 也会自动复用同一套工作区外开发环境。

## 运行 Full Mode V2 Viewer

打开任意 `.litematic`：

```powershell
cd <repo>
.\bin\viewer-backend\litematica_native_viewer.exe '<path-to-file.litematic>' --chunk-size=32 --display-mode=full
```

导出预览图：

```powershell
cd <repo>
.\bin\viewer-backend\litematica_native_viewer.exe '<path-to-file.litematic>' --chunk-size=32 --display-mode=full --preview-output='<output.png>' --auto-exit-seconds=4
```

## Piston Debug Fixture

项目保留两套 piston fixture：

- `_full_mode_piston_fixture.litematic`
- `_full_mode_piston_complex_fixture.litematic`

复杂 fixture 的说明文件：

- `_full_mode_piston_complex_fixture_layout.txt`

复杂 fixture 按区域组织：

- `Bxx`: base 单独展示区
- `Hxx`: head 单独展示区
- `Cxx`: base + head 配对区
- `Mxx`: mixed 组合区

开启 piston debug 编号模式：

```powershell
cd <repo>
$env:LBA_FULL_MODE_V2_PISTON_DEBUG='1'
.\bin\viewer-backend\litematica_native_viewer.exe '.\_full_mode_piston_complex_fixture.litematic' --chunk-size=32 --display-mode=full
```

关闭 debug 编号模式：

```powershell
cd <repo>
$env:LBA_FULL_MODE_V2_PISTON_DEBUG=$null
.\bin\viewer-backend\litematica_native_viewer.exe '.\_full_mode_piston_complex_fixture.litematic' --chunk-size=32 --display-mode=full
```

重新生成 complex fixture：

```powershell
cd <repo>
python .\scripts\generate_piston_complex_fixture.py
```

## Rust Backend 构建

```powershell
cd <repo>\tools\viewer-core
cargo check
cargo build --release
```

同步 release 构建到运行目录：

```powershell
cd <repo>
Copy-Item .\tools\viewer-core\target\release\litematica_native_viewer.exe .\bin\viewer-backend\litematica_native_viewer.exe -Force
Copy-Item .\tools\viewer-core\target\release\litematica_core.exe .\bin\viewer-backend\litematica_core.exe -Force
```

## 调试约定

- 正常运行保持安静。
- piston family 调试通过 `LBA_FULL_MODE_V2_PISTON_DEBUG=1` 开启。
- 临时日志、预览图和本地大场景验证产物不作为项目固定资产提交。
- fixture、layout 和生成脚本保留，用于复现和人工反馈。

## 开发注意

- Full Mode V2 的 block family 修复应尽量收在对应 family 的 typed 输出层。
- 不要为了单个问题随意改 shared UV / shared rotation / shared transform / shared culling。
- `moving_piston` 与 static piston family 分开处理。
- 修改渲染行为后，优先用 fixture 验证，再看真实大场景。


## 功能列表
此处列出项目已实现/规划中的功能。若要查看完整的设计细节，请移步[至此](docs/design.md)

### 投影库
投影文件资源管理器，搜集离散分布的文件并统一管理
- [ ] 基本实现
- [ ] 从外部来源获取投影
### 属性
读取并解析文件内的SNBT数据，并提供合适的修改选项
- [x] 导入投影文件
- [x] 解析并读取SNBT数据
	- [x] 文字数据（读写）：内部名称，作者，描述
	- [x] 格式数据（只读）：时间、尺寸、体积、版本
	- [x] 图片数据（读写）：预览图（数组）
	- [x] 分块数据（只读）：区域
- [ ] 版本转换

### 统计
根据投影内容进行统计分析，以便快速判断投影属性
- [x] 源项目统计学指标
	- [x] 偏度：红石、液体
	- [x] 统计分类
	- [x] 密度
- [ ] 方块分类统计饼图
- [ ] 方块统计饼图
- [ ] 生存模式实用指标
	- [ ] 稀缺材料需求标签：如：粘液块、石英、珊瑚
	- [ ] 特性标签：如：TNT复制

### 分层
自定义Y层级显示平面图
- [ ] 方块状态叠加显示
- [ ] 方块实体详情窗格
	- [ ] 容器内容分析
	- [ ] 告示牌内容分析
- [ ] 简单编辑功能

### 渲染
以3D的形式渲染投影结构
- [ ] 嵌入式插件渲染（[参考项目](https://github.com/misode/vscode-nbt)）
	- [x] 基本实现
	- [ ] 区域拆分
	- [x] 热更新并替换游戏资源
- [ ] 自建deepslate渲染
	- [x] 基本实现
	- [x] 区域拆分
	- [ ] 热更新并替换游戏资源
- [x] 预设相机方向
- [x] 自定义视场角（透视/正交）
- [x] 截取并输出图像
- [x] 渲染完整入镜模型并输出图像
	- [x] 计算重心：相机距离

### 替换
快速替换/限制投影里的不同方块
- [ ] 格式化替换脚本

### 材料列表
列出选定区域或层级所包含的方块数量
- [x] 单位统计（盒数、组数、余数）
- [x] 表项悬浮窗
- [x] 多工作簿共存
- [x] 导出列表数据
	- [x] `.csv` 逗号分隔值文件
	- [x] `.txt` ASCII艺术表
- [x] 导入并解析列表数据
	- [x] 读取由litematica模组导出的 `.csv` 逗号分隔值文件
- [x] Minecraft 方块物品图标

### 用户界面
- [ ] 主题
	- [x] Metro10：类似Windows 10界面的主题
	- [x] Minecraft：类似Minecraft界面的主题
	- [ ] Bulletin Azure：类似Blue Archive界面的主题
		- [ ] Bulletin意为”简报、档案“；Azure意为”蔚蓝色“
	- [ ] 。。。

### 枚举器
提供或自定义预设的筛选枚举
- [ ] 游戏数据值总表获取
- [ ] 拖动分类功能

### 简易几何投影生成
快速生成指定投影结构
- [ ] 常规立方体
- [ ] 球体
- [ ] 自定义曲线/曲面方程

