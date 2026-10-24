import React from 'react';
import {AbsoluteFill,Audio,Img,Sequence,staticFile,useCurrentFrame} from 'remotion';
import {theme,release} from '../theme';
import {copy,type Locale} from '../locales';
import {assets} from '../assets';
import {shots,type Shot,FPS} from '../timeline/shots';
import {ramp,mix} from '../motion/motion';
import {ProductWindow} from '../components/ProductWindow';
import {Pointer} from '../components/Pointer';
import {LibraryPanel,ViewerPanel,LayerPanel,StatsPanel,StockpilePanel,PropertiesPanel,ReplacePanel,ExportPanel,ComposePanel,FormatsPanel,SettingsPanel} from './Panels';

const Grid=()=> <AbsoluteFill style={{backgroundImage:'linear-gradient(#91b6dc0c 1px,transparent 1px),linear-gradient(90deg,#91b6dc0c 1px,transparent 1px)',backgroundSize:'72px 72px',maskImage:'radial-gradient(ellipse at 65% 45%,black,transparent 80%)',pointerEvents:'none'}}/>;
const SmallBrand=()=> <div style={{display:'flex',alignItems:'center',gap:15,fontSize:23,fontWeight:600,letterSpacing:1}}><Img src={staticFile(assets.logo)} style={{width:38,height:38}}/>Litematica-Nova <span style={{fontWeight:400,fontSize:18,color:theme.blue,border:`1px solid ${theme.blueDark}`,borderRadius:5,padding:'5px 8px'}}>0.4</span></div>;

const Hero=({locale,variant,f}:{locale:Locale;variant:number;f:number})=>{
 const c=copy(locale),title=locale==='zh-CN'?['让下一座建筑，','先被看见。']:['Your next build.','Seen clearly.'];
 const reveal=ramp(f,2,55);
 if(variant===2)return <div style={{position:'absolute',left:168,top:156,transform:`translateY(${(1-reveal)*55}px) rotateY(${mix(-5,0,reveal)}deg)`,opacity:reveal}}><ProductWindow locale={locale} route={3}><ViewerPanel locale={locale} variant={0} f={80}/></ProductWindow></div>;
 if(variant===3)return <><div style={{position:'absolute',left:144,top:95}}><SmallBrand/></div><Img src={staticFile(assets.hero)} style={{position:'absolute',left:770,top:140,width:1070,height:800,objectFit:'contain'}}/><div style={{position:'absolute',left:150,top:335}}><div style={{fontSize:77,fontWeight:650,letterSpacing:-2,lineHeight:1.18}}>Litematica<br/>Nova</div><div style={{marginTop:26,fontSize:32,color:theme.blue}}>Beta 0.4</div><div style={{marginTop:30,color:theme.mutedForeground,fontSize:25}}>{c.noGame}</div></div></>;
 return <><div style={{position:'absolute',left:144,top:95}}><SmallBrand/></div>
  <div style={{position:'absolute',left:710,top:130,width:1140,height:830,transform:`translateY(${mix(35,0,reveal)}px) rotate(${mix(-1.5,0,reveal)}deg)`,opacity:reveal}}>
   {variant===1&&<svg style={{position:'absolute',inset:0,width:'100%',height:'100%',opacity:.55}} viewBox="0 0 1140 830"><path d="M90 590 570 780 1080 520 600 340Z M90 590V200L570 10 1080 230V520 M570 780V390" stroke={theme.blue} strokeWidth="1" fill="none" strokeDasharray="9 8"/><path d="M90 590h990M570 10v770" stroke={theme.blueDark} strokeWidth=".6"/></svg>}
   <Img src={staticFile(assets.hero)} style={{position:'absolute',inset:0,width:'100%',height:'100%',objectFit:'contain',clipPath:variant===1?`inset(0 ${mix(85,0,reveal)}% 0 0)`:'none'}}/>
   <span style={{position:'absolute',bottom:25,right:18,fontSize:17,color:theme.mutedForeground}}>{c.real}</span>
  </div>
  <div style={{position:'absolute',left:145,top:355,width:720,transform:`translateY(${mix(24,0,ramp(f,10,38))}px)`,opacity:ramp(f,6,30)}}>
   <div style={{fontSize:locale==='zh-CN'?76:77,fontWeight:600,lineHeight:1.2,letterSpacing:-2.7}}>{title[0]}<br/><span style={{color:theme.blue}}>{title[1]}</span></div>
   <div style={{marginTop:32,fontSize:27,color:theme.mutedForeground,lineHeight:1.5}}>{variant===1?(locale==='zh-CN'?'蓝图标注 → 实际 GPU 画面':'Blueprint annotation → actual GPU frame'):c.noGame}</div>
  </div>
 </>;
};

