"""性能测试：透明洋红圆横穿主窗体 + 左下角 FPS 浮层（鼠标穿透）。"""

from __future__ import annotations

from time import perf_counter

from PySide6.QtCore import QPoint, Qt, QTimer
from PySide6.QtGui import QColor, QFont, QPainter, QPen
from PySide6.QtWidgets import QLabel, QWidget


class PerfTestCircleOverlay(QWidget):
    """全客户区覆盖，仅绘制移动的半透明圆形。"""

    def __init__(self, host: QWidget) -> None:
        super().__init__(host)
        self._radius = 28
        self._phase_start = perf_counter()
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setFocusPolicy(Qt.FocusPolicy.NoFocus)

    def restart_animation(self) -> None:
        self._phase_start = perf_counter()

    def paintEvent(self, event) -> None:  # noqa: ARG002
        w, h = self.width(), self.height()
        if w <= 0 or h <= 0:
            return
        cycle_s = 3.5
        elapsed = perf_counter() - self._phase_start
        t = (elapsed % cycle_s) / cycle_s
        cy = h // 2
        span = max(0.0, float(w - 2 * self._radius))
        cx = int(self._radius + t * span)
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        fill = QColor("#ff00ff")
        fill.setAlpha(100)
        p.setBrush(fill)
        pen = QPen(QColor("#ff00ff"))
        pen.setWidth(2)
        p.setPen(pen)
        p.drawEllipse(QPoint(cx, cy), self._radius, self._radius)


class PerfTestHud(QWidget):
    """左下角统计浮层。"""

    def __init__(self, host: QWidget) -> None:
        super().__init__(host)
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        self._label = QLabel(self)
        self._label.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
        font = QFont("Consolas", 9)
        if not font.exactMatch():
            font = QFont("Courier New", 9)
        self._label.setFont(font)
        self._label.setStyleSheet(
            "color: #eeeeee; background-color: rgba(0,0,0,170); "
            "padding: 6px 8px; border-radius: 4px;"
        )
        self._label.setText("…")

    def set_stats_text(self, text: str) -> None:
        self._label.setText(text)
        self._label.adjustSize()
        self.resize(self._label.size())

    def sync_position(self) -> None:
        host = self.parentWidget()
        if host is None:
            return
        margin = 8
        self.move(margin, max(0, host.height() - self.height() - margin))


class PerfTestController:
    """由主窗口持有：按设置启停。"""

    def __init__(self, host: QWidget) -> None:
        self._host = host
        self._overlay = PerfTestCircleOverlay(host)
        self._overlay.hide()
        self._hud = PerfTestHud(host)
        self._hud.hide()
        self._timer = QTimer(host)
        self._timer.setInterval(16)
        self._timer.timeout.connect(self._tick)
        self._enabled = False
        self._last_frame = perf_counter()
        self._win_start = perf_counter()
        self._accum_frames = 0
        self._accum_dt = 0.0
        self._last_report = perf_counter()

    def set_enabled(self, on: bool) -> None:
        if on == self._enabled:
            return
        self._enabled = on
        if on:
            self._last_frame = perf_counter()
            self._win_start = perf_counter()
            self._last_report = perf_counter()
            self._accum_frames = 0
            self._accum_dt = 0.0
            self._overlay.restart_animation()
            self._sync_geometry()
            self._overlay.show()
            self._hud.show()
            self._overlay.raise_()
            self._hud.raise_()
            self._timer.start()
        else:
            self._timer.stop()
            self._overlay.hide()
            self._hud.hide()

    def sync_geometry(self) -> None:
        if not self._enabled:
            return
        self._sync_geometry()

    def _sync_geometry(self) -> None:
        self._overlay.setGeometry(self._host.rect())
        self._hud.sync_position()
        self._overlay.raise_()
        self._hud.raise_()

    def _tick(self) -> None:
        if not self._enabled:
            return
        now = perf_counter()
        dt = now - self._last_frame
        self._last_frame = now
        self._accum_frames += 1
        self._accum_dt += dt

        inst_ms = dt * 1000.0
        inst_fps = 1.0 / dt if dt > 1e-6 else 0.0

        span = now - self._last_report
        if span >= 0.5 and self._accum_frames > 0:
            avg_fps = self._accum_frames / span
            avg_ms = (self._accum_dt / self._accum_frames) * 1000.0
            run_s = now - self._win_start
            self._hud.set_stats_text(
                f"性能测试\n"
                f"FPS（约 0.5s 均值）: {avg_fps:.1f}\n"
                f"瞬时 FPS: {inst_fps:.1f}\n"
                f"帧时间: {inst_ms:.2f} ms（均值 {avg_ms:.2f} ms）\n"
                f"运行: {run_s:.1f} s"
            )
            self._last_report = now
            self._accum_frames = 0
            self._accum_dt = 0.0
        elif self._accum_frames == 1:
            self._hud.set_stats_text(
                f"性能测试\n"
                f"瞬时 FPS: {inst_fps:.1f}\n"
                f"帧时间: {inst_ms:.2f} ms\n"
                f"采集中…"
            )

        self._hud.sync_position()
        self._overlay.update()
        self._overlay.raise_()
        self._hud.raise_()
