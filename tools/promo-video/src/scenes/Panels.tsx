import React from 'react';
import {Img, staticFile,interpolate} from 'remotion';
import {type Locale,copy} from '../locales';
import {theme} from '../theme';
import {assets} from '../assets';
import {ramp,clamp} from '../motion/motion';
import {Button,Badge,Label,PageTitle,Rule,Icon} from '../components/ProductWindow';
import layer32 from '../../assets/original/layer-32.json';
import layer44 from '../../assets/original/layer-44.json';
import layer60 from '../../assets/original/layer-60.json';

const fmt=(n:number)=>Math.round(n).toLocaleString('en-US');
export const materials = [
 {zh:'平滑石英块',en:'Smooth quartz',count:20744,color:'#e9e7df'},
 {zh:'磨制深板岩',en:'Polished deepslate',count:13160,color:'#555963'},
 {zh:'氧化铜',en:'Oxidized copper',count:7770,color:'#5e9a85'},
 {zh:'石英砖',en:'Quartz bricks',count:4855,color:'#d8d8d1'},
 {zh:'淡蓝色染色玻璃',en:'Light blue stained glass',count:568,color:'#99cbd8'},
 {zh:'切制铜块',en:'Cut copper',count:84,color:'#ba7d63'},
 {zh:'海晶灯',en:'Sea lantern',count:8,color:'#d5ece6'},
];
const Block=({color,size=32}:{color:string;size?:number})=><div style={{width:size,height:size,background:color,
 boxShadow:`inset 0 5px 0 #fff2,inset -6px -6px 0 #0003`,border:'1px solid #fff2',flexShrink:0}}/>;
const Field=({label,value}:{label:string;value:string})=><div style={{display:'flex',justifyContent:'space-between',gap:24,padding:'16px 0',borderBottom:`1px solid ${theme.border}`}}><span style={{color:theme.mutedForeground}}>{label}</span><strong style={{fontWeight:500}}>{value}</strong></div>;

export const LibraryPanel=({locale,variant,f}:{locale:Locale;variant:number;f:number})=>{
 const c=copy(locale),active=f>=42;const typed=c.searchTerm.slice(0,Math.max(0,Math.floor((f-48)/6)));
 const search=variant===1?typed:variant===2?c.searchTerm:'';
 const entries=(variant===2&&active)||(variant===5&&active)?1:variant===3&&active?3:6;
 return <><PageTitle subtitle={locale==='zh-CN'?'公开原创演示投影 · 0.4 工作区':'Original public demo schematic · 0.4 workspace'}>{c.nav[1]}</PageTitle>
  <div style={{display:'flex',gap:12,alignItems:'center'}}><Button active action={variant===0}><Icon type={1}/>{c.import}</Button>
   <div data-nova-action={variant===1||variant===2?'primary':undefined} style={{flex:1,border:`1px solid ${variant===1?theme.blue:theme.border}`,borderRadius:6,padding:'12px 15px',fontSize:20,color:search?theme.foreground:theme.mutedForeground}}>⌕　{search||c.search}{variant===1&&f>48?'│':''}</div>
   <Button>▦</Button></div><Rule/>
  <div style={{display:'flex',gap:26}}><div style={{width:160,flexShrink:0}}>{c.categories.map((v,i)=><div key={v} data-nova-action={variant===3&&i===1?'primary':undefined} style={{padding:'13px 10px',fontSize:19,background:i===(variant===3&&active?1:variant===5&&active?3:0)?theme.accent:'transparent',borderRadius:6,marginBottom:5}}>{v}</div>)}</div>
   <div style={{flex:1}}><Label>{variant===2?`1 ${c.matches}`:c.all}</Label><div style={{display:'grid',gridTemplateColumns:variant===4?'repeat(2, 1fr)':'repeat(3, 1fr)',gap:16}}>
    {Array.from({length:variant===4?2:entries},(_,i)=>{const land=variant===0?ramp(f,44+i*5,60+i*5):1;return <div key={i} data-nova-action={variant===4&&i===0?'primary':undefined} style={{background:theme.card,border:`1px solid ${i===0&&(variant>=2)&&(variant!==4||active)?theme.blue:theme.border}`,borderRadius:7,overflow:'hidden',
     opacity:land,transform:`translate(${(1-land)*(i%2?-70:80)}px,${(1-land)*-95}px) rotateX(${(1-land)*16}deg)`,boxShadow:land<1?'0 15px 20px #0007':'none'}}>
     <div style={{height:139,position:'relative',background:'#11151a'}}><Img src={staticFile(assets.hero)} style={{width:'100%',height:'100%',objectFit:'contain'}}/>{variant===5&&i===0&&<span data-nova-action="primary" style={{position:'absolute',right:10,top:7,color:active?'#e9d7a8':'#ddd',fontSize:26,padding:5}}>{active?'★':'☆'}</span>}</div>
     <div style={{padding:'12px 13px'}}><div style={{fontSize:19,fontWeight:600}}>{c.gallery}{i?` · ${String.fromCharCode(65+i)}`:''}</div><div style={{fontSize:15,color:theme.mutedForeground,marginTop:7}}>.litematic　96 × 100 × 72</div></div>
    </div>})}</div></div>
   {variant===4&&<div style={{width:260,opacity:ramp(f,42,62),borderLeft:`1px solid ${theme.border}`,paddingLeft:22}}><h3 style={{fontSize:23,margin:'0 0 20px'}}>{c.details}</h3><Field label={c.name} value={c.gallery}/><Field label="XYZ" value="96 × 100 × 72"/><Field label={c.blocks} value="55,909"/><Field label={c.version} value="3465"/><div style={{marginTop:22}}><Button active>{c.preview}</Button></div></div>}
  </div></>;
};

