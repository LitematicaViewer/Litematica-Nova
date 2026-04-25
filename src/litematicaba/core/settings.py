"""应用选项持久化（JSON）。

源码运行：``<仓库>/data/settings.json``。
打包后：``<exe 所在目录>/data/settings.json``。
"""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

from litematicaba.core.config import user_data_dir

# 有效主题列表（被注释掉的主题还尚未开发完成）
VALID_THEMES = (
    "QTDefault",
    # "Glass7",
    # "Metro8",
    "Metro10",
    # "Fluent11",
    # "LightMac",
    # "Bootstrap5",
    "Minecraft",
)
DEFAULT_THEME = "QTDefault"

# NBT「完整入镜导出」启发式默认值（与 editor.js 内回退值一致）
NBT_EXPORT_FULL_MARGIN_DEFAULT = 1.22
NBT_EXPORT_FULL_PERSPECTIVE_MIN_DISTANCE_DEFAULT = 6.0
NBT_EXPORT_FULL_PERSPECTIVE_DIAG_EXTRA_DEFAULT = 0.12
NBT_EXPORT_FULL_ORTHOGRAPHIC_NEED_HALF_PADDING_DEFAULT = 1.0
NBT_EXPORT_FULL_ORTHOGRAPHIC_HEIGHT_SCALE_DEFAULT = 0.38
NBT_EXPORT_FULL_ORTHOGRAPHIC_DIAG_EXTRA_DEFAULT = 0.15
NBT_EXPORT_FULL_ORTHOGRAPHIC_MIN_DISTANCE_DEFAULT = 12.0
NBT_EXPORT_FULL_ORTHOGRAPHIC_HALF_HEIGHT_MIN_DEFAULT = 2.5
NBT_VIEWER_LARGE_STRUCTURE_THRESHOLD_DEFAULT = 48 * 48 * 48

# 性能与预加载（选项「性能与预加载」）
BLOCK_ICON_PRELOAD_STARTUP = "startup"
BLOCK_ICON_PRELOAD_ON_LITEMATIC = "on_litematic_load"
BLOCK_ICON_PRELOAD_ON_MATERIAL_OR_FLAKE = "on_material_or_flake"
BLOCK_ICON_PRELOAD_NEVER = "never"
VALID_BLOCK_ICON_PRELOAD_MODES = (
    BLOCK_ICON_PRELOAD_STARTUP,
    BLOCK_ICON_PRELOAD_ON_LITEMATIC,
    BLOCK_ICON_PRELOAD_ON_MATERIAL_OR_FLAKE,
    BLOCK_ICON_PRELOAD_NEVER,
)
DEFAULT_BLOCK_ICON_PRELOAD_MODE = BLOCK_ICON_PRELOAD_STARTUP

MATERIAL_LIST_PREWARM_ON_LITEMATIC = "on_litematic_load"
MATERIAL_LIST_PREWARM_ON_MATERIAL_LIST = "on_material_list_open"
VALID_MATERIAL_LIST_PREWARM_MODES = (
    MATERIAL_LIST_PREWARM_ON_LITEMATIC,
    MATERIAL_LIST_PREWARM_ON_MATERIAL_LIST,
)
DEFAULT_MATERIAL_LIST_PREWARM_MODE = MATERIAL_LIST_PREWARM_ON_LITEMATIC

# 材料列表扫描后端：native（Rust，高性能）/ python（litemapy，完整功能）
MATERIAL_LIST_SCAN_BACKEND_NATIVE = "native"
MATERIAL_LIST_SCAN_BACKEND_PYTHON = "python"
VALID_MATERIAL_LIST_SCAN_BACKENDS = (
    MATERIAL_LIST_SCAN_BACKEND_NATIVE,
    MATERIAL_LIST_SCAN_BACKEND_PYTHON,
)
DEFAULT_MATERIAL_LIST_SCAN_BACKEND = MATERIAL_LIST_SCAN_BACKEND_NATIVE

