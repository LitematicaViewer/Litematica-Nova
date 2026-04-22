"""方块图标资源管理弹窗（结构对齐游戏资源语言管理）。"""

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

from litematicaba.core.game_resource_block_icon import (
    BLOCK_SOURCE_BUILTIN,
    BLOCK_SOURCE_VAULT,
    BLOCK_TYPE_2D,
    BLOCK_TYPE_ICON,
    InstalledBlockVisual,
    clear_active_layering,
    clear_active_material_list,
    delete_installed_block_visual,
    ensure_initial_block_2d_seeded,
    load_installed_block_visuals,
    register_vault_block_icon_slot,
    set_active_layering,
    set_active_material_list,
)
from litematicaba.ui.material_list_icon_prewarmer import restart_material_list_icon_prewarm
from litematicaba.ui.theme import current_theme_id
from litematicaba.ui.widgets.mcmeta_standard_table import (
    McmetaStandardTableCellHost,
    McmetaStandardTableRowHoverController,
    apply_block_icon_resource_table_chrome,
    apply_mcmeta_standard_table_row_heights,
    attach_mcmeta_row_hover_to_button,
    clear_mcmeta_table_current_cell,
)

_COL_VER = 0
_COL_KIND = 1
_COL_SRC = 2
_COL_OP = 3


class _VaultRegisterWorker(QThread):
    finished_ok = Signal(object)
    finished_err = Signal(str)

    def run(self) -> None:  # type: ignore[override]
        try:
            self.finished_ok.emit(register_vault_block_icon_slot())
        except Exception as exc:
            self.finished_err.emit(str(exc))


