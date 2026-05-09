import type { LibraryRecord } from "../../../../services/novaBackendAdapter";

export function LibraryPage({
    records,
    onSelectRecord
}: {
    records: LibraryRecord[];
    onSelectRecord: (record: LibraryRecord) => void;
}) {
    return (
        <section className="page-pad library-page">
            <div className="toolbar">
                <h2>投影库</h2>
                <span className="muted">{records.length} 条 AppData 记录</span>
                <span className="toolbar-spacer" />
                <span className="muted">Nova MVP 只读列表；导入请从首页打开文件。</span>
            </div>
            <div className="filter-row">
                <input readOnly placeholder="搜索和拖拽排序后续接入；当前不显示假数据。" />
                <select value="manual" disabled>
                    <option value="manual">手动排序</option>
                </select>
            </div>
            {records.length ? (
                <div className="library-list">
                    {records.map((record) => (
                        <button
                            className="library-row"
                            key={record.path}
                            type="button"
                            onClick={() => onSelectRecord(record)}
                        >
                            <strong>{record.displayName || record.fileName || record.path}</strong>
                            <span>{record.path}</span>
                            <small>{record.status || "unknown"}</small>
                        </button>
                    ))}
                </div>
            ) : (
                <div className="empty-state">
                    AppData 投影库暂无记录。此页面不会显示 Nova mock 数据。
                </div>
            )}
        </section>
    );
}