export const ViewerPanel=({locale,variant,f}:{locale:Locale;variant:number;f:number})=>{
 const c=copy(locale);const mode=variant===2?(f>=42?2:1):1;
 const image=variant===0||variant===3?assets.standard:null;
 return <><PageTitle subtitle={c.gallery}>{c.nav[3]}</PageTitle>
 <div style={{display:'flex',gap:12}}>{c.modes.map((v,i)=><Button action={variant===2&&i===2} active={mode===i} key={v}>{v}</Button>)}<div style={{marginLeft:'auto'}}><Badge>{image?c.ready:locale==='zh-CN'?'模式选择演示':'Mode selector demo'}</Badge></div></div>
 <div style={{height:460,marginTop:23,background:'#10151b',borderRadius:6,position:'relative',overflow:'hidden'}}>
  {image?<><Img src={staticFile(image)} style={{width:'100%',height:'100%',objectFit:'contain'}}/><span style={{position:'absolute',bottom:15,left:18,fontSize:16,color:'#d9e4ee',background:'#101318c0',padding:'6px 10px',borderRadius:5}}>{c.real} · {c.modes[mode]}</span></>:
  <div style={{position:'absolute',inset:0,padding:'70px 65px',display:'flex',flexDirection:'column',justifyContent:'center',gap:35}}><div style={{fontSize:40,fontWeight:500}}>{variant===1?(locale==='zh-CN'?'选择适合任务的加载方式':'Choose how your scene loads'):(locale==='zh-CN'?'三种模式，同一个工作区':'Three modes. One workspace.')}</div><div style={{display:'flex',gap:24}}><Button active={f<42}>□ {c.realtime}</Button><Button action={variant===1} active={f>=42}>☑ {c.preload}</Button></div><div style={{fontSize:21,color:theme.mutedForeground,lineHeight:1.6,maxWidth:880}}>{locale==='zh-CN'?'这里展示正式0.4的模式选择控件，不使用其他模式的画面冒充渲染结果。':'Published 0.4 mode controls. No other mode’s image is substituted for a render result.'}</div></div>}
 </div>
 {variant===3&&<div style={{position:'absolute',bottom:62,left:62,display:'flex',gap:14}}><Button action active={f>=42}>{f>=42?c.fly:c.drag}</Button>{f>=42&&<Badge>W A S D　Space / Ctrl　Esc</Badge>}</div>}
 </>;
};

