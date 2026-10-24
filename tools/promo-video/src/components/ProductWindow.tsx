import React, {type CSSProperties, type ReactNode} from 'react';
import {Img, staticFile} from 'remotion';
import {copy, type Locale} from '../locales';
import {theme} from '../theme';
import {assets} from '../assets';

export const Button = ({children,active=false,style={},action=false}:{children:ReactNode;active?:boolean;style?:CSSProperties;action?:boolean}) =>
 <div data-nova-action={action?'primary':undefined} style={{display:'inline-flex',alignItems:'center',justifyContent:'center',gap:10,padding:'11px 18px',
 borderRadius:6,background:active?theme.foreground:theme.secondary,color:active?'var(--film-primary-foreground,#18181b)':theme.foreground,
 border:`1px solid ${active?'transparent':theme.border}`,fontSize:21,whiteSpace:'nowrap',...style}}>{children}</div>;
export const Badge = ({children}:{children:ReactNode}) => <span style={{display:'inline-block',border:`1px solid ${theme.border}`,
 borderRadius:5,padding:'5px 11px',fontSize:17,color:theme.mutedForeground}}>{children}</span>;
export const Label = ({children}:{children:ReactNode}) => <div style={{fontSize:19,color:theme.mutedForeground,marginBottom:12}}>{children}</div>;
export const Icon = ({type=0,size=22}:{type?:number;size?:number}) => <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
 {type===0?<><path d="m12 3 9 5v9l-9 5-9-5V8z"/><path d="m3 8 9 5 9-5M12 13v9"/></>:
 type===1?<><path d="M3 5h6l2 3h10v12H3z"/></>:
 type===2?<><path d="M4 4h16v16H4zM8 8h8M8 12h8M8 16h5"/></>:
 type===3?<><path d="M4 19V9M10 19V4M16 19v-7M22 19H2"/></>:
 type===4?<><path d="m3 8 9-5 9 5-9 5zM3 12l9 5 9-5M3 16l9 5 9-5"/></>:
 type===5?<><path d="m4 12 5 5 11-11"/></>:
 type===6?<><path d="M4 6h15l-4-4M20 18H5l4 4M19 6v6M5 18v-6"/></>:
 type===8?<><path d="M12 3v12m-5-5 5 5 5-5M3 16v5h18v-5"/></>:
 <><path d="M12 3v3m0 12v3M3 12h3m12 0h3m-5-8-2 2M6 18l2-2M3 4l3 3m12 11 3 3"/><circle cx="12" cy="12" r="5"/></>}
 </svg>;

export const ProductWindow = ({locale,route=1,children,light=false,scale=1}:{locale:Locale;route?:number;children:ReactNode;light?:boolean;scale?:number}) => {
 const c=copy(locale);return <div style={{...({
  '--film-background':light?'#fafafa':'#141416','--film-foreground':light?'#18181b':'#f4f4f5',
  '--film-card':light?'#ffffff':'#1c1c1f','--film-popover':light?'#ffffff':'#242427',
  '--film-secondary':light?'#e4e4e7':'#303034','--film-muted':light?'#f4f4f5':'#252528',
  '--film-muted-foreground':light?'#63636c':'#b1b1bb','--film-accent':light?'#eeeeef':'#34343a',
  '--film-border':light?'#dcdce0':'#3b3b42','--film-primary-foreground':light?'#ffffff':'#18181b',
 } as CSSProperties),width:1584,height:784,background:light?'#fafafa':theme.background,
 color:light?'#18181b':theme.foreground,border:`1px solid ${light?'#dcdce0':theme.border}`,borderRadius:10,overflow:'hidden',
 boxShadow:'0 40px 100px #0007',fontFamily:theme.font,position:'relative'}}>
  <div style={{height:42,display:'flex',alignItems:'center',padding:'0 18px',gap:10,background:light?'#f0f0f2':'#202023',fontSize:17}}>
    <Img src={staticFile(assets.logo)} style={{width:23,height:23}}/>Litematica-Nova
    <div style={{marginLeft:'auto',display:'flex',gap:34,color:light?'#666':'#aaa'}}>─　□　×</div>
  </div>
  <div style={{display:'flex',height:742}}>
   <aside style={{width:220,flexShrink:0,borderRight:`1px solid ${light?'#ddd':theme.border}`,padding:'24px 10px',display:'flex',flexDirection:'column',gap:6,
    background:light?'#f4f4f5':'#1c1c1f'}}>
    <div style={{fontWeight:700,fontSize:22,padding:'6px 12px 24px'}}>Litematica-Nova</div>
    {c.nav.map((n,i)=><div key={n} style={{height:48,display:'flex',alignItems:'center',gap:12,padding:'0 14px',fontSize:18,
     borderRadius:6,background:i===route?(light?'#e4e4e7':theme.accent):'transparent',fontWeight:i===route?600:400}}><Icon type={i}/>{n}</div>)}
    <div style={{marginTop:'auto',fontSize:16,color:theme.mutedForeground,padding:12}}>Beta 0.4.0</div>
   </aside>
   <main style={{padding:'24px 30px',flex:1,minWidth:0,position:'relative',overflow:'hidden',fontSize:22,zoom:scale}}>{children}</main>
  </div>
  <div style={{position:'absolute',bottom:9,right:16,fontSize:14,color:light?'#666':theme.mutedForeground,
   background:light?'#fafafacc':'#141416cc',padding:'4px 8px',borderRadius:4}}>{c.demo}</div>
 </div>;
};
export const PageTitle = ({children,subtitle}:{children:ReactNode;subtitle?:string}) =>
 <div style={{marginBottom:26}}><div style={{fontSize:29,fontWeight:650,lineHeight:1.25}}>{children}</div>{subtitle&&<div style={{fontSize:18,color:theme.mutedForeground,marginTop:9}}>{subtitle}</div>}</div>;
export const Rule = () => <div style={{height:1,background:theme.border,margin:'22px 0'}}/>;
