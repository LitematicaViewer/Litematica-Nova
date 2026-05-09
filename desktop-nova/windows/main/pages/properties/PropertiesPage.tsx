import type { ActiveFile } from "../../../../shell/types";
import type { ProjectionAnalysis } from "../../../../services/novaBackendAdapter";
import { FormRow } from "../common/FormRow";

export function PropertiesPage({
    activeFile,
    analysis,
    onOpenFile,
    onOpenViewer
}: {
    activeFile: ActiveFile | null;
    analysis: ProjectionAnalysis | null;
    onOpenFile: () => void;
    onOpenViewer: () => void;
}) {
    const metadata = analysis?.metadata;
    const size = metadata?.enclosing_size;
    const density =
        metadata && metadata.total_volume
            ? `${((metadata.total_blocks / metadata.total_volume) * 100).toFixed(2)}%`
            : "-";

    return (
        <section className="properties-page">
            <div className="scroll-body">
                <div className="file-row">
                    <button type="button" onClick={onOpenFile}>选择 .litematic...</button>
                    <button type="button" disabled={!activeFile} onClick={onOpenViewer}>打开弹窗 Viewer</button>
                    <span className="path-label">{activeFile ? activeFile.path : "尚未选择文件"}</span>
                </div>
                <fieldset>
                    <legend>文件</legend>
                    <FormRow label="文件名">
                        <input readOnly value={activeFile?.name || ""} />
                    </FormRow>
                </fieldset>
                <div className="two-column">
                    <fieldset>
                        <legend>元数据</legend>
                        <FormRow label="内部名称"><input readOnly value={metadata?.name || ""} /></FormRow>
                        <FormRow label="作者"><input readOnly value={metadata?.author || ""} /></FormRow>
                        <FormRow label="描述"><input readOnly value={metadata?.description || ""} /></FormRow>
                        <FormRow label="创建时间"><input readOnly value={metadata?.time_created ?? "-"} /></FormRow>
                        <FormRow label="修改时间"><input readOnly value={metadata?.time_modified ?? "-"} /></FormRow>
                        <FormRow label="尺寸">
                            <div className="inline-fields">
                                <label>x:<input readOnly value={size?.x ?? 0} /></label>
                                <label>y:<input readOnly value={size?.y ?? 0} /></label>
                                <label>z:<input readOnly value={size?.z ?? 0} /></label>
                            </div>
                        </FormRow>
                        <FormRow label="体积">
                            <div className="inline-fields">
                                <label>方块:<input readOnly value={metadata?.total_blocks ?? 0} /></label>
                                <label>总量:<input readOnly value={metadata?.total_volume ?? 0} /></label>
                                <label>密度:<input readOnly value={density} /></label>
                            </div>
                        </FormRow>
                    </fieldset>
                    <div className="side-stack">
                        <fieldset>
                            <legend>后端</legend>
                            <div className="preview-box">LBA</div>
                            <p className="muted">数据来自 bin/viewer-backend/litematica_core.exe analyze。</p>
                        </fieldset>
                        <fieldset>
                            <legend>3D Viewer</legend>
                            <div className="render-placeholder">
                                本轮先接当前原生弹窗 Viewer；embedded viewer 后续继续接入。
                            </div>
                        </fieldset>
                    </div>
                </div>
                <fieldset>
                    <legend>版本</legend>
                    <FormRow label="Litematic 版本"><input readOnly value={metadata?.litematic_version ?? "-"} /></FormRow>
                    <FormRow label="Litematic 子版本"><input readOnly value={metadata?.litematic_subversion ?? "-"} /></FormRow>
                    <FormRow label="Minecraft 数据版本"><input readOnly value={metadata?.minecraft_data_version ?? "-"} /></FormRow>
                    <p className="muted">Nova MVP 中暂为只读。</p>
                </fieldset>
                <fieldset>
                    <legend>区域</legend>
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
                            {analysis?.regions.length ? (
                                analysis.regions.map((region) => (
                                    <tr key={region.name}>
                                        <td>{region.name}</td>
                                        <td>{region.size.x}</td>
                                        <td>{region.size.y}</td>
                                        <td>{region.size.z}</td>
                                        <td>{region.position.x}</td>
                                        <td>{region.position.y}</td>
                                        <td>{region.position.z}</td>
                                    </tr>
                                ))
                            ) : (
                                <tr>
                                    <td colSpan={7}>尚未加载区域数据。</td>
                                </tr>
                            )}
                        </tbody>
                    </table>
                </fieldset>
            </div>
            <footer className="footer-actions">
                <span className="toolbar-spacer" />
                <button type="button" disabled>保存元数据（后续接入）</button>
                <button type="button" disabled>另存为</button>
                <button type="button" disabled>恢复</button>
                <button type="button" disabled>转换</button>
            </footer>
        </section>
    );
}
