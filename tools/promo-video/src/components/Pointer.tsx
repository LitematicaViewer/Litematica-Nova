import {ramp, mix, clamp} from '../motion/motion';
import {interpolate} from 'remotion';
import {theme} from '../theme';
import {useLayoutEffect,useState} from 'react';

export const Pointer = ({frame,target=[1135,400],click=40}:{frame:number;target?:[number,number];click?:number}) => {
 const [anchor,setAnchor]=useState<[number,number]|null>(null);
 useLayoutEffect(()=>{
  const element=document.querySelector('[data-nova-action="primary"]');
  if(!element){setAnchor(null);return}
  const box=element.getBoundingClientRect();
  const stage=document.querySelector('[data-film-root]')?.getBoundingClientRect();
  const sx=stage&&stage.width>0?1920/stage.width:1,sy=stage&&stage.height>0?1080/stage.height:1;
  const next:[number,number]=[(box.left+box.width/2-(stage?.left||0))*sx,(box.top+box.height/2-(stage?.top||0))*sy];
  setAnchor(previous=>previous&&Math.abs(previous[0]-next[0])<.01&&Math.abs(previous[1]-next[1])<.01?previous:next);
 },[frame]);
 if(!anchor)return null;
 target=anchor;
 const p=ramp(frame,7,click-6),x=mix(1610,target[0],p),y=mix(790,target[1],p)-Math.sin(p*Math.PI)*38;
 const r=interpolate(frame,[click,click+28],[12,65],clamp);
 const alpha=interpolate(frame,[click,click+8,click+28],[.65,.45,0],clamp);
 const press=interpolate(frame,[click-3,click+2,click+10],[1,.86,1],clamp);
 return <div style={{position:'absolute',inset:0,pointerEvents:'none',opacity:ramp(frame,0,8)}}>
  {frame>=click&&<div style={{position:'absolute',left:target[0]-r,top:target[1]-r,width:r*2,height:r*2,
   border:`2px solid ${theme.blue}`,borderRadius:'50%',opacity:alpha}}/>}
  <svg style={{position:'absolute',left:x,top:y,filter:'drop-shadow(0 4px 5px #0008)',transform:`scale(${press})`,transformOrigin:'2px 2px'}} width="42" height="48" viewBox="0 0 42 48">
   <path d="M4 3v34l9-8 8 15 8-4-9-14 13-3Z" fill="#fafafa" stroke="#111318" strokeWidth="2"/>
  </svg>
 </div>;
};