export const LayerPanel=({locale,variant,f}:{locale:Locale;variant:number;f:number})=>{
 const c=copy(locale),changed=f>=42;
 const data=variant===0?(changed?layer44:layer32):(changed?layer60:layer44);
 const path=data.blocks.map((b:{x:number;z:number})=>`M${b.x},${b.z}h1v1h-1Z`).join('');
 return <><PageTitle subtitle={locale==='zh-CN'?'真实 Core 二维分层数据视图':'Actual Core 2D layer-data view'}>{c.layer}</PageTitle>
 <div style={{display:'flex',gap:28,height:530}}><div style={{flex:1,border:`1px solid ${theme.border}`,borderRadius:6,background:'#11161b',display:'flex',alignItems:'center',justifyContent:'center',position:'relative'}}>
  <svg viewBox="-3 -3 102 78" style={{width:700,height:510}} shapeRendering="crispEdges"><defs><pattern id="grid" width="8" height="8" patternUnits="userSpaceOnUse"><path d="M8 0H0V8" fill="none" stroke="#263440" strokeWidth=".15"/></pattern></defs><rect width="96" height="72" fill="url(#grid)"/><path d={path} fill={theme.blue}/></svg>
  <span style={{position:'absolute',left:16,bottom:15,fontSize:18}}>X 0–95　 Z 0–71</span></div>
  <div style={{width:230}}><Label>{c.height}</Label><div style={{fontSize:78,fontWeight:300,lineHeight:1.2}}>{data.y}</div><div data-nova-action="primary" style={{marginTop:26,height:7,background:theme.border,borderRadius:4}}><div style={{height:'100%',width:`${data.y/99*100}%`,background:theme.blue,borderRadius:4}}/></div><Field label="Y" value="0–99"/><Field label={c.blocks} value={fmt(data.blocks.length)}/><div style={{fontSize:18,color:theme.mutedForeground,lineHeight:1.6,marginTop:30}}>{locale==='zh-CN'?'按实际占用显示截面；不是三维 GPU 分层演出。':'A real occupied-cell slice, not a simulated 3D GPU cut.'}</div></div>
 </div></>;
};

