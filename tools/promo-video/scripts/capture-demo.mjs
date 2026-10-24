import fs from 'node:fs';
import path from 'node:path';
const out=path.resolve('tools/promo-video/assets/original');fs.mkdirSync(out,{recursive:true});
const operations=[];
const block=(name,properties)=>({name:'minecraft:'+name,...((properties??{grass_block:{snowy:'false'},oak_leaves:{distance:'7',persistent:'true',waterlogged:'false'}}[name])?{properties:properties??{grass_block:{snowy:'false'},oak_leaves:{distance:'7',persistent:'true',waterlogged:'false'}}[name]}:{} )});
const box=(from,to,name,properties)=>operations.push({type:'fill_box',from,to,block:block(name,properties)});
const hollow=(from,to,name)=>operations.push({type:'hollow_box',from,to,thickness:1,block:block(name)});
const cylinder=(center,radius,height,name,filled=false)=>operations.push({type:'cylinder',center,radius,height,filled,block:block(name)});
const sphere=(center,radius,name,filled=true)=>operations.push({type:'sphere',center,radius,filled,block:block(name)});
// Original architectural demonstration. Only Nova's published shape generator
// performs voxel generation; no separate scene parser or renderer is used.
box([1,0,1],[94,1,70],'polished_deepslate');box([3,2,3],[92,3,68],'smooth_quartz');box([5,4,5],[90,4,66],'grass_block');
box([40,5,2],[55,5,23],'smooth_quartz');box([6,5,31],[89,5,40],'smooth_quartz');box([38,5,8],[57,5,63],'smooth_quartz');
for(const [x,z] of [[14,14],[81,14],[14,57],[81,57]]){
  cylinder([x,5,z],6,25,'quartz_bricks',false);
  for(const y of [5,16,28])cylinder([x,y,z],7,1,'polished_blackstone_bricks',false);
  sphere([x,31,z],7,'oxidized_copper');box([x-7,24,z-7],[x+7,30,z+7],'air');cylinder([x,28,z],7,2,'smooth_quartz',false);
  for(const dx of [-2,2]){box([x+dx,9,z-6],[x+dx,13,z-6],'cyan_stained_glass');box([x+dx,19,z-6],[x+dx,23,z-6],'cyan_stained_glass');}
  box([x,38,z],[x,40,z],'gold_block');
}
hollow([21,5,19],[74,31,52],'quartz_bricks');box([22,6,20],[73,30,51],'air');
for(const y of [5,16,27,31])box([20,y,18],[75,y,53],'smooth_quartz');
for(const x of [23,31,39,47,55,63,71]){
  box([x,6,18],[x+1,30,19],'quartz_pillar',{axis:'y'});box([x,6,52],[x+1,30,53],'quartz_pillar',{axis:'y'});
}
for(const x of [26,34,42,50,58,66])for(const y of [8,19]){box([x,y,19],[x+3,y+5,19],'light_blue_stained_glass');box([x,y,52],[x+3,y+5,52],'light_blue_stained_glass');}
for(const z of [23,31,39,47]){box([21,8,z],[21,13,z+2],'cyan_stained_glass');box([74,8,z],[74,13,z+2],'cyan_stained_glass');box([21,19,z],[21,24,z+2],'cyan_stained_glass');box([74,19,z],[74,24,z+2],'cyan_stained_glass');}
operations.push({type:'roof_gable',from:[20,32,18],to:[75,47,53],axis:'z',overhang:1,block:block('oxidized_copper')});
box([46,6,18],[49,12,19],'air');
for(const x of [41,53])box([x,6,13],[x+1,18,14],'quartz_pillar',{axis:'y'});
box([40,19,12],[55,19,19],'smooth_quartz');box([41,20,13],[54,20,18],'cut_copper');
cylinder([48,33,36],10,6,'quartz_bricks',false);sphere([48,40,36],11,'oxidized_copper');box([37,29,25],[59,39,47],'air');cylinder([48,38,36],11,2,'smooth_quartz',false);box([48,51,36],[48,54,36],'gold_block');
// Forecourt reflecting pools and gardens.
for(const [x,z]of [[25,10],[63,10],[25,60],[63,60]]){box([x-6,5,z-3],[x+6,5,z+3],'smooth_quartz');box([x-5,5,z-2],[x+5,5,z+2],'water',{level:'0'});}
for(const [x,z]of [[7,24],[7,47],[88,24],[88,47],[33,7],[62,7],[33,64],[62,64]]){box([x,5,z],[x,10,z],'oak_log',{axis:'y'});sphere([x,12,z],3,'oak_leaves');}
for(const x of [9,34,61,86])for(const z of [8,63]){box([x,5,z],[x,7,z],'polished_blackstone_bricks');box([x,8,z],[x,8,z],'sea_lantern');}
// Symmetric vertical air margin is intentional demo framing, not image crop.
for(const operation of operations)for(const key of ['from','to','base','center'])if(operation[key])operation[key][1]+=20;
const plan={version:1,minecraft_data_version:3465,metadata:{name:'Nova Azure Observatory',author:'Nova demonstration',description:'Original architectural demonstration created for Nova 0.4.0; CC0-1.0.'},regions:[{name:'Observatory',origin:[0,0,0],size:[96,100,72],operations}]};
fs.writeFileSync(path.join(out,'nova-observatory.plan.json'),JSON.stringify(plan,null,2));
fs.copyFileSync('desktop-nova/src-tauri/icons/icon.png',path.join(out,'nova-product-icon.png'));
const rotated=structuredClone(plan);rotated.metadata.name+=' - Quarter turn';rotated.regions[0].size=[72,100,96];
for(const operation of rotated.regions[0].operations){for(const key of ['from','to','base','center'])if(operation[key]){const[x,y,z]=operation[key];operation[key]=[z,y,95-x];}if(operation.axis)operation.axis=operation.axis==='x'?'z':operation.axis==='z'?'x':operation.axis;}
fs.writeFileSync(path.join(out,'nova-observatory-quarter-turn.plan.json'),JSON.stringify(rotated,null,2));
console.log(JSON.stringify({operations:operations.length,size:[96,100,72],plan:'nova-observatory.plan.json'}));