# 材料列表方块图标预载：主线程泵送节奏与解码位置（design / 性能选项）
BLOCK_ICON_PREWARM_DECODE_MAIN = "main"
BLOCK_ICON_PREWARM_DECODE_WORKER = "worker_experimental"
VALID_BLOCK_ICON_PREWARM_DECODE_THREADS = (
    BLOCK_ICON_PREWARM_DECODE_MAIN,
    BLOCK_ICON_PREWARM_DECODE_WORKER,
)
DEFAULT_BLOCK_ICON_PREWARM_BATCH_INTERVAL_MS = 8
DEFAULT_BLOCK_ICON_PREWARM_BATCH_COUNT = 10
DEFAULT_BLOCK_ICON_PREWARM_DECODE_THREAD = BLOCK_ICON_PREWARM_DECODE_MAIN

# 投影文件大于该字节数时在后台线程执行 ``amulet_nbt.load``，并显示可中断的进度对话框（选项「性能与预加载」）
DEFAULT_LITEMATIC_ASYNC_LOAD_MIN_BYTES = 128 * 1024
DEFAULT_PROJECTION_LIBRARY_LIMIT = 20
VALID_PROJECTION_LIBRARY_LIMITS = tuple(range(10, 101, 10))
RENDER_BUILD_MODE_NORMAL = "normal"
RENDER_BUILD_MODE_FAST_EXPERIMENTAL = "fast_experimental"
RENDER_BUILD_MODE_FULL = "full"
VALID_RENDER_BUILD_MODES = (
    RENDER_BUILD_MODE_NORMAL,
    RENDER_BUILD_MODE_FAST_EXPERIMENTAL,
    RENDER_BUILD_MODE_FULL,
)
DEFAULT_RENDER_BUILD_MODE = RENDER_BUILD_MODE_NORMAL