export const StatsPanel=({locale,variant,f}:{locale:Locale;variant:number;f:number})=>{
 const c=copy(locale);return <><PageTitle subtitle={c.gallery}>{variant===0?c.overview:c.materials}</PageTitle>
 <div style={{display:'flex',gap:20,marginBottom:26}}>{[[c.blocks,'55,909'],[locale==='zh-CN'?'材料种类':'Material types','15'],[locale==='zh-CN'?'区域':'Regions','1']].map(([title,value],i)=><div key={title} style={{flex:1,padding:'18px 22px',border:`1px solid ${theme.border}`,borderRadius:6,background:theme.card,transform:`translateY(${(1-ramp(f,i*8,24+i*8))*30}px)`,opacity:ramp(f,i*8,24+i*8)}}><Label>{title}</Label><div style={{fontSize:47,fontWeight:300}}>{value}</div></div>)}</div>
 <div style={{display:'flex',gap:12,alignItems:'center',marginBottom:18}}><Button active={variant!==3||f<42}>{c.materials}</Button><Button action={variant===3} active={variant===3&&f>=42}>{c.containers}</Button><div data-nova-action={variant===2?'primary':undefined} style={{marginLeft:'auto'}}><Badge>{variant===2&&f>=42?c.stacks:locale==='zh-CN'?'个':'Items'}</Badge></div></div>
 {variant===3&&f>=42?<div style={{padding:'45px 25px',background:theme.card,border:`1px solid ${theme.border}`,borderRadius:7}}><div style={{fontSize:58,fontWeight:300}}>0</div><h3 style={{fontSize:25}}>{c.containers}</h3><p style={{fontSize:20,color:theme.mutedForeground}}>{locale==='zh-CN'?'当前演示投影没有容器；不会凭空添加物品。':'This demo schematic has no containers. No invented contents.'}</p></div>:
 <div style={{border:`1px solid ${theme.border}`,borderRadius:6,overflow:'hidden'}}>
  <div style={{display:'grid',gridTemplateColumns:'2fr 1fr 1fr',padding:'13px 20px',background:theme.secondary,fontSize:19,color:theme.mutedForeground}}><span>{c.item}</span><span>{c.required}</span><span>{variant===2?c.stacks:'ID'}</span></div>
  {materials.slice(variant===2?5:0,variant===2?7:5).map((m,i)=><div key={m.en} style={{display:'grid',gridTemplateColumns:'2fr 1fr 1fr',alignItems:'center',padding:'14px 20px',borderTop:`1px solid ${theme.border}`,background:i===0?theme.card:'transparent',opacity:ramp(f,10+i*7,26+i*7),transform:`perspective(900px) translateY(${(1-ramp(f,10+i*7,26+i*7))*-35}px) rotateX(${(1-ramp(f,10+i*7,26+i*7))*12}deg)`}}>
   <span style={{display:'flex',gap:14,alignItems:'center',fontSize:23}}><Block color={m.color}/>{locale==='zh-CN'?m.zh:m.en}</span><strong style={{fontSize:26,fontWeight:500}}>{fmt(m.count)}</strong><span style={{fontSize:18,color:theme.mutedForeground}}>{variant===2&&f>=42?`${Math.floor(m.count/64)} ${c.stacks} + ${m.count%64}`:variant===2?'—':m.en.toLowerCase().replaceAll(' ','_')}</span>
  </div>)}
 </div>}
 </>;
};

export const StockpilePanel=({locale,variant,f}:{locale:Locale;variant:number;f:number})=>{
 const c=copy(locale);const prepared=variant===0?0:variant===1?Math.round(interpolate(f,[42,72],[0,64],clamp)):64;
 const remain=84-prepared; const other=variant===3&&f<42;
 return <><PageTitle subtitle={locale==='zh-CN'?'独立任务 · 本地持久保存':'Independent tasks · saved locally'}>{c.nav[5]}</PageTitle>
  <div style={{display:'flex',gap:15,alignItems:'center',marginBottom:24}}><Button action={variant===3} active>{other?(locale==='zh-CN'?'第二次施工':'A second build'):c.task}</Button><Button action={variant===0}>{c.create}</Button><div style={{marginLeft:'auto'}}><Badge>{variant===4&&f>=42?c.saved:locale==='zh-CN'?'任务演示':'Task demonstration'}</Badge></div></div>
  <div style={{display:'flex',gap:18,marginBottom:25}}><div style={{flex:1,background:theme.card,border:`1px solid ${theme.border}`,borderRadius:7,padding:22}}><Label>{c.prepared} / {c.required}</Label><div style={{fontSize:38}}>{other?0:prepared} <span style={{fontSize:24,color:theme.mutedForeground}}>/ 55,909</span></div></div><div style={{flex:1,background:theme.card,border:`1px solid ${theme.border}`,borderRadius:7,padding:22}}><Label>{c.remaining}</Label><div style={{fontSize:38}}>{fmt(55909-(other?0:prepared))}</div></div></div>
  <div style={{opacity:variant===0?ramp(f,44,64):1,border:`1px solid ${theme.border}`,borderRadius:7,overflow:'hidden'}}><div style={{display:'grid',gridTemplateColumns:'1.7fr 1fr 1fr 1fr',padding:'15px 22px',fontSize:19,color:theme.mutedForeground,background:theme.secondary}}><span>{c.item}</span><span>{c.required}</span><span>{c.prepared}</span><span>{c.remaining}</span></div>
  {[materials[5],materials[4],materials[6]].map((m,i)=><div key={m.en} style={{display:'grid',gridTemplateColumns:'1.7fr 1fr 1fr 1fr',padding:'14px 22px',alignItems:'center',borderTop:`1px solid ${theme.border}`,background:i===0?theme.card:'transparent'}}><span style={{display:'flex',gap:14,alignItems:'center'}}><Block color={m.color}/>{locale==='zh-CN'?m.zh:m.en}</span><span>{m.count}</span><span data-nova-action={variant===1&&i===0?'primary':undefined} style={{border:`1px solid ${i===0?theme.blue:theme.border}`,padding:'8px 12px',borderRadius:5,width:100,background:theme.background}}>{i===0&&!other?prepared:0}</span><strong style={{fontWeight:500,color:i===0?theme.blue:theme.foreground}}>{i===0&&!other?remain:m.count}</strong></div>)}
  </div>
  <div style={{display:'flex',alignItems:'center',gap:18,marginTop:14}}><div style={{flex:1,height:8,background:theme.border,borderRadius:5}}><div style={{width:`${other?0:prepared/84*100}%`,height:'100%',background:theme.blue,borderRadius:5}}/></div><span style={{fontSize:23}}>{(other?0:prepared/84*100).toFixed(1)}%</span><Badge>{locale==='zh-CN'?'当前材料进度':'Selected item progress'}</Badge></div>
 </>;
};

