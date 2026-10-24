import fs from 'node:fs';
import path from 'node:path';
import ts from 'typescript';
const root=path.resolve(import.meta.dirname,'..');
const source=fs.readFileSync(path.join(root,'src/timeline/shots.ts'),'utf8');
const js=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ES2022,target:ts.ScriptTarget.ES2022}}).outputText;
const {shots}=await import('data:text/javascript;base64,'+Buffer.from(js).toString('base64'));
fs.mkdirSync(path.join(root,'output'),{recursive:true});
const time=(s)=>{const ms=Math.round(s*1000);return `${String(Math.floor(ms/3600000)).padStart(2,'0')}:${String(Math.floor(ms/60000)%60).padStart(2,'0')}:${String(Math.floor(ms/1000)%60).padStart(2,'0')},${String(ms%1000).padStart(3,'0')}`};
for(const [locale,key] of [['zh-CN','onScreenTextZh'],['en-US','onScreenTextEn']]){
 const srt=shots.map((s,i)=>`${i+1}\n${time(s.inSeconds)} --> ${time(s.outSeconds)}\n${s[key]}\n`).join('\n');
 fs.writeFileSync(path.join(root,`output/Litematica-Nova-Official-Intro-${locale}.srt`),srt);
}
fs.writeFileSync(path.join(root,'shot-plan.json'),JSON.stringify(shots,null,2)+'\n');
fs.copyFileSync(path.join(root,'shot-plan.json'),path.join(root,'output/shot-plan.json'));
fs.writeFileSync(path.join(root,'recording-script.md'),'# 双语字幕 / 可选录音稿\n\n本次成片采用原创配乐与 SFX，无配音；以下短句对应实际字幕，不宣称存在语音轨。\n\n'+shots.map(s=>`## ${s.id} · ${s.inSeconds}–${s.outSeconds}s\n\n${s.onScreenTextZh}\n\n${s.onScreenTextEn}`).join('\n\n'));
console.log(`${shots.length} shots; ${shots.at(-1).outSeconds}s; bilingual SRT written`);
