export function LibraryPage() {
    return (
        <section className="page-pad library-page">
            <div className="toolbar">
                <h2>投影库</h2>
                <span className="muted">0 条记录，0 条需关注</span>
                <span className="toolbar-spacer" />
                <label>
                    最近保留
                    <select defaultValue="20">
                        <option>10</option>
                        <option>20</option>
                        <option>30</option>
                        <option>50</option>
                        <option>100</option>
                    </select>
                </label>
                <button type="button">刷新校验</button>
            </div>
            <div className="filter-row">
                <input placeholder="搜索投影名、原文件名或原地址" />
                <select defaultValue="">
                    <option value="">全部标签</option>
                </select>
                <select defaultValue="manual">
                    <option value="manual">手动排序</option>
                    <option value="recent">最近导入/使用</option>
                    <option value="name">名称</option>
                </select>
                <button type="button">应用排序</button>
            </div>
            <div className="empty-state">投影库还没有内容。请先通过默认导入打开一个 .litematic 文件。</div>
        </section>
    );
}