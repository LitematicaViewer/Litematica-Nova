import type { ActiveFile } from "../../../../shell/types";
import { FormRow } from "../common/FormRow";

export function PropertiesPage({
    activeFile,
    onOpenFile
}: {
    activeFile: ActiveFile | null;
    onOpenFile: () => void;
}) {
    return (
        <section className="properties-page">
            <div className="scroll-body">
                <div className="file-row">
                    <button type="button" onClick={onOpenFile}>选择文件...</button>
                    <button type="button">在库中选择...</button>
                    <span className="path-label">{activeFile ? activeFile.path : "当前未激活文件"}</span>
                </div>
                <fieldset>
                    <legend>文件</legend>
                    <FormRow label="文件名称：">
                        <input defaultValue={activeFile?.name || ""} />
                    </FormRow>
                </fieldset>
                <div className="two-column">
                    <fieldset>
                        <legend>SNBT 元数据</legend>
                        <FormRow label="内部名称："><input defaultValue={activeFile?.name || ""} /></FormRow>
                        <FormRow label="作者："><input /></FormRow>
                        <FormRow label="描述："><input /></FormRow>
                        <FormRow label="创建时间："><input readOnly value="-" /></FormRow>
                        <FormRow label="修改时间："><input readOnly value="-" /></FormRow>
                        <FormRow label="尺寸：">
                            <div className="inline-fields">
                                <label>x:<input readOnly value="0" /></label>
                                <label>y:<input readOnly value="0" /></label>
                                <label>z:<input readOnly value="0" /></label>
                            </div>
                        </FormRow>
                        <FormRow label="体积：">
                            <div className="inline-fields">
                                <label>方块:<input readOnly value="0" /></label>
                                <label>总计:<input readOnly value="0" /></label>
                                <label>密度:<input readOnly value="-" /></label>
                            </div>
                        </FormRow>
                    </fieldset>
                    <div className="side-stack">
                        <fieldset>
                            <legend>预览图</legend>
                            <div className="preview-box">140 x 140</div>
                            <p className="muted">PreviewImageData: 0 项</p>
                            <label>
                                缩放采样方式：
                                <select defaultValue="smooth">
                                    <option value="smooth">平滑缩放</option>
                                    <option value="nearest">邻近像素</option>
                                </select>
                            </label>
                            <div className="button-row">
                                <button type="button">清空预览图</button>
                                <button type="button">导入预览图</button>
                            </div>
                        </fieldset>
                        <fieldset>
                            <legend>3D 渲染图</legend>
                            <div className="render-placeholder">
                                渲染页还没有构建过 3D cache。构建完成后这里会显示固定摄像机的嵌入式 3D 渲染图。
                            </div>
                        </fieldset>
                    </div>
                </div>
                <fieldset>
                    <legend>版本信息</legend>
                    <FormRow label="投影文件版本："><input readOnly value="-" /></FormRow>
                    <FormRow label="Minecraft 数据版本："><input readOnly value="-" /></FormRow>
                    <p className="muted">只读：来自当前 .litematic 文件；这里不是版本转换入口。</p>
                </fieldset>
                <fieldset>
                    <legend>区域列表</legend>
                    <table>
                        <thead>
                            <tr>
                                <th>名称</th>
                                <th>Size X</th>
                                <th>Size Y</th>
                                <th>Size Z</th>
                                <th>Pos X</th>
                                <th>Pos Y</th>
                                <th>Pos Z</th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr>
                                <td colSpan={7}>当前文件无子区域，或 Regions 无法解析。</td>
                            </tr>
                        </tbody>
                    </table>
                </fieldset>
            </div>
            <footer className="footer-actions">
                <span className="toolbar-spacer" />
                <button type="button">保存</button>
                <button type="button">另存为</button>
                <button type="button">恢复默认值</button>
                <button type="button">转换格式</button>
            </footer>
        </section>
    );
}