@dataclass
class AppSettings:
    # 界面：
    ## 主题名称
    theme_id: str = DEFAULT_THEME
    ## 显示磁贴网格
    show_tile_grid: bool = False
    ## 自动放置磁贴优先列数
    tile_auto_place_preferred_cols: int = 12
    ## 磁贴视图右侧留白
    tile_view_right_padding_px: int = 64

    # 调试：
    ## 显示UI测试入口（即将弃用；未来移动至“侧栏功能编辑”）
    show_ui_test_nav: bool = True
    ## 显示控件信息
    show_widget_inspector: bool = False
    perf_test_overlay: bool = False
    # Deepslate / WebView 渲染包：默认不在启动时联网检查；见 design §2.0.4、§2.6.3.9
    deepslate_check_updates_on_startup: bool = False
    # 3D 轨道：纵向拖拽符号；勾选后适合与鼠标默认相反的触摸屏习惯
    deepslate_invert_y: bool = False
    # vscode-nbt 同源 3D：在画布上叠加 cPos / cRot / cDist 等相机原始数据
    nbt_viewer_camera_debug: bool = False
    # NBT 3D 预览当前应用的 MC 数据资源版本 id（如 1.21.4），对应 user_data/.../nbt-viewer/<id>/mcmeta/
    nbt_mcmeta_target_version: str = ""
    # NBT 完整入镜导出（FOV>0 透视 / FOV=0 正交）相机距离启发式；经 WebView 注入 __lbaExportFullParams
    nbt_export_full_margin: float = NBT_EXPORT_FULL_MARGIN_DEFAULT
    nbt_export_full_perspective_min_distance: float = NBT_EXPORT_FULL_PERSPECTIVE_MIN_DISTANCE_DEFAULT
    nbt_export_full_perspective_diag_extra: float = NBT_EXPORT_FULL_PERSPECTIVE_DIAG_EXTRA_DEFAULT
    nbt_export_full_orthographic_need_half_padding: float = NBT_EXPORT_FULL_ORTHOGRAPHIC_NEED_HALF_PADDING_DEFAULT
    nbt_export_full_orthographic_height_scale: float = NBT_EXPORT_FULL_ORTHOGRAPHIC_HEIGHT_SCALE_DEFAULT
    nbt_export_full_orthographic_diag_extra: float = NBT_EXPORT_FULL_ORTHOGRAPHIC_DIAG_EXTRA_DEFAULT
    nbt_export_full_orthographic_min_distance: float = NBT_EXPORT_FULL_ORTHOGRAPHIC_MIN_DISTANCE_DEFAULT
    nbt_export_full_orthographic_half_height_min: float = NBT_EXPORT_FULL_ORTHOGRAPHIC_HALF_HEIGHT_MIN_DEFAULT
    # NBT 3D：触发“大结构渲染警告”前的体素数量阈值（x*y*z）
    nbt_viewer_large_structure_threshold: int = NBT_VIEWER_LARGE_STRUCTURE_THRESHOLD_DEFAULT
    # 方块图标后台预载策略；不改为清除已解码的内存缓存（切换图标包仍会按逻辑失效）。
    block_icon_preload_mode: str = DEFAULT_BLOCK_ICON_PRELOAD_MODE
    # 材料列表「整个投影」扫描时机；不改为清除磁盘材料缓存。
    material_list_prewarm_mode: str = DEFAULT_MATERIAL_LIST_PREWARM_MODE
    # 材料列表扫描后端：python (完整功能) / native_experimental (高性能，暂不支持子区域)
    material_list_scan_backend: str = DEFAULT_MATERIAL_LIST_SCAN_BACKEND
    # 方块图标预载：定时器间隔（毫秒）、每 tick 处理数量；解码在主线程或工作线程（实验性）。
    block_icon_prewarm_batch_interval_ms: int = DEFAULT_BLOCK_ICON_PREWARM_BATCH_INTERVAL_MS
    block_icon_prewarm_batch_count: int = DEFAULT_BLOCK_ICON_PREWARM_BATCH_COUNT
    block_icon_prewarm_decode_thread: str = DEFAULT_BLOCK_ICON_PREWARM_DECODE_THREAD
    # 文件大小 **大于** 该值时后台加载；否则主线程同步加载（小文件避免线程调度开销）
    litematic_async_load_min_bytes: int = DEFAULT_LITEMATIC_ASYNC_LOAD_MIN_BYTES
    # 渲染构建模式：normal 为正式线；fast_experimental 为实验线入口
    projection_library_limit: int = DEFAULT_PROJECTION_LIBRARY_LIMIT
    render_build_mode: str = DEFAULT_RENDER_BUILD_MODE
    auto_generate_projection_preview: bool = True

    def normalized(self) -> AppSettings:
        t = self.theme_id if self.theme_id in VALID_THEMES else DEFAULT_THEME
        bim = (
            self.block_icon_preload_mode
            if self.block_icon_preload_mode in VALID_BLOCK_ICON_PRELOAD_MODES
            else DEFAULT_BLOCK_ICON_PRELOAD_MODE
        )
        mlm = (
            self.material_list_prewarm_mode
            if self.material_list_prewarm_mode in VALID_MATERIAL_LIST_PREWARM_MODES
            else DEFAULT_MATERIAL_LIST_PREWARM_MODE
        )
        bdt = (
            self.block_icon_prewarm_decode_thread
            if self.block_icon_prewarm_decode_thread in VALID_BLOCK_ICON_PREWARM_DECODE_THREADS
            else DEFAULT_BLOCK_ICON_PREWARM_DECODE_THREAD
        )
        return AppSettings(
            theme_id=t,
            show_ui_test_nav=self.show_ui_test_nav,
            show_tile_grid=bool(self.show_tile_grid),
            tile_auto_place_preferred_cols=max(1, min(64, int(self.tile_auto_place_preferred_cols))),
            tile_view_right_padding_px=max(0, min(300, int(self.tile_view_right_padding_px))),
            show_widget_inspector=bool(self.show_widget_inspector),
            perf_test_overlay=bool(self.perf_test_overlay),
            deepslate_check_updates_on_startup=bool(self.deepslate_check_updates_on_startup),
            deepslate_invert_y=bool(self.deepslate_invert_y),
            nbt_viewer_camera_debug=bool(self.nbt_viewer_camera_debug),
            nbt_mcmeta_target_version=str(self.nbt_mcmeta_target_version or "").strip()[:64],
            nbt_export_full_margin=max(
                1.001, min(3.0, float(self.nbt_export_full_margin))
            ),
            nbt_export_full_perspective_min_distance=max(
                0.5, min(500.0, float(self.nbt_export_full_perspective_min_distance))
            ),
            nbt_export_full_perspective_diag_extra=max(
                0.0, min(2.0, float(self.nbt_export_full_perspective_diag_extra))
            ),
            nbt_export_full_orthographic_need_half_padding=max(
                0.0, min(50.0, float(self.nbt_export_full_orthographic_need_half_padding))
            ),
            nbt_export_full_orthographic_height_scale=max(
                0.05, min(2.0, float(self.nbt_export_full_orthographic_height_scale))
            ),
            nbt_export_full_orthographic_diag_extra=max(
                0.0, min(2.0, float(self.nbt_export_full_orthographic_diag_extra))
            ),
            nbt_export_full_orthographic_min_distance=max(
                0.5, min(500.0, float(self.nbt_export_full_orthographic_min_distance))
            ),
            nbt_export_full_orthographic_half_height_min=max(
                0.1, min(100.0, float(self.nbt_export_full_orthographic_half_height_min))
            ),
            nbt_viewer_large_structure_threshold=max(
                1_000, min(1_000_000_000, int(self.nbt_viewer_large_structure_threshold))
            ),
            block_icon_preload_mode=bim,
            material_list_prewarm_mode=mlm,
            block_icon_prewarm_batch_interval_ms=max(
                0, min(2000, int(self.block_icon_prewarm_batch_interval_ms))
            ),
            block_icon_prewarm_batch_count=max(
                1, min(500, int(self.block_icon_prewarm_batch_count))
            ),
            block_icon_prewarm_decode_thread=bdt,
            litematic_async_load_min_bytes=max(
                0, min(512 * 1024 * 1024, int(self.litematic_async_load_min_bytes))
            ),
            projection_library_limit=(
                int(self.projection_library_limit)
                if int(self.projection_library_limit) in VALID_PROJECTION_LIBRARY_LIMITS
                else DEFAULT_PROJECTION_LIBRARY_LIMIT
            ),
            render_build_mode=(
                self.render_build_mode
                if self.render_build_mode in VALID_RENDER_BUILD_MODES
                else DEFAULT_RENDER_BUILD_MODE
            ),
            auto_generate_projection_preview=bool(self.auto_generate_projection_preview),
        )