export const PropertiesPanel=({locale,variant}:{locale:Locale;variant:number})=>{
 const c=copy(locale);return <><PageTitle subtitle={c.gallery}>{c.nav[2]}</PageTitle><div style={{display:'flex',gap:34}}><div style={{flex:1}}><Img src={staticFile(assets.hero)} style={{width:'100%',height:390,objectFit:'contain',background:'#11161c',borderRadius:6}}/><div style={{marginTop:18}}><Badge>{c.real}</Badge></div></div><div style={{width:400}}><Field label={c.name} value="Nova Azure Observatory"/><Field label="XYZ" value="96 × 100 × 72"/><Field label={c.blocks} value="55,909"/><Field label={locale==='zh-CN'?'区域':'Regions'} value="1"/><Field label={locale==='zh-CN'?'实体':'Entities'} value="0"/><Field label={c.version} value="3465"/>{variant===1&&<div style={{marginTop:24,padding:18,border:`1px solid ${theme.blue}`,borderRadius:6,color:theme.blue,fontSize:19,lineHeight:1.6}}>{locale==='zh-CN'?'导出前检查数据是否可表达；不静默丢弃实体或附加数据。':'Checks target capabilities before writing. No silent loss of entities or extra data.'}</div>}</div></div></>;
};

export const ReplacePanel=({locale,variant,f}:{locale:Locale;variant:number;f:number})=>{
 const c=copy(locale),done=variant>=1&&f>=42;
 return <><PageTitle subtitle={c.gallery}>{c.nav[6]}</PageTitle><div style={{display:'flex',gap:28,alignItems:'center',marginTop:35}}>
  <div style={{flex:1,padding:30,background:theme.card,border:`1px solid ${theme.border}`,borderRadius:7}}><Label>{c.source}</Label><div style={{display:'flex',gap:18,alignItems:'center',fontSize:30}}><Block color="#ba7d63" size={64}/>{locale==='zh-CN'?'切制铜块':'Cut copper'}</div><div style={{marginTop:24,color:theme.mutedForeground,fontSize:22}}>minecraft:cut_copper</div></div>
  <div style={{fontSize:50,color:theme.blue}}>→</div><div style={{flex:1,padding:30,background:theme.card,border:`1px solid ${theme.blue}`,borderRadius:7}}><Label>{c.target}</Label><div style={{display:'flex',gap:18,alignItems:'center',fontSize:30}}><Block color="#5e9a85" size={64}/>{locale==='zh-CN'?'氧化铜':'Oxidized copper'}</div><div style={{marginTop:24,color:theme.mutedForeground,fontSize:22}}>minecraft:oxidized_copper</div></div></div>
  <div style={{marginTop:35,display:'flex',gap:20,alignItems:'center'}}><Button action={variant!==0} active>{variant===2?c.saveAs:c.replace}</Button><span style={{color:theme.mutedForeground}}>{locale==='zh-CN'?'匹配':'Matched'} 84</span></div>
  <div style={{marginTop:30,padding:25,border:`1px solid ${done?theme.blue:theme.border}`,borderRadius:6,background:theme.card,fontSize:28}}>{variant===2&&done?'Nova-Observatory-edited.litematic':done?'cut_copper 84 → oxidized_copper 84':locale==='zh-CN'?'源文件保持不变':'Original schematic stays unchanged'}</div>
  <div style={{fontSize:18,color:theme.mutedForeground,marginTop:22}}>{locale==='zh-CN'?'替换控件与结果状态演示；不把原始 GPU 画面伪装成替换后的视图。':'Replacement controls and result-state demonstration; no fabricated edited GPU view.'}</div>
 </>;
};

