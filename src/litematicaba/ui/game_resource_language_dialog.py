"""游戏资源语言管理弹窗。"""

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

from litematicaba.core.game_resource_language import (
    LANG_SOURCE_BUILTIN,
    LANG_SOURCE_GITHUB_INVENTIVETALENT,
    InstalledLanguage,
    delete_installed_language,
    download_github_language,
    ensure_initial_language_seeded,
    fetch_github_branches,
    fetch_github_languages,
    load_installed_languages,
    save_installed_languages,
    set_active_language,
)
from litematicaba.ui.theme import current_theme_id
from litematicaba.ui.widgets.mcmeta_standard_table import (
    McmetaStandardTableCellHost,
    McmetaStandardTableRowHoverController,
    apply_game_resource_language_table_chrome,
    apply_mcmeta_standard_table_row_heights,
    attach_mcmeta_row_hover_to_button,
    clear_mcmeta_table_current_cell,
)

_COL_LANG = 0
_COL_BRANCH = 1
_COL_SOURCE = 2
_COL_OP = 3


class _GithubBranchesWorker(QThread):
    finished_ok = Signal(list)
    finished_err = Signal(str)

    def run(self) -> None:  # type: ignore[override]
        try:
            self.finished_ok.emit(fetch_github_branches())
        except Exception as exc:
            self.finished_err.emit(str(exc))


class _GithubLanguagesWorker(QThread):
    finished_ok = Signal(list)
    finished_err = Signal(str)

    def __init__(self, branch: str) -> None:
        super().__init__()
        self._branch = branch

    def run(self) -> None:  # type: ignore[override]
        try:
            self.finished_ok.emit(fetch_github_languages(self._branch))
        except Exception as exc:
            self.finished_err.emit(str(exc))


class _DownloadLanguageWorker(QThread):
    finished_ok = Signal(object)
    finished_err = Signal(str)

    def __init__(self, branch: str, language: str) -> None:
        super().__init__()
        self._branch = branch
        self._language = language

    def run(self) -> None:  # type: ignore[override]
        try:
            self.finished_ok.emit(download_github_language(branch=self._branch, language=self._language))
        except Exception as exc:
            self.finished_err.emit(str(exc))


