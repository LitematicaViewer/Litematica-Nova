import type { ChangeEvent } from "react";

import { FormRow } from "../common/FormRow";

type OptionsPageProps = {
    theme: string;
    onThemeChange: (theme: string) => void;
};

export function OptionsPage({ theme, onThemeChange }: OptionsPageProps) {
    const handleThemeChange = (event: ChangeEvent<HTMLSelectElement>) => {
        onThemeChange(event.target.value);
    };
    return (
        <section className="page-pad options-page">
            <fieldset>
                <legend>界面</legend>
                <FormRow label="主题：">
                    <select value={theme} onChange={handleThemeChange}>
                        <option>WebDefault</option>
                        <option>Bootstrap5</option>
                        <option>Metro10</option>
                        <option>Minecraft</option>
                    </select>
                </FormRow>
                <label><input type="checkbox" /> 显示磁贴网格（仅影响可拖拽磁贴区域）</label>
                <FormRow label="自动放置磁贴优先列数："><input type="number" min="1" max="64" defaultValue="12" /></FormRow>
                <FormRow label="磁贴视图右侧留白："><input type="number" min="0" max="300" defaultValue="64" /></FormRow>
            </fieldset>
            <fieldset>
                <legend>调试</legend>
                <label><input type="checkbox" defaultChecked /> 在侧栏显示「UI 测试」入口</label>
                <label><input type="checkbox" /> 显示控件信息（已弃用，现在按下F12即可查看控件信息）</label>
                <label><input type="checkbox" /> 性能测试（洋红圆动画 + 左下角 FPS 浮层，均不拦截鼠标）</label>
            </fieldset>
            <fieldset>
                <legend>性能与预加载</legend>
                <FormRow label="方块图标预加载时机：">
                    <select><option>软件启动时（默认）</option><option>首次加载投影文件时</option><option>首次点击材料列表或分层时</option><option>不加载方块图标（回退手段）</option></select>
                </FormRow>
                <FormRow label="材料列表计算时机：">
                    <select><option>首次加载投影文件时（默认）</option><option>首次点击材料列表时</option></select>
                </FormRow>
                <FormRow label="材料列表扫描方案：">
                    <select><option>Native Bridge 扫描（高性能，仅整投影）</option><option>Python 扫描（支持子区域）</option></select>
                </FormRow>
            </fieldset>
            <fieldset>
                <legend>游戏资源</legend>
                <button type="button">管理游戏语言...</button>
                <p className="muted">语言将被用于：“材料列表”</p>
                <button type="button">管理方块图标...</button>
                <p className="muted">方块图标将被用于：“材料列表”、“分层”</p>
                <button type="button">管理物品图标...</button>
                <p className="muted">物品图标将被用于：“材料列表”、“分层”</p>
            </fieldset>
        </section>
    );
}