export const ExportPanel=({locale,variant,f}:{locale:Locale;variant:number;f:number})=>{
 const c=copy(locale),world=variant===3||variant===4,done=variant===0||variant===1?f>=48:true;
 return <><PageTitle subtitle={c.gallery}>{world?c.world:c.export}</PageTitle>
  <div style={{display:'flex',gap:12,marginBottom:28}}><Button active={variant===0}>{c.csv}</Button><Button active={variant===1||variant===2}>{c.html}</Button><Button active={world}>{c.world}</Button></div>
  {world?<div style={{display:'flex',gap:28}}><div style={{flex:1,padding:28,border:`1px solid ${theme.border}`,borderRadius:7,background:theme.card}}><Label>{c.targetVersion}</Label><div style={{fontSize:34}}>Minecraft Java 1.20.1</div><Rule/><Field label={locale==='zh-CN'?'数据保留':'Data preservation'} value={locale==='zh-CN'?'按目标能力检查':'Target capability checks'}/><Field label={locale==='zh-CN'?'坐标':'Coordinates'} value="XYZ"/><div style={{fontSize:22,lineHeight:1.7,marginTop:22,color:theme.mutedForeground}}>{locale==='zh-CN'?'不能等价迁移的实体或附加数据会显示具体原因，而不是丢弃后继续。':'Entities or extra data that cannot be preserved produce a clear reason, instead of silent loss.'}</div></div><div style={{width:430,padding:24,border:`1px solid ${theme.border}`,borderRadius:7,background:theme.card}}>
 <Label>{c.files}</Label><div style={{opacity:variant===3?ramp(f,44,66):1}}>
  <div style={{fontSize:23,color:theme.blue,margin:'18px 0'}}>Nova-Observatory-World/</div>
  <div style={{fontFamily:'Consolas, monospace',fontSize:21,lineHeight:2.3,paddingLeft:16}}>level.dat <span style={{fontSize:16,color:theme.mutedForeground}}>553 B</span><br/>region/r.0.0.mca<br/><span style={{fontSize:17,color:theme.mutedForeground}}>131,072 B · 30 chunks</span></div>
  <div style={{marginTop:20,fontSize:21}}>55,909 {c.blocks}</div>
 </div><div style={{fontSize:17,color:theme.mutedForeground,lineHeight:1.6,marginTop:20}}>{locale==='zh-CN'?'实际Java 1.20.1存档文件；尚未进入游戏实测。':'Actual Java 1.20.1 world files; not yet loaded in-game.'}</div>
 {variant===3&&<div style={{marginTop:26}}><Button action active>{c.export}</Button></div>}
 </div></div>:
  <><div style={{border:`1px solid ${theme.border}`,borderRadius:7,background:theme.card,overflow:'hidden'}}><div style={{padding:'18px 24px',background:theme.secondary,fontSize:20}}>{c.files}</div>
   {['Nova-Observatory-materials.csv','Nova-Observatory-materials.html'].map((name,i)=><div key={name} style={{display:'flex',alignItems:'center',gap:20,padding:'17px 24px',borderTop:`1px solid ${theme.border}`,opacity:variant===0||variant===1?ramp(f,44+i*10,62+i*10):1,transform:`translateY(${(variant===0||variant===1?1-ramp(f,44+i*10,62+i*10):0)*22}px)`}}><Icon type={2} size={34}/><div><div style={{fontSize:25}}>{name}</div><div style={{fontSize:18,color:theme.mutedForeground,marginTop:8}}>{i?'247,230 bytes · HTML':'775 bytes · 15 rows · CSV'}</div></div><span style={{marginLeft:'auto',color:theme.green}}>{done?'✓':''}</span></div>)}</div>
   <div style={{marginTop:18,padding:18,background:'#10151a',borderRadius:7,fontFamily:'Consolas, monospace',fontSize:18,lineHeight:1.65,color:'#c5d6e4'}}>Name,Item ID,Quantity<br/>"Cut Copper","minecraft:cut_copper",84<br/>"Smooth Quartz Block","minecraft:smooth_quartz",20744</div>
   <div style={{position:'absolute',left:30,bottom:45,display:'flex',alignItems:'center',gap:18}}><Button action={variant===0||variant===1} active>{c.export}</Button>{done&&<Badge>{c.complete}</Badge>}</div></>}
 </>;
};