const Outro=({locale,f}:{locale:Locale;f:number})=>{
 const c=copy(locale);return <><Grid/><div style={{position:'absolute',left:145,top:92}}><SmallBrand/></div>
 <div style={{position:'absolute',left:120,top:225,width:1000,height:650,opacity:.8,transform:`translateX(${mix(-45,0,ramp(f,0,38))}px)`}}><Img src={staticFile(assets.hero)} style={{width:'100%',height:'100%',objectFit:'contain'}}/></div>
 <div style={{position:'absolute',left:1045,top:285,width:760,opacity:ramp(f,5,42),transform:`translateY(${mix(30,0,ramp(f,5,42))}px)`}}>
  <div style={{fontSize:locale==='zh-CN'?61:59,fontWeight:600,letterSpacing:-1.9,lineHeight:1.25}}>{locale==='zh-CN'?<>从蓝图，<br/>走向下一座建筑。</>:<>From a blueprint<br/>to your next build.</>}</div>
  <div style={{marginTop:33,fontSize:27,color:theme.blue}}>{c.download}</div>
  <div style={{marginTop:35,padding:'18px 20px',border:`1px solid ${theme.border}`,borderRadius:7,fontSize:21,background:'#1c2027'}}>github.com/LitematicaViewer/<br/><strong style={{fontSize:26}}>Litematica-Nova</strong>　→ Releases</div>
 </div><div style={{position:'absolute',bottom:80,left:145,fontSize:21,color:theme.mutedForeground}}>{c.platforms}</div>
 <div style={{position:'absolute',bottom:39,left:145,right:145,fontSize:16,color:'#8b929c'}}>{c.warning}</div></>;
};

const ShotView=({shot,locale,f}:{shot:Shot;locale:Locale;f:number})=>{
 const c=copy(locale),v=shot.variant;
 if(shot.kind==='hero')return <Hero locale={locale} variant={v} f={f}/>;
 if(shot.kind==='outro')return <Outro locale={locale} f={f}/>;
 const route={library:1,viewer:3,layer:4,stats:5,stockpile:5,properties:2,replace:6,export:5,compose:7,formats:1,settings:9}[shot.kind];
 const light=shot.kind==='settings'&&v===0&&f>=42;
 const zoom=1; // S42 shows the real scale setting, not a fictitious applied window scale.
 let panel:React.ReactNode;
 switch(shot.kind){
  case 'library':panel=<LibraryPanel locale={locale} variant={v} f={f}/>;break;
  case 'viewer':panel=<ViewerPanel locale={locale} variant={v} f={f}/>;break;
  case 'layer':panel=<LayerPanel locale={locale} variant={v} f={f}/>;break;
  case 'stats':panel=<StatsPanel locale={locale} variant={v} f={f}/>;break;
  case 'stockpile':panel=<StockpilePanel locale={locale} variant={v} f={f}/>;break;
  case 'properties':panel=<PropertiesPanel locale={locale} variant={v}/>;break;
  case 'replace':panel=<ReplacePanel locale={locale} variant={v} f={f}/>;break;
  case 'export':panel=<ExportPanel locale={locale} variant={v} f={f}/>;break;
  case 'compose':panel=<ComposePanel locale={locale} variant={v} f={f}/>;break;
  case 'formats':panel=<FormatsPanel locale={locale} f={f}/>;break;
  default:panel=<SettingsPanel locale={locale} variant={v} f={f}/>;
 }
 const glide=shot.kind==='library'&&v===0?mix(18,0,ramp(f,0,55)):0;
 return <>
  <div style={{position:'absolute',left:174,top:38,right:174,display:'flex',justifyContent:'space-between',alignItems:'center'}}><div style={{fontSize:20,color:theme.blue,letterSpacing:1.8}}>{shot.act[0]} / {locale==='zh-CN'?(shot.act[0]==='A'?'整理':shot.act[0]==='B'?'施工':'交付'):shot.act.slice(4).toUpperCase()}</div><div style={{fontSize:18,color:theme.mutedForeground}}>Litematica-Nova · 0.4</div></div>
  <div style={{position:'absolute',left:174,top:82,fontSize:locale==='zh-CN'?47:43,fontWeight:600,letterSpacing:-1,opacity:ramp(f,0,16),transform:`translateY(${mix(10,0,ramp(f,0,22))}px)`}}>{locale==='zh-CN'?shot.onScreenTextZh:shot.onScreenTextEn}</div>
  <div style={{position:'absolute',left:168,top:174,transform:`translateY(${glide}px)`,opacity:.65+.35*ramp(f,0,18)}}><ProductWindow locale={locale} route={route} light={light} scale={zoom}>{panel}</ProductWindow></div>
  <Pointer frame={f}/>
  <div style={{position:'absolute',left:174,top:1000,color:theme.mutedForeground,fontSize:18,display:'flex',justifyContent:'space-between',width:1572}}><span>{c.platforms}</span><span>{shot.id} · Beta {release.version}</span></div>
 </>;
};