def _settings_path() -> Path:
    return user_data_dir() / "settings.json"

# 加载设置
def load_settings() -> AppSettings:
    path = _settings_path()
    if not path.is_file():
        return AppSettings()
    try:
        raw: dict[str, Any] = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return AppSettings()
    return AppSettings(
        theme_id=str(raw.get("theme_id", DEFAULT_THEME)),
        show_ui_test_nav=bool(raw.get("show_ui_test_nav", True)),
        show_tile_grid=bool(raw.get("show_tile_grid", False)),
        tile_auto_place_preferred_cols=int(raw.get("tile_auto_place_preferred_cols", 9)),
        tile_view_right_padding_px=int(raw.get("tile_view_right_padding_px", 64)),
        show_widget_inspector=bool(raw.get("show_widget_inspector", False)),
        perf_test_overlay=bool(raw.get("perf_test_overlay", False)),
        deepslate_check_updates_on_startup=bool(raw.get("deepslate_check_updates_on_startup", False)),
        deepslate_invert_y=bool(raw.get("deepslate_invert_y", False)),
        nbt_viewer_camera_debug=bool(raw.get("nbt_viewer_camera_debug", False)),
        nbt_mcmeta_target_version=str(raw.get("nbt_mcmeta_target_version", "") or ""),
        nbt_export_full_margin=float(
            raw.get("nbt_export_full_margin", NBT_EXPORT_FULL_MARGIN_DEFAULT)
        ),
        nbt_export_full_perspective_min_distance=float(
            raw.get(
                "nbt_export_full_perspective_min_distance",
                NBT_EXPORT_FULL_PERSPECTIVE_MIN_DISTANCE_DEFAULT,
            )
        ),
        nbt_export_full_perspective_diag_extra=float(
            raw.get(
                "nbt_export_full_perspective_diag_extra",
                NBT_EXPORT_FULL_PERSPECTIVE_DIAG_EXTRA_DEFAULT,
            )
        ),
        nbt_export_full_orthographic_need_half_padding=float(
            raw.get(
                "nbt_export_full_orthographic_need_half_padding",
                NBT_EXPORT_FULL_ORTHOGRAPHIC_NEED_HALF_PADDING_DEFAULT,
            )
        ),
        nbt_export_full_orthographic_height_scale=float(
            raw.get(
                "nbt_export_full_orthographic_height_scale",
                NBT_EXPORT_FULL_ORTHOGRAPHIC_HEIGHT_SCALE_DEFAULT,
            )
        ),
        nbt_export_full_orthographic_diag_extra=float(
            raw.get(
                "nbt_export_full_orthographic_diag_extra",
                NBT_EXPORT_FULL_ORTHOGRAPHIC_DIAG_EXTRA_DEFAULT,
            )
        ),
        nbt_export_full_orthographic_min_distance=float(
            raw.get(
                "nbt_export_full_orthographic_min_distance",
                NBT_EXPORT_FULL_ORTHOGRAPHIC_MIN_DISTANCE_DEFAULT,
            )
        ),
        nbt_export_full_orthographic_half_height_min=float(
            raw.get(
                "nbt_export_full_orthographic_half_height_min",
                NBT_EXPORT_FULL_ORTHOGRAPHIC_HALF_HEIGHT_MIN_DEFAULT,
            )
        ),
        nbt_viewer_large_structure_threshold=int(
            raw.get(
                "nbt_viewer_large_structure_threshold",
                NBT_VIEWER_LARGE_STRUCTURE_THRESHOLD_DEFAULT,
            )
        ),
        block_icon_preload_mode=str(
            raw.get("block_icon_preload_mode", DEFAULT_BLOCK_ICON_PRELOAD_MODE)
        ),
        material_list_prewarm_mode=str(
            raw.get("material_list_prewarm_mode", DEFAULT_MATERIAL_LIST_PREWARM_MODE)
        ),
        material_list_scan_backend=str(
            raw.get("material_list_scan_backend", DEFAULT_MATERIAL_LIST_SCAN_BACKEND)
        ),
        block_icon_prewarm_batch_interval_ms=int(
            raw.get(
                "block_icon_prewarm_batch_interval_ms",
                DEFAULT_BLOCK_ICON_PREWARM_BATCH_INTERVAL_MS,
            )
        ),
        block_icon_prewarm_batch_count=int(
            raw.get(
                "block_icon_prewarm_batch_count",
                DEFAULT_BLOCK_ICON_PREWARM_BATCH_COUNT,
            )
        ),
        block_icon_prewarm_decode_thread=str(
            raw.get(
                "block_icon_prewarm_decode_thread",
                DEFAULT_BLOCK_ICON_PREWARM_DECODE_THREAD,
            )
        ),
        litematic_async_load_min_bytes=int(
            raw.get(
                "litematic_async_load_min_bytes",
                DEFAULT_LITEMATIC_ASYNC_LOAD_MIN_BYTES,
            )
        ),
        projection_library_limit=int(
            raw.get("projection_library_limit", DEFAULT_PROJECTION_LIBRARY_LIMIT)
        ),
        render_build_mode=str(
            raw.get("render_build_mode", DEFAULT_RENDER_BUILD_MODE)
        ),
        auto_generate_projection_preview=bool(raw.get("auto_generate_projection_preview", True)),
    ).normalized()


def save_settings(settings: AppSettings) -> None:
    path = _settings_path()
    data = asdict(settings.normalized())
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
