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
                    Nova 外观试验入口。这里使用当前 Litematica-BA 后端，分析、投影库、生成、替换和原生 Viewer
                    都通过 adapter 调用现有能力。
                </p>
                <button type="button" onClick={onOpenFile}>打开 .litematic</button>
                <button type="button" onClick={onOpenStatistics}>查看统计</button>
                <button type="button" onClick={onOpenRender}>进入 3D 渲染</button>
            </div>
        </section>
    );
}