class GameResourceBlockIconDialog(QDialog):
    """来源、子选项占位、Vault 登记与已装载列表。"""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName("GameResourceBlockIconDialog")
        self.setWindowTitle("方块图标资源管理")
        self.resize(960, 560)
        ensure_initial_block_2d_seeded()

        self._vault_worker: _VaultRegisterWorker | None = None
        self._installed: list[InstalledBlockVisual] = load_installed_block_visuals()
        self._hover_ctrl: McmetaStandardTableRowHoverController | None = None

        root = QVBoxLayout(self)

        form = QFormLayout()
        self._source = QComboBox()
        self._source.addItem("内建", BLOCK_SOURCE_BUILTIN)
        self._source.addItem("vault", BLOCK_SOURCE_VAULT)
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

        self._table = QTableWidget(0, 4)
        self._table.setHorizontalHeaderLabels(["版本（目前均为未知）", "类型", "来源", ""])
        hh = self._table.horizontalHeader()
        hh.setSectionsMovable(False)
        hh.setSectionResizeMode(_COL_VER, QHeaderView.ResizeMode.Fixed)
        hh.setSectionResizeMode(_COL_KIND, QHeaderView.ResizeMode.Fixed)
        hh.setSectionResizeMode(_COL_SRC, QHeaderView.ResizeMode.Fixed)
        hh.setSectionResizeMode(_COL_OP, QHeaderView.ResizeMode.Stretch)
        self._table.setColumnWidth(_COL_VER, 180)
        self._table.setColumnWidth(_COL_KIND, 72)
        self._table.setColumnWidth(_COL_SRC, 220)

        app = QApplication.instance()
        tid = current_theme_id(app if isinstance(app, QApplication) else None)
        self._hover_ctrl = apply_block_icon_resource_table_chrome(self._table, tid)
        self._table.setObjectName("OptionsBlockIconResourceTable")

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
        vault = self._source.currentData() == BLOCK_SOURCE_VAULT
        self._btn_download.setEnabled((not busy) and vault)
        self._status.setText(text)

    def _refresh_table(self) -> None:
        assert self._hover_ctrl is not None
        self._table._mcmeta_hover_row = None  # type: ignore[attr-defined]
        has_ml = any(i.active_material_list for i in self._installed)
        has_layer = any(i.active_layering for i in self._installed)
        rows = [
            InstalledBlockVisual(
                id=BLOCK_SOURCE_BUILTIN,
                version_label="未知",
                kind=BLOCK_TYPE_2D,
                source_label="内建",
                source_key=BLOCK_SOURCE_BUILTIN,
                file_relpath="",
                active_material_list=not has_ml,
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
            k_it = QTableWidgetItem(item.kind)
            k_it.setFlags(Qt.ItemFlag.ItemIsSelectable | Qt.ItemFlag.ItemIsEnabled)
            self._table.setItem(r, _COL_KIND, k_it)
            s_it = QTableWidgetItem(item.source_label)
            s_it.setFlags(Qt.ItemFlag.ItemIsSelectable | Qt.ItemFlag.ItemIsEnabled)
            self._table.setItem(r, _COL_SRC, s_it)

            op_host = McmetaStandardTableCellHost(self._table, r, self._hover_ctrl)
            op_row = QHBoxLayout(op_host)
            op_row.setContentsMargins(0, 0, 0, 0)
            op_row.setSpacing(4)

            btn_ml = QPushButton("应用到材料列表")
            btn_ml.setCheckable(True)
            if item.id == BLOCK_SOURCE_BUILTIN:
                btn_ml.setChecked(not has_ml)
            else:
                btn_ml.setChecked(bool(item.active_material_list))
            btn_ml.clicked.connect(
                lambda _c=False, item_id=item.id: self._on_apply_material_list(item_id)
            )
            attach_mcmeta_row_hover_to_button(btn_ml, self._hover_ctrl, r)
            op_row.addWidget(btn_ml)

            btn_layer = QPushButton("应用到分层")
            btn_layer.setCheckable(True)
            can_layer = item.kind == BLOCK_TYPE_2D
            btn_layer.setEnabled(can_layer)
            if not can_layer:
                btn_layer.setChecked(False)
                btn_layer.setToolTip("Icon 类型不能应用到分层")
            elif item.id == BLOCK_SOURCE_BUILTIN:
                btn_layer.setChecked(not has_layer)
            else:
                btn_layer.setChecked(bool(item.active_layering))
            btn_layer.clicked.connect(
                lambda _c=False, item_id=item.id: self._on_apply_layering(item_id)
            )
            attach_mcmeta_row_hover_to_button(btn_layer, self._hover_ctrl, r)
            op_row.addWidget(btn_layer)

            btn_del = QPushButton("删除")
            if item.id == BLOCK_SOURCE_BUILTIN:
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
        if src == BLOCK_SOURCE_BUILTIN:
            self._set_busy(False, "当前来源：内建（2D，block_2d/initial）")
        else:
            self._set_busy(
                False,
                "当前来源：vault（Icon，block_icon/vault；下载将登记至 block_icon/installed.json）",
            )

    def _on_download_clicked(self) -> None:
        if self._source.currentData() != BLOCK_SOURCE_VAULT:
            return
        self._set_busy(True, "正在准备 Vault 目录…")
        self._vault_worker = _VaultRegisterWorker()
        self._vault_worker.finished_ok.connect(self._on_vault_ok)
        self._vault_worker.finished_err.connect(self._on_vault_err)
        self._vault_worker.start()

    def _on_vault_ok(self, item: object) -> None:
        self._vault_worker = None
        if not isinstance(item, InstalledBlockVisual):
            self._set_busy(False, "登记失败")
            return
        self._installed = load_installed_block_visuals()
        self._refresh_table()
        self._set_busy(
            False,
            "已写入 block_icon/installed.json。站点为动态网页，批量方块图标自动拉取将在后续版本接入。",
        )
        restart_material_list_icon_prewarm()

    def _on_vault_err(self, err: str) -> None:
        self._vault_worker = None
        self._set_busy(False, "登记失败")
        QMessageBox.warning(self, "方块图标", f"Vault 登记失败：\n{err}")

    def _on_apply_material_list(self, item_id: str) -> None:
        if item_id == BLOCK_SOURCE_BUILTIN:
            self._installed = clear_active_material_list()
            self._refresh_table()
            self._set_busy(False, "已应用到材料列表：内建 2D")
            restart_material_list_icon_prewarm()
            return
        self._installed = set_active_material_list(item_id)
        self._refresh_table()
        self._set_busy(False, "已应用到材料列表")
        restart_material_list_icon_prewarm()

    def _on_apply_layering(self, item_id: str) -> None:
        if item_id == BLOCK_SOURCE_BUILTIN:
            self._installed = clear_active_layering()
            self._refresh_table()
            self._set_busy(False, "已应用到分层：内建 2D")
            return
        self._installed = set_active_layering(item_id)
        self._refresh_table()
        self._set_busy(False, "已应用到分层")

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
            f"确定删除该资源？\n\n类型：{target.kind}\n来源：{target.source_label}",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
            QMessageBox.StandardButton.No,
        )
        if r != QMessageBox.StandardButton.Yes:
            return
        self._installed = delete_installed_block_visual(item_id)
        self._refresh_table()
        self._set_busy(False, "已删除资源")

    def closeEvent(self, event) -> None:  # type: ignore[no-untyped-def]
        if self._hover_ctrl is not None:
            self._hover_ctrl.remove_from_viewport()
        super().closeEvent(event)
