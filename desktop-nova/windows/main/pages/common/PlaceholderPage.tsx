export function PlaceholderPage({ title }: { title: string }) {
    return (
        <section className="page-pad placeholder-page">
            <h2>{title}</h2>
            <p>已恢复页面入口与布局风格；核心业务流程待接入 Tauri 后端。</p>
        </section>
    );
}
