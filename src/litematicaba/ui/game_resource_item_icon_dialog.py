"""物品图标资源管理弹窗（结构对齐方块图标管理）。"""

from __future__ import annotations

from PySide6.QtCore import QThread, Qt, Signal
from PySide6.QtWidgets import (
    QApplication,
    QComboBox,
    QDialog,
    QFormLayout,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QMessageBox,
    QPushButton,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
    QWidget,
)

from litematicaba.core.game_resource_item_icon import (
    ITEM_SOURCE_BUILTIN,
    ITEM_SOURCE_VAULT,
    InstalledItemIcon,
    clear_active_layering_item_icon,
    delete_installed_item_icon,
    download_and_process_vault_item_icons,
    ensure_initial_item_seeded,
    load_installed_item_icons,
    register_vault_item_icon_slot,
    set_active_layering_item_icon,
)
from litematicaba.ui.layering_item_icon_prewarmer import restart_layering_item_icon_prewarm
from litematicaba.ui.theme import current_theme_id
from litematicaba.ui.widgets.mcmeta_standard_table import (
    McmetaStandardTableCellHost,
    McmetaStandardTableRowHoverController,
    apply_item_icon_resource_table_chrome,
    apply_mcmeta_standard_table_row_heights,
    attach_mcmeta_row_hover_to_button,
    clear_mcmeta_table_current_cell,
)
from litematicaba.ui.widgets.progress_dialog import GenericProgressDialog

_COL_VER = 0
_COL_SRC = 1
_COL_OP = 2


class _VaultDownloadWorker(QThread):
    progress = Signal(int, int, str)
    finished_ok = Signal(object)
    finished_err = Signal(str)

    def __init__(self) -> None:
        super().__init__()
        self._is_canceled = False

    def cancel(self) -> None:
        self._is_canceled = True

    def _progress_callback(self, current: int, total: int, status: str) -> bool:
        self.progress.emit(current, total, status)
        return not self._is_canceled

    def run(self) -> None:  # type: ignore[override]
        try:
            download_and_process_vault_item_icons(self._progress_callback)
            if self._is_canceled:
                return
            self.finished_ok.emit(register_vault_item_icon_slot())
        except Exception as exc:
            self.finished_err.emit(str(exc))


