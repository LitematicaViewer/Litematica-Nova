# 方块颜色缓存

`data/blockColorCache.json` 是 `normal` 和 `fast_experimental` viewer 模式使用的简化颜色来源。

生成命令：

```powershell
python scripts\generate_block_color_cache.py
```

如果自动发现 Minecraft 客户端资源失败，可以显式传入 jar：

```powershell
python scripts\generate_block_color_cache.py --jar "D:\.minecraft\versions\1.21-SurvivalRedstonePower\1.21-SurvivalRedstonePower.jar"
```

生成脚本读取：

- `assets/minecraft/blockstates/*.json`
- `assets/minecraft/models/block/*.json`
- `assets/minecraft/textures/block/*.png`

脚本会沿着 blockstate -> model -> texture 引用链，采样非透明像素，并对草、树叶、水做简化的原版 tint 处理，最后写出代表性 RGB。

viewer 读取顺序：

1. 完整 blockstate key，例如 `minecraft:oak_log[axis=y]`
2. 方块 id，例如 `minecraft:oak_log`
3. 本地 id，例如 `oak_log`
4. 内置材质 fallback
5. `DEFAULT_BLOCK_COLOR`

Full Mode V2 不使用这个文件；完整模式继续走运行时 texture atlas。

