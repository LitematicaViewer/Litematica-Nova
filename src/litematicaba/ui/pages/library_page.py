from __future__ import annotations

from datetime import datetime
from pathlib import Path

from PySide6.QtCore import QEasingCurve, QMimeData, QPoint, QPropertyAnimation, QTimer, Qt, QThread, QUrl, Signal
from PySide6.QtGui import QDrag, QPixmap
from PySide6.QtWidgets import (
    QAbstractItemView,
    QComboBox,
    QDialog,
    QFileDialog,
    QFormLayout,
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QMessageBox,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

from litematicaba.core.projection_library import (
    ProjectionLibraryEntry,
    ProjectionLibraryError,
    ProjectionLibraryStore,
    ProjectionValidation,
)
from litematicaba.core.projection_preview import (
    ProjectionPreviewError,
    ProjectionPreviewRequest,
    generate_projection_preview,
)
from litematicaba.core.settings import (
    AppSettings,
    DEFAULT_PROJECTION_LIBRARY_LIMIT,
    VALID_PROJECTION_LIBRARY_LIMITS,
    save_settings,
)


def _format_size(num_bytes: int) -> str:
    value = float(max(0, num_bytes))
    units = ("B", "KB", "MB", "GB")
    index = 0
    while value >= 1024.0 and index < len(units) - 1:
        value /= 1024.0
        index += 1
    return f"{int(value)} {units[index]}" if index == 0 else f"{value:.1f} {units[index]}"


def _format_time(raw: str) -> str:
    try:
        return datetime.fromisoformat(raw).strftime("%Y-%m-%d %H:%M:%S")
    except ValueError:
        return raw or "-"


class ProjectionEditDialog(QDialog):
    def __init__(self, entry: ProjectionLibraryEntry, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle("更改投影属性")
        self.resize(620, 360)

        layout = QVBoxLayout(self)
        form = QFormLayout()
        self._display_name_edit = QLineEdit(entry.display_name)
        self._original_path_edit = QLineEdit(entry.original_file_path)
        self._note_edit = QLineEdit(entry.note)
        self._internal_name_edit = QLineEdit(entry.internal_name)
        self._author_edit = QLineEdit(entry.author)
        self._description_edit = QLineEdit(entry.description)
        self._litematic_version_edit = QLineEdit(str(entry.litematic_version or "-"))
        self._minecraft_data_version_edit = QLineEdit(str(entry.minecraft_data_version or "-"))
        self._litematic_version_edit.setReadOnly(True)
        self._minecraft_data_version_edit.setReadOnly(True)
        self._save_hint = QLabel("保存前会检查原地址：原地址存在则覆盖原地址文件，不存在则覆盖投影库备份文件。")
        self._save_hint.setWordWrap(True)
        self._save_hint.setStyleSheet("color: palette(mid);")

        browse_button = QPushButton("浏览...")
        browse_button.clicked.connect(self._browse_original_path)
        path_row = QWidget()
        path_row_layout = QHBoxLayout(path_row)
        path_row_layout.setContentsMargins(0, 0, 0, 0)
        path_row_layout.addWidget(self._original_path_edit, 1)
        path_row_layout.addWidget(browse_button)

        form.addRow("显示名", self._display_name_edit)
        form.addRow("原地址", path_row)
        form.addRow("内部名称", self._internal_name_edit)
        form.addRow("作者", self._author_edit)
        form.addRow("描述", self._description_edit)
        form.addRow("投影文件版本", self._litematic_version_edit)
        form.addRow("Minecraft 数据版本", self._minecraft_data_version_edit)
        form.addRow("备注", self._note_edit)
        layout.addLayout(form)
        layout.addWidget(self._save_hint)

        buttons = QHBoxLayout()
        buttons.addStretch(1)
        cancel_button = QPushButton("取消")
        save_button = QPushButton("保存")
        cancel_button.clicked.connect(self.reject)
        save_button.clicked.connect(self.accept)
        buttons.addWidget(cancel_button)
        buttons.addWidget(save_button)
        layout.addLayout(buttons)

    @property
    def display_name(self) -> str:
        return self._display_name_edit.text().strip()

    @property
    def original_path(self) -> str:
        return self._original_path_edit.text().strip()

    @property
    def note(self) -> str:
        return self._note_edit.text().strip()

    @property
    def internal_name(self) -> str:
        return self._internal_name_edit.text().strip()

    @property
    def author(self) -> str:
        return self._author_edit.text().strip()

    @property
    def description(self) -> str:
        return self._description_edit.text().strip()

    def _browse_original_path(self) -> None:
        path, _ = QFileDialog.getOpenFileName(
            self,
            "选择原投影文件",
            self._original_path_edit.text().strip(),
            "Litematic Files (*.litematic);;All Files (*.*)",
        )
        if path:
            self._original_path_edit.setText(path)


class DraggablePathLabel(QLabel):
    def __init__(self, path: str, parent: QWidget | None = None) -> None:
        super().__init__(path or "未设置原地址", parent)
        self._path = path
        self._drag_start: QPoint | None = None
        self.setWordWrap(True)
        self.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        self.setToolTip(path or "未设置原地址")

    def mousePressEvent(self, event) -> None:  # type: ignore[override]
        if event.button() == Qt.MouseButton.LeftButton:
            self._drag_start = event.position().toPoint()
        super().mousePressEvent(event)

    def mouseMoveEvent(self, event) -> None:  # type: ignore[override]
        if not self._path or self._drag_start is None:
            super().mouseMoveEvent(event)
            return
        if (event.position().toPoint() - self._drag_start).manhattanLength() < 8:
            super().mouseMoveEvent(event)
            return
        mime = QMimeData()
        mime.setText(self._path)
        path = Path(self._path)
        if path.exists():
            mime.setUrls([QUrl.fromLocalFile(str(path))])
        drag = QDrag(self)
        drag.setMimeData(mime)
        drag.exec(Qt.DropAction.CopyAction)


class ProjectionListWidget(QListWidget):
    order_changed = Signal(list)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._smooth_scroll_target = 0
        self._scrollbar_dragging = False
        self._drag_sort_active = False
        self._scroll_animation = QPropertyAnimation(self.verticalScrollBar(), b"value", self)
        self._scroll_animation.setDuration(170)
        self._scroll_animation.setEasingCurve(QEasingCurve.Type.OutCubic)
        self.setDragDropMode(QAbstractItemView.DragDropMode.InternalMove)
        self.setDefaultDropAction(Qt.DropAction.MoveAction)
        self.setDragDropOverwriteMode(False)
        self.setDropIndicatorShown(True)
        self.setSelectionMode(QAbstractItemView.SelectionMode.SingleSelection)
        self.setVerticalScrollMode(QAbstractItemView.ScrollMode.ScrollPerPixel)
        self.setAutoScroll(True)
        self.setAutoScrollMargin(56)
        self.setSpacing(8)
        self.setFrameShape(QFrame.Shape.NoFrame)
        bar = self.verticalScrollBar()
        bar.setSingleStep(24)
        bar.sliderPressed.connect(self._on_scrollbar_pressed)
        bar.sliderReleased.connect(self._on_scrollbar_released)
        bar.actionTriggered.connect(self._on_scrollbar_action)

    def wheelEvent(self, event) -> None:  # type: ignore[override]
        if self._drag_sort_active:
            super().wheelEvent(event)
            return
        bar = self.verticalScrollBar()
        pixel_delta = event.pixelDelta().y()
        angle_delta = event.angleDelta().y()
        if pixel_delta == 0 and angle_delta == 0:
            super().wheelEvent(event)
            return
        delta = pixel_delta if pixel_delta else int(angle_delta / 120.0 * 92)
        current = bar.value()
        base = self._smooth_scroll_target if self._scroll_animation.state() == QPropertyAnimation.State.Running else current
        target = max(bar.minimum(), min(bar.maximum(), base - delta))
        self._animate_scroll_to(target)
        event.accept()

    def startDrag(self, supported_actions) -> None:  # type: ignore[override]
        self.stop_smooth_scroll()
        self._drag_sort_active = True
        try:
            super().startDrag(supported_actions)
        finally:
            self._drag_sort_active = False

    def dropEvent(self, event) -> None:  # type: ignore[override]
        self.stop_smooth_scroll()
        super().dropEvent(event)
        self.order_changed.emit(self.entry_order())

    def stop_smooth_scroll(self) -> None:
        self._scroll_animation.stop()
        self._smooth_scroll_target = self.verticalScrollBar().value()

    def _animate_scroll_to(self, target: int, *, duration: int = 170) -> None:
        bar = self.verticalScrollBar()
        target = max(bar.minimum(), min(bar.maximum(), int(target)))
        self._smooth_scroll_target = target
        start = bar.value()
        if start == target:
            self._scroll_animation.stop()
            return
        self._scroll_animation.stop()
        self._scroll_animation.setDuration(duration)
        self._scroll_animation.setStartValue(start)
        self._scroll_animation.setEndValue(target)
        self._scroll_animation.start()

    def _on_scrollbar_pressed(self) -> None:
        self._scrollbar_dragging = True
        self.stop_smooth_scroll()

    def _on_scrollbar_released(self) -> None:
        self._scrollbar_dragging = False
        self._smooth_scroll_target = self.verticalScrollBar().value()

    def _on_scrollbar_action(self, _action: int) -> None:
        if self._scrollbar_dragging or self._drag_sort_active:
            return
        bar = self.verticalScrollBar()
        start = bar.value()

        def smooth_action_result() -> None:
            if self._scrollbar_dragging or self._drag_sort_active:
                return
            target = bar.value()
            if target == start:
                return
            bar.setValue(start)
            self._animate_scroll_to(target, duration=150)

        QTimer.singleShot(0, smooth_action_result)

    def entry_order(self) -> list[str]:
        return [
            str(self.item(index).data(Qt.ItemDataRole.UserRole))
            for index in range(self.count())
            if self.item(index) is not None
        ]


class ProjectionPreviewWorker(QThread):
    finished_ok = Signal(str)
    failed = Signal(str)

    def __init__(self, store: ProjectionLibraryStore, entry: ProjectionLibraryEntry) -> None:
        super().__init__()
        self._store = store
        self._entry = entry

    def run(self) -> None:  # type: ignore[override]
        try:
            path = generate_projection_preview(self._store, ProjectionPreviewRequest(entry=self._entry))
        except Exception as exc:
            self.failed.emit(str(exc))
            return
        self.finished_ok.emit(str(path))


class ProjectionLibraryCard(QFrame):
    open_requested = Signal(str)
    save_as_requested = Signal(str)
    edit_requested = Signal(str)
    delete_requested = Signal(str)
    preview_requested = Signal(str)

    def __init__(
        self,
        entry: ProjectionLibraryEntry,
        validation: ProjectionValidation,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self._entry_id = entry.entry_id
        self.setObjectName("projectionLibraryCard")
        self.setFrameShape(QFrame.Shape.StyledPanel)

        root = QHBoxLayout(self)
        root.setContentsMargins(14, 14, 14, 14)
        root.setSpacing(14)

        text_col = QVBoxLayout()
        text_col.setSpacing(7)
        title_row = QHBoxLayout()
        title_label = QLabel(entry.display_name)
        title_font = title_label.font()
        title_font.setBold(True)
        title_font.setPointSize(max(12, title_font.pointSize() + 1))
        title_label.setFont(title_font)
        size_label = QLabel(_format_size(entry.file_size))
        size_label.setStyleSheet("color: palette(mid);")
        title_row.addWidget(title_label, 1)
        title_row.addWidget(size_label)
        text_col.addLayout(title_row)

        tags = entry.normalized_tags()
        tag_label = QLabel("标签：" + (" / ".join(tags) if tags else "未分类"))
        tag_label.setStyleSheet("color: palette(mid);")
        text_col.addWidget(tag_label)

        metadata_bits: list[str] = []
        if entry.internal_name:
            metadata_bits.append(f"内部名称：{entry.internal_name}")
        if entry.author:
            metadata_bits.append(f"作者：{entry.author}")
        if metadata_bits:
            metadata_label = QLabel("    ".join(metadata_bits))
            metadata_label.setStyleSheet("color: palette(mid);")
            metadata_label.setWordWrap(True)
            text_col.addWidget(metadata_label)

        original_name = entry.original_file_name or Path(entry.backup_file_path).name
        text_col.addWidget(QLabel(f"原文件：{original_name}"))
        path_label = DraggablePathLabel(entry.original_file_path or "未设置原地址")
        path_label.setStyleSheet("color: palette(mid);")
        text_col.addWidget(path_label)

        meta_label = QLabel(f"导入：{_format_time(entry.imported_at)}    最近使用：{_format_time(entry.last_used_at)}")
        meta_label.setStyleSheet("color: palette(mid);")
        meta_label.setWordWrap(True)
        text_col.addWidget(meta_label)

        if entry.note:
            note_label = QLabel(f"备注：{entry.note}")
            note_label.setWordWrap(True)
            note_label.setStyleSheet("color: palette(mid);")
            text_col.addWidget(note_label)

        if validation.has_issue:
            status_label = QLabel("；".join(validation.messages))
            status_label.setWordWrap(True)
            status_label.setStyleSheet("color: #c62828;")
            text_col.addWidget(status_label)

        buttons = QHBoxLayout()
        open_button = QPushButton("打开")
        save_as_button = QPushButton("另存为")
        edit_button = QPushButton("更改属性")
        delete_button = QPushButton("删除")
        open_button.clicked.connect(lambda: self.open_requested.emit(self._entry_id))
        save_as_button.clicked.connect(lambda: self.save_as_requested.emit(self._entry_id))
        edit_button.clicked.connect(lambda: self.edit_requested.emit(self._entry_id))
        delete_button.clicked.connect(lambda: self.delete_requested.emit(self._entry_id))
        buttons.addWidget(open_button)
        buttons.addWidget(save_as_button)
        buttons.addWidget(edit_button)
        buttons.addWidget(delete_button)
        buttons.addStretch(1)
        text_col.addLayout(buttons)
        root.addLayout(text_col, 1)

        preview_col = QVBoxLayout()
        preview_col.setSpacing(6)
        self._preview_label = QLabel()
        self._preview_label.setObjectName("projectionPreviewImage")
        self._preview_label.setFixedSize(180, 112)
        self._preview_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._preview_label.setStyleSheet("background: #444; color: #ddd; border: 1px solid #666;")
        preview = entry.preview_path
        if preview is not None and preview.is_file():
            pixmap = QPixmap(str(preview))
            if not pixmap.isNull():
                self._preview_label.setPixmap(
                    pixmap.scaled(
                        self._preview_label.size(),
                        Qt.AspectRatioMode.KeepAspectRatio,
                        Qt.TransformationMode.SmoothTransformation,
                    )
                )
        else:
            self._preview_label.setText("暂无预览图")
        preview_button = QPushButton("生成预览")
        preview_button.clicked.connect(lambda: self.preview_requested.emit(self._entry_id))
        preview_col.addWidget(self._preview_label)
        preview_col.addWidget(preview_button)
        preview_col.addStretch(1)
        root.addLayout(preview_col)


class LibraryPage(QWidget):
    open_projection = Signal(str)

    def __init__(
        self,
        projection_library: ProjectionLibraryStore,
        *,
        app_settings: AppSettings | None = None,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self._store = projection_library
        self._app_settings = app_settings or AppSettings()
        self._all_rows: list[tuple[ProjectionLibraryEntry, ProjectionValidation]] = []
        self._preview_worker: ProjectionPreviewWorker | None = None

        root = QVBoxLayout(self)
        root.setContentsMargins(12, 12, 12, 12)
        root.setSpacing(10)

        actions = QHBoxLayout()
        title = QLabel("投影库")
        title_font = title.font()
        title_font.setBold(True)
        title_font.setPointSize(max(14, title_font.pointSize() + 2))
        title.setFont(title_font)
        self._summary_label = QLabel("")
        self._summary_label.setStyleSheet("color: palette(mid);")
        self._limit_combo = QComboBox()
        for value in VALID_PROJECTION_LIBRARY_LIMITS:
            self._limit_combo.addItem(str(value), value)
        self._refresh_button = QPushButton("刷新校验")
        self._refresh_button.clicked.connect(self.refresh_library)
        self._limit_combo.currentIndexChanged.connect(self._on_limit_changed)
        actions.addWidget(title)
        actions.addWidget(self._summary_label)
        actions.addStretch(1)
        actions.addWidget(QLabel("最近保留"))
        actions.addWidget(self._limit_combo)
        actions.addWidget(self._refresh_button)
        root.addLayout(actions)

        filter_row = QHBoxLayout()
        self._search_edit = QLineEdit()
        self._search_edit.setPlaceholderText("搜索投影名、原文件名或原地址")
        self._tag_combo = QComboBox()
        self._tag_combo.addItem("全部标签", "")
        self._sort_combo = QComboBox()
        self._sort_combo.addItem("手动排序", "manual")
        self._sort_combo.addItem("最近导入/使用", "recent")
        self._sort_combo.addItem("名称", "name")
        self._apply_sort_button = QPushButton("应用排序")
        self._search_edit.textChanged.connect(self._render_filtered_entries)
        self._tag_combo.currentIndexChanged.connect(self._render_filtered_entries)
        self._apply_sort_button.clicked.connect(self._apply_sort_mode)
        filter_row.addWidget(self._search_edit, 2)
        filter_row.addWidget(self._tag_combo)
        filter_row.addWidget(self._sort_combo)
        filter_row.addWidget(self._apply_sort_button)
        root.addLayout(filter_row)

        self._empty_hint = QLabel("投影库还没有内容。请先通过默认导入打开一个 .litematic 文件。")
        self._empty_hint.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._empty_hint.setWordWrap(True)
        self._empty_hint.setStyleSheet("color: palette(mid);")

        self._list = ProjectionListWidget(self)
        self._list.order_changed.connect(self._persist_manual_order)

        root.addWidget(self._empty_hint)
        root.addWidget(self._list, 1)

        self._apply_limit_selection()
        self.refresh_library()

    def apply_app_settings(self, settings: AppSettings) -> None:
        self._app_settings = settings
        self._apply_limit_selection()

    def showEvent(self, event) -> None:  # type: ignore[override]
        super().showEvent(event)
        self.refresh_library()

    def refresh_library(self) -> None:
        try:
            self._all_rows = self._store.list_entries_with_validation()
        except ProjectionLibraryError as exc:
            QMessageBox.critical(self, "投影库", str(exc))
            return
        self._refresh_tag_filter()
        self._render_filtered_entries()

    def _refresh_tag_filter(self) -> None:
        current = str(self._tag_combo.currentData() or "")
        tags = sorted({tag for entry, _ in self._all_rows for tag in entry.normalized_tags()})
        self._tag_combo.blockSignals(True)
        self._tag_combo.clear()
        self._tag_combo.addItem("全部标签", "")
        for tag in tags:
            self._tag_combo.addItem(tag, tag)
        index = self._tag_combo.findData(current)
        self._tag_combo.setCurrentIndex(index if index >= 0 else 0)
        self._tag_combo.blockSignals(False)

    def _render_filtered_entries(self) -> None:
        self._list.stop_smooth_scroll()
        self._list.clear()
        query = self._search_edit.text().strip().lower()
        tag_filter = str(self._tag_combo.currentData() or "")
        rows: list[tuple[ProjectionLibraryEntry, ProjectionValidation]] = []
        for entry, validation in self._all_rows:
            haystack = " ".join(
                [entry.display_name, entry.original_file_name, entry.original_file_path, " ".join(entry.normalized_tags())]
            ).lower()
            if query and query not in haystack:
                continue
            if tag_filter and tag_filter not in entry.normalized_tags():
                continue
            rows.append((entry, validation))

        self._empty_hint.setVisible(not rows)
        issue_count = sum(1 for _, validation in self._all_rows if validation.has_issue)
        self._summary_label.setText(f"{len(self._all_rows)} 条记录，{issue_count} 条需关注")

        for entry, validation in rows:
            item = QListWidgetItem()
            item.setData(Qt.ItemDataRole.UserRole, entry.entry_id)
            card = ProjectionLibraryCard(entry, validation, self._list)
            card.open_requested.connect(self._open_entry)
            card.save_as_requested.connect(self._save_as_entry)
            card.edit_requested.connect(self._edit_entry)
            card.delete_requested.connect(self._delete_entry)
            card.preview_requested.connect(self._generate_preview)
            item.setSizeHint(card.sizeHint())
            self._list.addItem(item)
            self._list.setItemWidget(item, card)

    def _apply_limit_selection(self) -> None:
        target = getattr(self._app_settings, "projection_library_limit", DEFAULT_PROJECTION_LIBRARY_LIMIT)
        index = self._limit_combo.findData(target)
        if index < 0:
            index = self._limit_combo.findData(DEFAULT_PROJECTION_LIBRARY_LIMIT)
        self._limit_combo.blockSignals(True)
        self._limit_combo.setCurrentIndex(max(0, index))
        self._limit_combo.blockSignals(False)

    def _on_limit_changed(self) -> None:
        value = int(self._limit_combo.currentData() or DEFAULT_PROJECTION_LIBRARY_LIMIT)
        self._app_settings.projection_library_limit = value
        save_settings(self._app_settings)
        try:
            self._store.enforce_limit(value)
        except ProjectionLibraryError as exc:
            QMessageBox.critical(self, "投影库", str(exc))
            return
        self.refresh_library()

    def _apply_sort_mode(self) -> None:
        mode = str(self._sort_combo.currentData() or "manual")
        try:
            self._store.sort_entries(mode)
        except ProjectionLibraryError as exc:
            QMessageBox.critical(self, "投影库", str(exc))
            return
        order = {entry_id: index for index, entry_id in enumerate(ordered)}
        self._all_rows.sort(key=lambda row: (order.get(row[0].entry_id, len(order)), row[0].display_name.lower()))

    def _persist_manual_order(self, ordered: list[str]) -> None:
        if str(self._sort_combo.currentData() or "manual") != "manual":
            return
        try:
            self._store.reorder_entries(ordered)
        except ProjectionLibraryError as exc:
            QMessageBox.critical(self, "投影库", str(exc))
            return
        self.refresh_library()

    def _open_entry(self, entry_id: str) -> None:
        try:
            backup_path = self._store.open_entry_backup(entry_id)
        except ProjectionLibraryError as exc:
            QMessageBox.critical(self, "投影库", str(exc))
            return
        self.open_projection.emit(str(backup_path))
        self.refresh_library()

    def _save_as_entry(self, entry_id: str) -> None:
        try:
            entry = self._store.get_entry(entry_id)
        except ProjectionLibraryError as exc:
            QMessageBox.critical(self, "投影库", str(exc))
            return
        target, _ = QFileDialog.getSaveFileName(
            self,
            "投影另存为",
            str(Path(entry.display_name or entry.original_file_name).with_suffix(".litematic")),
            "Litematic Files (*.litematic);;All Files (*.*)",
        )
        if not target:
            return
        try:
            self._store.save_backup_as(entry_id, target)
        except ProjectionLibraryError as exc:
            QMessageBox.critical(self, "投影库", str(exc))
            return
        QMessageBox.information(self, "投影库", "已另存为到目标路径。")

    def _delete_entry(self, entry_id: str) -> None:
        answer = QMessageBox.question(
            self,
            "删除投影",
            "会同时删除投影库记录和备份文件，是否继续？",
        )
        if answer != QMessageBox.StandardButton.Yes:
            return
        try:
            self._store.remove_entry(entry_id)
        except ProjectionLibraryError as exc:
            QMessageBox.critical(self, "投影库", str(exc))
            return
        self.refresh_library()

    def _edit_entry(self, entry_id: str) -> None:
        try:
            entry = self._store.get_entry(entry_id)
        except ProjectionLibraryError as exc:
            QMessageBox.critical(self, "投影库", str(exc))
            return
        dialog = ProjectionEditDialog(entry, self)
        if dialog.exec() != QDialog.DialogCode.Accepted:
            return
        try:
            self._store.update_entry(
                entry_id,
                display_name=dialog.display_name,
                original_file_path=dialog.original_path,
                note=dialog.note,
                internal_name=dialog.internal_name,
                author=dialog.author,
                description=dialog.description,
            )
        except ProjectionLibraryError as exc:
            QMessageBox.critical(self, "投影库", str(exc))
            return
        self.refresh_library()

    def _generate_preview(self, entry_id: str) -> None:
        if self._preview_worker is not None and self._preview_worker.isRunning():
            QMessageBox.information(self, "生成预览图", "已有预览图生成任务正在运行。")
            return
        try:
            entry = self._store.get_entry(entry_id)
        except (ProjectionLibraryError, ProjectionPreviewError) as exc:
            QMessageBox.warning(self, "生成预览图", str(exc))
            return
        worker = ProjectionPreviewWorker(self._store, entry)
        self._preview_worker = worker
        worker.finished_ok.connect(lambda _path: self._on_preview_generated(worker))
        worker.failed.connect(lambda message: self._on_preview_failed(worker, message))
        worker.finished.connect(worker.deleteLater)
        worker.start()

    def _on_preview_generated(self, worker: ProjectionPreviewWorker) -> None:
        if self._preview_worker is worker:
            self._preview_worker = None
        self.refresh_library()

    def _on_preview_failed(self, worker: ProjectionPreviewWorker, message: str) -> None:
        if self._preview_worker is worker:
            self._preview_worker = None
        QMessageBox.warning(self, "生成预览图", message)
        self.refresh_library()