export const ComposePanel=({locale,variant,f}:{locale:Locale;variant:number;f:number})=>{
 const c=copy(locale),done=f>=42;return <><PageTitle subtitle={locale==='zh-CN'?'多投影持久实例 · 控件与状态演示':'Persistent schematic instances · controls and state demo'}>{c.compose}</PageTitle>
 <div style={{display:'flex',gap:26}}><div style={{width:320}}><Button action={variant===0} active>+ {c.instance}</Button><Rule/>{(variant===0&&!done?[0]:[0,1]).map(i=><div key={i} style={{padding:18,border:`1px solid ${i===0?theme.blue:theme.border}`,borderRadius:6,background:theme.card,marginBottom:14}}><div style={{fontSize:22}}>{c.gallery} · {i?'B':'A'}</div><div style={{fontSize:17,color:theme.mutedForeground,marginTop:10}}>☑　{locale==='zh-CN'?'显示　未锁定':'Visible · Unlocked'}</div></div>)}<Badge>{locale==='zh-CN'?'源文件不会随实例移除而删除':'Removing an instance keeps its source'}</Badge></div>
 <div style={{flex:1,padding:26,border:`1px solid ${theme.border}`,borderRadius:7,background:theme.card}}><h3 style={{fontSize:26,margin:'0 0 22px'}}>{variant===2?c.splitRange:c.instance+' A'}</h3>
 {variant===2?<><div style={{display:'grid',gridTemplateColumns:'1fr 1fr 1fr',gap:12,color:theme.mutedForeground,fontSize:20}}><span>X</span><span>Y</span><span>Z</span></div>{[0,1].map(i=><div key={i} style={{display:'grid',gridTemplateColumns:'1fr 1fr 1fr',gap:12,marginTop:17,fontSize:28}}><span>{i?'48–95':'0–47'}</span><span>0–99</span><span>0–71</span></div>)}<Rule/><Button active>{c.split}</Button></>:<><Label>XYZ</Label><div style={{display:'flex',gap:16}}>{[variant===1&&done?'32':'0','0','0'].map((v,i)=><div key={i} data-nova-action={variant===1&&i===0?'primary':undefined} style={{flex:1,padding:18,border:`1px solid ${i===0?theme.blue:theme.border}`,borderRadius:5,background:theme.background,fontSize:34}}>{v}</div>)}</div><Field label={locale==='zh-CN'?'旋转':'Rotation'} value={variant===1?'90°':'0°'}/><Field label={locale==='zh-CN'?'镜像':'Mirror'} value="X / Z"/><div style={{marginTop:25}}><Button>{c.conflict}</Button></div></>}
 <div style={{fontSize:18,color:theme.mutedForeground,lineHeight:1.6,marginTop:30}}>{locale==='zh-CN'?'本镜头展示真实发布控件与演示状态；不伪造多实例 GPU 结果。':'Published controls with demo state; this shot does not invent a composed GPU result.'}</div></div></div></>;
};

