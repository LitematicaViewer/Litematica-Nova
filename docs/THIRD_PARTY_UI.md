# 第三方 UI 组件来源

核查日期：2026-10-12。主题接入采用源码由项目维护（source-owned）的方式；下列上游是组件来源，不是 Nova 已获得上游认证的声明。

| 来源 | 官方入口与取用位置 | 许可证 |
| --- | --- | --- |
| shadcn/ui | [官网](https://ui.shadcn.com)、官方 `new-york-v4` registry，例如 [button.json](https://ui.shadcn.com/r/styles/new-york-v4/button.json)；[源码仓库](https://github.com/shadcn-ui/ui) | [MIT](https://github.com/shadcn-ui/ui/blob/main/LICENSE.md)，Copyright (c) 2023 shadcn |
| 8bitcn/ui | [官网](https://www.8bitcn.com)、官方 registry，例如 [button.json](https://www.8bitcn.com/r/button.json)；[源码仓库](https://github.com/TheOrcDev/8bitcn-ui) | [MIT](https://github.com/TheOrcDev/8bitcn-ui/blob/main/license.md)，Copyright (c) 2025 8bitcn |

## 本项目适配约束

- 保持 React 18：只按现有工程调整导入、ref 和样式接入，不为引入主题升级业务架构。
- 三主题共享同一组件树与操作逻辑，通过语义设计令牌和组件变体切换；均支持浅色、深色、跟随系统。Minecraft 保留既有字体、纹理按钮和物品栏资源，在共享控件上适配明暗，不复制三套页面。
- 源码组件可按桌面应用需要适配，不把官方示例代码未经验证的功能当作 Nova 已实现能力。
- 字体使用本地资源或系统字体，不在运行时加载 Google Fonts 等外部字体服务。
- 下拉框、弹窗及方块搜索使用现有 `radix-ui` 依赖；方块搜索按官方 [Popover 文档](https://www.radix-ui.com/primitives/docs/components/popover) 使用 Portal 与可用空间变量，避免被页面滚动容器裁剪。
- 修改上游源码时保留来源与版权信息；后续更新应对照官方 registry，而不是只按主题名称重新仿制外观。

本轮取用的共享控件：

| 控件 | 来源与本地适配 |
| --- | --- |
| Button、Input、Dialog、Select、Table | shadcn/ui 源码与已有 8bitcn/ui 对应组件；适配 React 18、现有事件接口及主题令牌 |
| Checkbox | [shadcn registry](https://ui.shadcn.com/r/styles/new-york-v4/checkbox.json)、[8bitcn registry](https://www.8bitcn.com/r/checkbox.json)，由 Radix 处理勾选与不确定状态 |
| Textarea、RadioGroup、Slider、ScrollArea | shadcn/ui 官方 `new-york-v4` registry；本地分别维护 `textarea.tsx`、`radio-group.tsx`、`slider.tsx`、`scroll-area.tsx`，统一主题状态与滚动条 |

这些是实际组件替换，不是给三套业务页面换色。投影库卡片、导航和材料列表属于 Nova 的共享功能组合；原生 Viewer 不属于这些 Web 控件，也不宣称通过此接入获得 DOM 内嵌渲染能力。

## MIT 许可文本

以下许可适用于上述引用的组件源码。版权分别归原作者所有：

```text
MIT License

Copyright (c) 2023 shadcn
Copyright (c) 2025 8bitcn

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```