const hasClick=(s:Shot)=>s.kind==='library'||s.kind==='layer'||s.kind==='viewer'&&s.variant>0||
 s.kind==='stats'&&s.variant>=2||s.kind==='stockpile'&&[0,1,3].includes(s.variant)||
 s.kind==='replace'&&s.variant>0||s.kind==='export'&&s.variant<2||s.kind==='compose'&&s.variant<2||s.kind==='settings'&&s.variant<2;
const Soundtrack=()=> <>
 <Audio src={staticFile('audio/nova-original-score-master.wav')} volume={0.86}/>
 {shots.map(s=><React.Fragment key={s.id}>
  {(hasClick(s)||s.kind==='hero')&&<Sequence from={Math.round((s.inSeconds+.65)*FPS)} durationInFrames={12}><Audio src={staticFile(s.kind==='hero'?'audio/soft-impact.wav':'audio/click.wav')} volume={s.kind==='hero'?.36:.7}/></Sequence>}
  {(hasClick(s)||s.kind==='stockpile'&&s.variant===4)&&<Sequence from={Math.round((s.inSeconds+1.25)*FPS)} durationInFrames={45}><Audio src={staticFile('audio/confirm.wav')} volume={.23}/></Sequence>}
  {s.kind!=='outro'&&<Sequence from={Math.round((s.outSeconds-.4)*FPS)} durationInFrames={24}><Audio src={staticFile('audio/swish.wav')} volume={.18}/></Sequence>}
 </React.Fragment>)}
 </>;

export const Film=({locale}:{locale:Locale})=> <AbsoluteFill data-film-root style={{background:theme.stage,color:theme.foreground,fontFamily:theme.font,overflow:'hidden'}}>
 <Grid/>{shots.map(shot=><Sequence key={shot.id} from={shot.inSeconds*FPS} durationInFrames={(shot.outSeconds-shot.inSeconds)*FPS}><TimedShot shot={shot} locale={locale}/></Sequence>)}
 <Soundtrack/>
 </AbsoluteFill>;
const TimedShot=({shot,locale}:{shot:Shot;locale:Locale})=>{const f=useCurrentFrame();return <ShotView shot={shot} locale={locale} f={f}/>};
export const StyleFrame=({kind}:{kind:'hero'|'feature'|'outro'})=> <AbsoluteFill data-film-root style={{background:theme.stage,color:theme.foreground,fontFamily:theme.font}}><Grid/><ShotView locale="zh-CN" shot={kind==='hero'?shots[0]:kind==='feature'?shots.find(s=>s.kind==='stats'&&s.variant===1)!:shots[shots.length-1]} f={85}/></AbsoluteFill>;
