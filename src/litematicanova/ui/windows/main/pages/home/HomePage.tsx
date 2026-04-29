export function HomePage({
    onOpenFile,
    onOpenStatistics,
    onOpenRender
}: {
    onOpenFile: () => void;
    onOpenStatistics: () => void;
    onOpenRender: () => void;
}) {
    return (
        <section className="home-page">
            <div className="home-center">
                <h1>Litematica Nova</h1>
                <p>
                    第一阶段主链：打开 .litematic、做结构分析、构建单一 3D cache，
                    再用同一份 cache 驱动嵌入式预览和弹窗 viewer。
                </p>
                <button type="button" onClick={onOpenFile}>打开 .litematic</button>
                <button type="button" onClick={onOpenStatistics}>查看分析统计</button>
                <button type="button" onClick={onOpenRender}>进入 3D 渲染页</button>
            </div>
        </section>
    );
}