export const FormatsPanel=({locale,f}:{locale:Locale;f:number})=>{
 const c=copy(locale);return <><PageTitle subtitle={locale==='zh-CN'?'根据目标能力处理，保留明确的失败原因':'Capability-aware handling, with clear failure reasons'}>{c.formats}</PageTitle>
 <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:20,marginTop:32}}>{['Litematica','Minecraft Structure','Create','AnvilCraft','Building Gadgets'].map((n,i)=><div key={n} style={{display:'flex',alignItems:'center',gap:20,padding:30,border:`1px solid ${i===0?theme.blue:theme.border}`,borderRadius:7,background:theme.card,opacity:ramp(f,8+i*7,24+i*7),transform:`translateY(${(1-ramp(f,8+i*7,24+i*7))*40}px)`}}><Icon size={38}/><span style={{fontSize:30}}>{n}</span></div>)}</div>
 <div style={{fontSize:20,color:theme.mutedForeground,marginTop:25,lineHeight:1.6}}>{c.warning}</div></>;
};

export const SettingsPanel=({locale,variant,f}:{locale:Locale;variant:number;f:number})=>{
 const c=copy(locale),active=f>=42;return <><PageTitle>{c.nav[9]}</PageTitle><div style={{display:'flex',gap:30}}><div style={{flex:1}}><Label>{c.theme}</Label><div style={{display:'flex',flexWrap:'wrap',gap:12}}>{['shadcn/ui','8bitcn/ui','Minecraft','Metro10','Aero7'].map((v,i)=><Button active={i===0} key={v}>{v}</Button>)}</div><Rule/>
 <Label>{locale==='zh-CN'?'颜色模式':'Color mode'}</Label><div style={{display:'flex',gap:12}}><Button active={!active||variant!==0}>{locale==='zh-CN'?'深色':'Dark'}</Button><Button action={variant===0} active={active&&variant===0}>{locale==='zh-CN'?'浅色':'Light'}</Button></div><Rule/>
 <Label>{c.scale}</Label><div style={{display:'flex',gap:24,alignItems:'center'}}><div style={{fontSize:48,fontWeight:300}}>{variant===1&&active?'1.25×':'1.00×'}</div><Badge>0.50×–5.00×</Badge></div><div data-nova-action={variant===1?'primary':undefined} style={{marginTop:25,width:520,height:8,background:theme.border,borderRadius:6}}><div style={{height:'100%',width:variant===1&&active?'30%':'20%',background:theme.blue,borderRadius:6}}/></div></div>
 <div style={{width:380,padding:28,border:`1px solid ${theme.border}`,borderRadius:7,background:theme.card}}><Label>{locale==='zh-CN'?'正式安装包':'Official downloads'}</Label>{['Windows x64 · Setup','Linux x64 · Deb / AppImage','macOS · Intel / Apple Silicon'].map((v,i)=><div key={v} style={{padding:'22px 0',fontSize:23,borderBottom:`1px solid ${theme.border}`,color:variant===2&&f>40+i*10?theme.blue:theme.foreground}}>{v}</div>)}<div style={{fontSize:17,color:theme.mutedForeground,lineHeight:1.6,marginTop:22}}>{locale==='zh-CN'?'图形与安装仍有平台限制；macOS 未公证。详见发布说明。':'Platform graphics and install limits remain. macOS is not notarized. See release notes.'}</div></div></div></>;
};