class GameResourceItemIconDialog(QDialog):
    """来源、Vault 下载与已装载列表（物品仅 2D）。"""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName("GameResourceItemIconDialog")
        self.setWindowTitle("物品图标资源管理")
        self.resize(900, 560)
        ensure_initial_item_seeded()

        self._download_worker: _VaultDownloadWorker | None = None
        self._installed: list[InstalledItemIcon] = load_installed_item_icons()
        self._hover_ctrl: McmetaStandardTableRowHoverController | None = None

        root = QVBoxLayout(self)

        form = QFormLayout()
        self._source = QComboBox()
        self._source.addItem("内建", ITEM_SOURCE_BUILTIN)
        self._source.addItem("vault", ITEM_SOURCE_VAULT)
        form.addRow("来源", self._source)
        root.addLayout(form)

        row2 = QHBoxLayout()
        row2.addWidget(QLabel("子选项1"))
        self._sub1 = QComboBox()
        self._sub1.setMinimumWidth(160)
        row2.addWidget(self._sub1, 1)
        row2.addWidget(QLabel("子选项2"))
        self._sub2 = QComboBox()
        self._sub2.setMinimumWidth(160)
        row2.addWidget(self._sub2, 1)
        self._btn_download = QPushButton("下载")
        row2.addWidget(self._btn_download)
        root.addLayout(row2)

        self._table = QTableWidget(0, 3)
        self._table.setHorizontalHeaderLabels(["版本（目前均为未知）", "来源", ""])
        hh = self._table.horizontalHeader()
        hh.setSectionsMovable(False)
        hh.setSectionResizeMode(_COL_VER, QHeaderView.ResizeMode.Fixed)
        hh.setSectionResizeMode(_COL_SRC, QHeaderView.ResizeMode.Fixed)
        hh.setSectionResizeMode(_COL_OP, QHeaderView.ResizeMode.Stretch)
        self._table.setColumnWidth(_COL_VER, 180)
        self._table.setColumnWidth(_COL_SRC, 220)

        app = QApplication.instance()
        tid = current_theme_id(app if isinstance(app, QApplication) else None)
        self._hover_ctrl = apply_item_icon_resource_table_chrome(self._table, tid)
        self._table.setObjectName("OptionsItemIconResourceTable")

        root.addWidget(QLabel("已装载的资源"))
        root.addWidget(self._table, 1)

        self._status = QLabel("")
        self._status.setStyleSheet("color: palette(mid);")
        root.addWidget(self._status)

        close_row = QHBoxLayout()
        close_row.addStretch()
        btn_close = QPushButton("关闭")
        btn_close.clicked.connect(self.accept)
        close_row.addWidget(btn_close)
        root.addLayout(close_row)

        self._source.currentIndexChanged.connect(self._on_source_changed)
        self._btn_download.clicked.connect(self._on_download_clicked)

        self._sub1.setEnabled(False)
        self._sub2.setEnabled(False)

        self._refresh_table()
        self._on_source_changed()

    def _set_busy(self, busy: bool, text: str = "") -> None:
        self._source.setEnabled(not busy)
        vault = self._source.currentData() == ITEM_SOURCE_VAULT
        self._btn_download.setEnabled((not busy) and vault)
        self._status.setText(text)

    def _refresh_table(self) -> None:
        assert self._hover_ctrl is not None
        self._table._mcmeta_hover_row = None  # type: ignore[attr-defined]
        has_layer = any(i.active_layering for i in self._installed)
        rows = [
            InstalledItemIcon(
                id=ITEM_SOURCE_BUILTIN,
                version_label="未知",
                source_label="内建",
                source_key=ITEM_SOURCE_BUILTIN,
                file_relpath="",
                active_layering=not has_layer,
                installed_at="",
            )
        ] + list(self._installed)
        self._table.clearContents()
        self._table.setRowCount(0)
        self._table.setRowCount(len(rows))
        for r, item in enumerate(rows):
            for c in range(self._table.columnCount()):
                self._table.removeCellWidget(r, c)
            v_it = QTableWidgetItem(item.version_label)
            v_it.setFlags(Qt.ItemFlag.ItemIsSelectable | Qt.ItemFlag.ItemIsEnabled)
            self._table.setItem(r, _COL_VER, v_it)
            s_it = QTableWidgetItem(item.source_label)
            s_it.setFlags(Qt.ItemFlag.ItemIsSelectable | Qt.ItemFlag.ItemIsEnabled)
            self._table.setItem(r, _COL_SRC, s_it)

            op_host = McmetaStandardTableCellHost(self._table, r, self._hover_ctrl)
            op_row = QHBoxLayout(op_host)
            op_row.setContentsMargins(0, 0, 0, 0)
            op_row.setSpacing(4)

            btn_apply = QPushButton("应用")
            btn_apply.setCheckable(True)
            btn_apply.setChecked(bool(item.active_layering))
            btn_apply.clicked.connect(lambda _c=False, item_id=item.id: self._on_apply(item_id))
            attach_mcmeta_row_hover_to_button(btn_apply, self._hover_ctrl, r)
            op_row.addWidget(btn_apply)

            btn_del = QPushButton("删除")
            if item.id == ITEM_SOURCE_BUILTIN:
                btn_del.setEnabled(False)
            btn_del.clicked.connect(lambda _c=False, item_id=item.id: self._on_delete(item_id))
            attach_mcmeta_row_hover_to_button(btn_del, self._hover_ctrl, r)
            op_row.addWidget(btn_del)
            op_row.addStretch()
            self._table.setCellWidget(r, _COL_OP, op_host)

        app = QApplication.instance()
        tid = current_theme_id(app if isinstance(app, QApplication) else None)
        apply_mcmeta_standard_table_row_heights(self._table, tid)
        self._hover_ctrl.set_hover_row(None)
        clear_mcmeta_table_current_cell(self._table)

    def _on_source_changed(self) -> None:
        src = self._source.currentData()
        if src == ITEM_SOURCE_BUILTIN:
            self._set_busy(False, "当前来源：内建（2D，item/initial）")
        else:
            self._set_busy(False, "当前来源：vault（2D，item/vault；下载将登记至 item/installed.json）")

    def _on_download_clicked(self) -> None:
        if self._source.currentData() != ITEM_SOURCE_VAULT:
            return

        self._progress_dlg = GenericProgressDialog("下载物品图标", self)
        self._download_worker = _VaultDownloadWorker()

        self._download_worker.progress.connect(self._progress_dlg.set_progress)
        self._download_worker.progress.connect(
            lambda _c, _t, status: self._progress_dlg.set_status(status)
        )
        self._download_worker.finished_ok.connect(self._on_vault_ok)
        self._download_worker.finished_err.connect(self._on_vault_err)
        self._progress_dlg.canceled.connect(self._download_worker.cancel)

        self._download_worker.start()
        self._progress_dlg.exec()

    def _on_vault_ok(self, item: object) -> None:
        if hasattr(self, "_progress_dlg"):
            self._progress_dlg.accept()
        self._download_worker = None
        if not isinstance(item, InstalledItemIcon):
            self._set_busy(False, "登记失败")
            return
        self._installed = load_installed_item_icons()
        self._refresh_table()
        self._set_busy(False, "已写入 item/installed.json，Vault 图标索引下载完成。")
        restart_layering_item_icon_prewarm()

    def _on_vault_err(self, err: str) -> None:
        if hasattr(self, "_progress_dlg"):
            self._progress_dlg.reject()
        self._download_worker = None
        self._set_busy(False, "登记失败")
        QMessageBox.warning(self, "物品图标", f"Vault 下载或登记失败：\n{err}")

    def _on_apply(self, item_id: str) -> None:
        if item_id == ITEM_SOURCE_BUILTIN:
            self._installed = clear_active_layering_item_icon()
            self._refresh_table()
            self._set_busy(False, "已应用：内建 2D")
            restart_layering_item_icon_prewarm()
            return
        self._installed = set_active_layering_item_icon(item_id)
        self._refresh_table()
        self._set_busy(False, "已应用")
        restart_layering_item_icon_prewarm()

    def _on_delete(self, item_id: str) -> None:
        target = None
        for item in self._installed:
            if item.id == item_id:
                target = item
                break
        if target is None:
            return
        r = QMessageBox.question(
            self,
            "删除资源",
            f"确定删除该资源？\n\n来源：{target.source_label}",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
            QMessageBox.StandardButton.No,
        )
        if r != QMessageBox.StandardButton.Yes:
            return
        self._installed = delete_installed_item_icon(item_id)
        self._refresh_table()
        self._set_busy(False, "已删除资源")
        restart_layering_item_icon_prewarm()

    def closeEvent(self, event) -> None:  # type: ignore[no-untyped-def]
        if self._hover_ctrl is not None:
            self._hover_ctrl.remove_from_viewport()
        super().closeEvent(event)