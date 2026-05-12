import React, { useState } from 'react';
import { BlockIcon } from '../../../../components/BlockIcon';
import { Dropdown } from '../../../../components/Dropdown';
import { getBlockProperties, translateKey, translateValue, getAllBlocks } from '../../../../../src/business/facade';
import { applyReplaceBlocks, checkFileExists, dryRunReplaceBlocks, selectLitematicSavePath } from '../../../../../src/business/facade';

function PropertySelector({ blockId, value, onChange, isTo }: any) {
  const props = getBlockProperties(blockId);
  const keys = Object.keys(props).sort();
  if (keys.length === 0) return <span style={{fontSize: 12, opacity: 0.5}}>无属性</span>;
  
  return (
    <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
      {keys.map(k => (
        <div key={k} style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
          <span style={{ fontSize: 12 }}>{translateKey(k)}:</span>
          <Dropdown
            value={value[k] || (isTo ? "$keep" : "")}
            options={[
              ...(isTo ? [{label: "默认 (保持)", value: "$keep"}] : [{label: "所有", value: ""}]),
              ...props[k].map(v => ({ label: `${translateValue(k, v)} (${v})`, value: v }))
            ]}
            onChange={v => {
              const nv = { ...value };
              if (!v) delete nv[k]; else nv[k] = v;
              onChange(nv);
            }}
          />
        </div>
      ))}
    </div>
  );
}

function RuleRow({ rule, onChange, onRemove, onDuplicate }: any) {
  return (
    <div className="rule-row">
      <div className="rule-block">
        <div style={{fontWeight: 'bold', marginBottom: 4}}>替换前 (From)</div>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 8 }}>
          <BlockIcon blockId={rule.match.name} />
          <input className="input" style={{flex: 1}} value={rule.match.name} onChange={e => onChange({...rule, match: {...rule.match, name: e.target.value}})} placeholder="minecraft:stone" />
        </div>
        <PropertySelector blockId={rule.match.name} value={rule.match.properties || {}} onChange={(p: any) => onChange({...rule, match: {...rule.match, properties: p}})} isTo={false} />
      </div>
      
      <div style={{ fontSize: 24, fontWeight: 'bold', margin: '0 8px' }}>→</div>
      
      <div className="rule-block">
        <div style={{fontWeight: 'bold', marginBottom: 4}}>替换后 (To)</div>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 8 }}>
          <BlockIcon blockId={rule.replace.name} />
          <input className="input" style={{flex: 1}} value={rule.replace.name} onChange={e => onChange({...rule, replace: {...rule.replace, name: e.target.value}})} placeholder="minecraft:stone" />
        </div>
        <PropertySelector blockId={rule.replace.name} value={rule.replace.properties || {}} onChange={(p: any) => onChange({...rule, replace: {...rule.replace, properties: p}})} isTo={true} />
        <div style={{ marginTop: 8, display: 'flex', gap: 8, alignItems: 'center' }}>
          <span style={{ fontSize: 12 }}>状态模式:</span>
          <Dropdown value={rule.property_mode} options={[{label: "保持同名状态 (merge)", value: "merge"}, {label: "使用目标默认状态 (drop)", value: "drop"}, {label: "仅使用手动指定 (replace)", value: "replace"}]} onChange={v => onChange({...rule, property_mode: v})} />
        </div>
      </div>
      
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        <button className="btn" onClick={onDuplicate}>复制</button>
        <button className="btn" onClick={onRemove}>删除</button>
      </div>
    </div>
  );
}