class GameResourceLanguageDialog(QDialog):
    """来源切换、远程拉取与已装载管理。"""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName("GameResourceLanguageDialog")
        self.setWindowTitle("游戏资源语言管理")
        self.resize(900, 560)
        ensure_initial_language_seeded()

        self._branch_worker: _GithubBranchesWorker | None = None
        self._lang_worker: _GithubLanguagesWorker | None = None
        self._download_worker: _DownloadLanguageWorker | None = None
        self._installed: list[InstalledLanguage] = load_installed_languages()
        self._hover_ctrl: McmetaStandardTableRowHoverController | None = None

        root = QVBoxLayout(self)

        form = QFormLayout()
        self._source = QComboBox()
        self._source.addItem("（内建）", LANG_SOURCE_BUILTIN)
        self._source.addItem("github/InventivetalentDev", LANG_SOURCE_GITHUB_INVENTIVETALENT)
        form.addRow("语言来源", self._source)
        root.addLayout(form)

        row2 = QHBoxLayout()
        row2.addWidget(QLabel("版本分支"))
        self._branch = QComboBox()
        self._branch.setMinimumWidth(180)
        row2.addWidget(self._branch, 1)
        row2.addWidget(QLabel("语言"))
        self._lang = QComboBox()
        self._lang.setMinimumWidth(160)
        row2.addWidget(self._lang, 1)
        self._btn_download = QPushButton("下载")
        row2.addWidget(self._btn_download)
        root.addLayout(row2)

        self._table = QTableWidget(0, 4)
        self._table.setHorizontalHeaderLabels(["语言", "分支版本", "来源", ""])
        hh = self._table.horizontalHeader()
        hh.setSectionsMovable(False)
        hh.setSectionResizeMode(_COL_LANG, QHeaderView.ResizeMode.Fixed)
        hh.setSectionResizeMode(_COL_BRANCH, QHeaderView.ResizeMode.Fixed)
        hh.setSectionResizeMode(_COL_SOURCE, QHeaderView.ResizeMode.Fixed)
        hh.setSectionResizeMode(_COL_OP, QHeaderView.ResizeMode.Stretch)
        self._table.setColumnWidth(_COL_LANG, 140)
        self._table.setColumnWidth(_COL_BRANCH, 140)
        self._table.setColumnWidth(_COL_SOURCE, 220)

        app = QApplication.instance()
        tid = current_theme_id(app if isinstance(app, QApplication) else None)
        self._hover_ctrl = apply_game_resource_language_table_chrome(self._table, tid)
        self._table.setObjectName("OptionsLanguageFileTable")

        root.addWidget(QLabel("已装载的语言"))
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
        self._branch.currentIndexChanged.connect(self._on_branch_changed)
        self._btn_download.clicked.connect(self._on_download_clicked)

        self._refresh_table()
        self._on_source_changed()

    def _set_busy(self, busy: bool, text: str = "") -> None:
        self._source.setEnabled(not busy)
        self._branch.setEnabled(not busy and self._source.currentData() == LANG_SOURCE_GITHUB_INVENTIVETALENT)
        self._lang.setEnabled(not busy and self._source.currentData() == LANG_SOURCE_GITHUB_INVENTIVETALENT)
        self._btn_download.setEnabled(
            (not busy)
            and self._source.currentData() == LANG_SOURCE_GITHUB_INVENTIVETALENT
            and self._branch.count() > 0
            and self._lang.count() > 0
        )
        self._status.setText(text)

    def _refresh_table(self) -> None:
        assert self._hover_ctrl is not None
        self._table._mcmeta_hover_row = None  # type: ignore[attr-defined]
        has_active_installed = any(i.active for i in self._installed)
        rows = [
            InstalledLanguage(
                id=LANG_SOURCE_BUILTIN,
                language="zh_cn（内建）",
                branch="-",
                source="（内建）",
                file_relpath="",
                active=not has_active_installed,
                installed_at="",
            )
        ] + list(self._installed)
        self._table.clearContents()
        self._table.setRowCount(0)
        self._table.setRowCount(len(rows))
        for r, item in enumerate(rows):
            for c in range(self._table.columnCount()):
                self._table.removeCellWidget(r, c)
            lang_it = QTableWidgetItem(item.language)
            lang_it.setFlags(Qt.ItemFlag.ItemIsSelectable | Qt.ItemFlag.ItemIsEnabled)
            self._table.setItem(r, _COL_LANG, lang_it)
            br_it = QTableWidgetItem(item.branch)
            br_it.setFlags(Qt.ItemFlag.ItemIsSelectable | Qt.ItemFlag.ItemIsEnabled)
            self._table.setItem(r, _COL_BRANCH, br_it)
            src_it = QTableWidgetItem(item.source)
            src_it.setFlags(Qt.ItemFlag.ItemIsSelectable | Qt.ItemFlag.ItemIsEnabled)
            self._table.setItem(r, _COL_SOURCE, src_it)

            op_host = McmetaStandardTableCellHost(self._table, r, self._hover_ctrl)
            op_row = QHBoxLayout(op_host)
            op_row.setContentsMargins(0, 0, 0, 0)
            op_row.setSpacing(6)

            btn_use = QPushButton("使用")
            btn_use.setCheckable(True)
            btn_use.setChecked(bool(item.active))
            btn_use.clicked.connect(lambda _c=False, item_id=item.id: self._on_use(item_id))
            attach_mcmeta_row_hover_to_button(btn_use, self._hover_ctrl, r)
            op_row.addWidget(btn_use)

            btn_del = QPushButton("删除")
            if item.id == LANG_SOURCE_BUILTIN:
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
        if src == LANG_SOURCE_BUILTIN:
            self._branch.clear()
            self._lang.clear()
            self._set_busy(False, "当前使用：内建语言")
            self._refresh_table()
            return
        self._load_branches()

    def _load_branches(self) -> None:
        self._branch.clear()
        self._lang.clear()
        self._set_busy(True, "正在获取分支列表…")
        self._branch_worker = _GithubBranchesWorker()
        self._branch_worker.finished_ok.connect(self._on_branches_loaded)
        self._branch_worker.finished_err.connect(self._on_branches_failed)
        self._branch_worker.start()

    def _on_branches_loaded(self, branches: list[str]) -> None:
        self._branch_worker = None
        self._branch.blockSignals(True)
        self._branch.clear()
        for b in branches:
            self._branch.addItem(b, b)
        self._branch.blockSignals(False)
        if self._branch.count() == 0:
            self._set_busy(False, "未找到可用分支")
            return
        self._set_busy(False, f"已加载分支：{self._branch.count()} 个")
        self._on_branch_changed()

    def _on_branches_failed(self, err: str) -> None:
        self._branch_worker = None
        self._set_busy(False, "获取分支失败")
        QMessageBox.warning(self, "游戏资源语言", f"获取分支失败：\n{err}")

    def _on_branch_changed(self) -> None:
        if self._source.currentData() != LANG_SOURCE_GITHUB_INVENTIVETALENT:
            return
        if self._lang_worker is not None and self._lang_worker.isRunning():
            return
        branch = str(self._branch.currentData() or "").strip()
        if not branch:
            self._lang.clear()
            return
        self._set_busy(True, f"正在获取语言列表（{branch}）…")
        self._lang_worker = _GithubLanguagesWorker(branch)
        self._lang_worker.finished_ok.connect(self._on_languages_loaded)
        self._lang_worker.finished_err.connect(self._on_languages_failed)
        self._lang_worker.start()

    def _on_languages_loaded(self, langs: list[str]) -> None:
        self._lang_worker = None
        self._lang.clear()
        for x in langs:
            self._lang.addItem(x, x)
        idx = self._lang.findData("zh_cn")
        if idx >= 0:
            self._lang.setCurrentIndex(idx)
        self._set_busy(False, f"已加载语言：{self._lang.count()} 个")

    def _on_languages_failed(self, err: str) -> None:
        self._lang_worker = None
        self._lang.clear()
        self._set_busy(False, "获取语言列表失败")
        QMessageBox.warning(self, "游戏资源语言", f"获取语言列表失败：\n{err}")

    def _on_download_clicked(self) -> None:
        branch = str(self._branch.currentData() or "").strip()
        lang = str(self._lang.currentData() or "").strip()
        if not branch or not lang:
            return
        self._set_busy(True, f"正在下载 {branch} / {lang} …")
        self._download_worker = _DownloadLanguageWorker(branch, lang)
        self._download_worker.finished_ok.connect(self._on_download_ok)
        self._download_worker.finished_err.connect(self._on_download_err)
        self._download_worker.start()

    def _on_download_ok(self, item: object) -> None:
        self._download_worker = None
        if not isinstance(item, InstalledLanguage):
            self._set_busy(False, "下载失败")
            return
        for i in range(len(self._installed)):
            self._installed[i].active = False
        item.active = True
        replaced = False
        for i, old in enumerate(self._installed):
            if old.id == item.id:
                self._installed[i] = item
                replaced = True
                break
        if not replaced:
            self._installed.append(item)
        save_installed_languages(self._installed)
        self._refresh_table()
        self._set_busy(False, f"已下载并装载：{item.language} @ {item.branch}")

    def _on_download_err(self, err: str) -> None:
        self._download_worker = None
        self._set_busy(False, "下载失败")
        QMessageBox.warning(self, "游戏资源语言", f"下载失败：\n{err}")

    def _on_use(self, item_id: str) -> None:
        if item_id == LANG_SOURCE_BUILTIN:
            for i in range(len(self._installed)):
                self._installed[i].active = False
            save_installed_languages(self._installed)
            self._refresh_table()
            self._set_busy(False, "已切换为内建语言")
            return
        self._installed = set_active_language(item_id)
        self._refresh_table()
        self._set_busy(False, "已切换当前使用语言")

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
            "删除语言",
            f"确定删除该语言包？\n\n语言：{target.language}\n分支：{target.branch}\n来源：{target.source}",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
            QMessageBox.StandardButton.No,
        )
        if r != QMessageBox.StandardButton.Yes:
            return
        self._installed = delete_installed_language(item_id)
        self._refresh_table()
        self._set_busy(False, "已删除语言包")

    def closeEvent(self, event) -> None:  # type: ignore[no-untyped-def]
        if self._hover_ctrl is not None:
            self._hover_ctrl.remove_from_viewport()
        super().closeEvent(event)