export function ReplacePage({ currentFile }: any) {
  const [rules, setRules] = useState<any[]>([]);
  const [log, setLog] = useState("");
  const [dryRunRes, setDryRunRes] = useState("");
  const [showDialog, setShowDialog] = useState(false);
  const [outPath, setOutPath] = useState("");

  if (!currentFile) {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', height: '100%', opacity: 0.7 }}>
        <h2>方块替换</h2>
        <p>请先在投影库中选择并打开一个 .litematic 文件</p>
      </div>
    );
  }

  const handleAdd = () => setRules([...rules, { match: { name: "minecraft:stone", properties: {} }, replace: { name: "minecraft:air", properties: {} }, property_mode: "merge" }]);

  const handleDryRun = async () => {
    if (rules.length === 0) return alert("请至少添加一条替换规则。");
    setLog("> 执行: replace-blocks (Dry-run)");
    try {
      const res = await dryRunReplaceBlocks(currentFile, rules);
      setDryRunRes(res);
      setOutPath(currentFile.replace(".litematic", ".replaced.litematic"));
      setShowDialog(true);
      setLog(log + "\n" + res);
    } catch(e: any) {
      alert("执行失败: " + e);
      setLog(log + "\n执行失败: " + e);
    }
  };

  const handleApply = async () => {
    if (!outPath || outPath === currentFile) return alert("输出路径不能与输入路径相同。");
    if (await checkFileExists(outPath)) return alert("输出文件已存在，请重命名。");
    setShowDialog(false);
    setLog("> 执行: replace-blocks (Apply)");
    try {
      const res = await applyReplaceBlocks(currentFile, outPath);
      alert("替换完成！\n文件已保存至:\n" + outPath);
      setLog(log + "\n" + res);
    } catch(e: any) {
      alert("后端返回错误代码: " + e);
      setLog(log + "\n" + e);
    }
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', gap: 12 }}>
      <div style={{ flex: 1, overflowY: 'auto', border: '1px solid var(--border)', background: 'var(--bg)', padding: 12 }}>
        {rules.length === 0 && <div style={{ opacity: 0.5, fontStyle: 'italic' }}>无规则</div>}
        {rules.map((r, i) => (
          <RuleRow key={i} rule={r} onChange={(nr: any) => { const n = [...rules]; n[i] = nr; setRules(n); }} onRemove={() => setRules(rules.filter((_, idx) => idx !== i))} onDuplicate={() => setRules([...rules, JSON.parse(JSON.stringify(r))])} />
        ))}
      </div>

      <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
        <button className="btn" onClick={handleAdd}>添加规则</button>
        <button className="btn" onClick={() => setRules([])}>清空规则</button>
        <div style={{ flex: 1 }} />
        <button className="btn" style={{ backgroundColor: '#0078d4', color: 'white', fontWeight: 'bold', minWidth: 150 }} onClick={handleDryRun}>替换方块 (Dry-run)</button>
      </div>

      <textarea readOnly value={log} placeholder="执行日志..." style={{ height: 80, border: '1px solid var(--border)', background: 'var(--surface)', color: 'var(--fg)', padding: 8, fontFamily: 'inherit', resize: 'none' }} />
      
      {showDialog && (
        <div className="dialog-overlay">
          <div className="dialog-content">
            <h3 style={{marginTop: 0}}>确认替换 - 预览</h3>
            <div>Dry-run 结果摘要:</div>
            <textarea readOnly value={dryRunRes} style={{ width: '100%', height: 200, marginTop: 8, marginBottom: 16, background: 'var(--bg)', color: 'var(--fg)', border: '1px solid var(--border)', padding: 8, fontFamily: 'inherit', resize: 'none' }} />
            <div style={{ marginBottom: 8 }}>选择输出路径:</div>
            <div style={{ display: 'flex', gap: 8, marginBottom: 16 }}>
              <input className="input" style={{ flex: 1 }} value={outPath} readOnly />
              <button className="btn" onClick={async () => {
                const res = await selectLitematicSavePath(outPath);
                if (res) setOutPath(res);
              }}>浏览...</button>
            </div>
            <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
              <button className="btn" onClick={() => setShowDialog(false)}>取消</button>
              <button className="btn" style={{ backgroundColor: '#0078d4', color: 'white', fontWeight: 'bold' }} onClick={handleApply}>开始替换</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}


