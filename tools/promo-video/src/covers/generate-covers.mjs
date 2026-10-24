import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {chromium} from 'playwright';

const workspace=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const output=path.join(workspace,'output'), assets=path.join(workspace,'assets/original');
const locales=['zh-CN','en-US'];
// Covers are a post-production artifact: both completed films are required.
const movies=locales.map(locale=>{
  const file=path.join(output,`Litematica-Nova-Official-Intro-${locale}.mp4`);
  if(!fs.existsSync(file))throw new Error(`Film is missing: ${locale}`);
  const probe=spawnSync('ffprobe',['-v','error','-show_entries','format=duration','-of','json',file],{encoding:'utf8'});
  if(probe.status!==0)throw new Error(`Cannot inspect completed film: ${locale}`);
  const seconds=Number(JSON.parse(probe.stdout).format.duration);
  if(!(seconds>=115&&seconds<=125))throw new Error(`Film duration does not permit covers yet: ${locale}`);
  return {locale,seconds,file:path.basename(file)};
});
const data=(name)=>`data:image/png;base64,${fs.readFileSync(path.join(assets,name)).toString('base64')}`;
const hero=data('hero-standard-contained.png'), icon=data('nova-product-icon.png');
const evidence=JSON.parse(fs.readFileSync(path.join(assets,'asset-manifest.json'),'utf8'));
if(evidence.productVersion!=='0.4.0'||evidence.finalVideoRenderer!=='Standard')throw new Error('Covers require the frozen Standard release image.');
const uiEvidence=JSON.parse(fs.readFileSync(path.join(assets,'product-ui-capture.json'),'utf8'));
if(uiEvidence.appVersion!=='0.4.0'||uiEvidence.kind!=='actual-published-React-UI-with-empty-demo-native-adapter')throw new Error('Product UI evidence does not match the released React demo.');
const copy={
  'zh-CN':{language:'zh-CN',eyebrow:'MINECRAFT 投影查看器',line1:'你的建筑，',line2:'一眼看清。',subtitle:'无需启动游戏，先看清每一块。',features:['分层检查','材料统计','备货进度'],workspace:'Nova 工作区',preview:'Standard · 真实预览'},
  'en-US':{language:'en-US',eyebrow:'MINECRAFT SCHEMATIC VIEWER',line1:'Your build.',line2:'In full view.',subtitle:'See every block. Before you build.',features:['Layers','Materials','Stockpile'],workspace:'Nova workspace',preview:'Actual Standard preview'},
};
const escape=(s)=>s.replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;').replaceAll('"','&quot;');
function makeSvg(locale){
  const c=copy[locale],ui=data(`product-ui-${locale}.png`);
  const metadata={schema:'nova.official.cover.v1',locale,productVersion:'0.4.0',releaseTag:'beta-0.4.0',
    filmsCompletedBeforeCoverGeneration:movies.map(({locale,seconds})=>({locale,seconds})),
    building:{source:'hero-standard-contained.png',kind:'actual-production-Standard-Unlit-GPU-CaptureFrame',wholeModel:true},
    interface:{source:`product-ui-${locale}.png`,kind:uiEvidence.kind,nativeNovaProcessLaunched:false,personalUserDataRead:false},
    graphicTreatment:'SVG typography, background, card frame and drop shadows; embedded source pixels are not repainted.',selfContained:true};
  let chipX=112;
  const chips=c.features.map((label,i)=>{
    const width=locale==='zh-CN'?132:[108,138,138][i];
    const item=`<g transform="translate(${chipX} 566)"><rect width="${width}" height="42" rx="7" fill="#20262e" stroke="#37414e"/><text x="${width/2}" y="27" text-anchor="middle" class="feature">${escape(label)}</text></g>`;
    chipX+=width+12;return item;
  }).join('');
  return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="1920" height="1080" viewBox="0 0 1920 1080" role="img" aria-labelledby="title description" xml:lang="${locale}">
  <title id="title">${escape(c.line1+' '+c.line2)} — Litematica-Nova 0.4</title>
  <desc id="description">${locale==='zh-CN'?'建筑来自0.4正式版Standard真实GPU捕获；界面为正式发布React前端在空演示原生适配器下的真实画面，并非Native程序运行截图。':'Building: actual Standard GPU capture from release 0.4. UI: the released React frontend with an empty demo native adapter, not a running Native application screenshot.'}</desc>
  <metadata><![CDATA[${JSON.stringify(metadata)}]]></metadata>
  <defs>
    <linearGradient id="background" x2="1" y2="1"><stop stop-color="#161a20"/><stop offset="1" stop-color="#0e1217"/></linearGradient>
    <radialGradient id="stage"><stop stop-color="#2c3a48" stop-opacity=".63"/><stop offset="1" stop-color="#18212a" stop-opacity="0"/></radialGradient>
    <linearGradient id="fade" x2="0" y2="1"><stop stop-color="#a4bdd4" stop-opacity=".1"/><stop offset="1" stop-color="#a4bdd4" stop-opacity="0"/></linearGradient>
    <pattern id="grid" width="80" height="80" patternUnits="userSpaceOnUse"><path d="M 80 0 L 0 0 0 80" fill="none" stroke="#b4c5d6" stroke-opacity=".04" stroke-width="1"/></pattern>
    <filter id="model-shadow" x="-25%" y="-20%" width="150%" height="160%"><feDropShadow dx="0" dy="26" stdDeviation="28" flood-color="#000" flood-opacity=".42"/></filter>
    <filter id="window-shadow" x="-25%" y="-25%" width="150%" height="160%"><feDropShadow dx="0" dy="18" stdDeviation="24" flood-color="#000" flood-opacity=".58"/></filter>
    <clipPath id="ui-clip"><rect x="113" y="671" width="598" height="373" rx="0"/></clipPath>
    <style>
      text{font-family:'Segoe UI','Microsoft YaHei UI','Microsoft YaHei',sans-serif;fill:#f4f4f5}
      .brand{font-size:33px;font-weight:650;letter-spacing:-.8px}
      .eyebrow{font-size:18px;letter-spacing:2.4px;fill:#91b6dc;font-weight:650}
      .headline{font-size:${locale==='zh-CN'?108:102}px;font-weight:750;letter-spacing:${locale==='zh-CN'?'-3':'-4'}px}
      .subtitle{font-size:25px;fill:#b1bbc7}
      .feature{font-size:19px;fill:#c3cdd8;font-weight:550}
      .window-title{font-size:15px;fill:#adb7c4}
    </style>
  </defs>
  <rect width="1920" height="1080" fill="url(#background)"/>
  <rect x="768" width="1152" height="1080" fill="url(#grid)"/>
  <ellipse cx="1350" cy="600" rx="660" ry="535" fill="url(#stage)"/>
  <path d="M780 894 L1270 620 L1850 951" fill="none" stroke="#8ca7bf" stroke-opacity=".12"/>
  <path d="M794 932 L1285 658 L1866 989" fill="none" stroke="#8ca7bf" stroke-opacity=".06"/>
  <image x="112" y="88" width="61" height="61" href="${icon}"/>
  <text x="194" y="131" class="brand">Litematica-Nova</text>
  <rect x="1710" y="96" width="104" height="47" rx="23.5" fill="#243344" stroke="#49627a"/>
  <text x="1762" y="127" font-size="24" font-weight="650" text-anchor="middle" fill="#d8e6f4">0.4</text>
  <text x="112" y="241" class="eyebrow">${escape(c.eyebrow)}</text>
  <text x="108" y="367" class="headline" data-left-text="headline-one">${escape(c.line1)}</text>
  <text x="108" y="481" class="headline" data-left-text="headline-two">${escape(c.line2)}</text>
  <text x="112" y="534" class="subtitle" data-left-text="subtitle">${escape(c.subtitle)}</text>
  ${chips}
  <image x="782" y="114" width="1080" height="944.36" preserveAspectRatio="xMidYMid meet" href="${hero}" filter="url(#model-shadow)"/>
  <g transform="translate(112 640)" filter="url(#window-shadow)">
    <rect width="600" height="405" rx="10" fill="#1d2229" stroke="#556171" stroke-width="1.2"/>
    <path d="M10 0H590Q600 0 600 10V31H0V10Q0 0 10 0Z" fill="#2a3039"/>
    <image x="13" y="8" width="15" height="15" href="${icon}"/>
    <text x="38" y="21" class="window-title">${escape(c.workspace)}</text>
    <path d="M531 15H542 M559 10H570V21H559Z M583 10L592 20 M592 10L583 20" fill="none" stroke="#8793a1" stroke-width="1"/>
  </g>
  <image x="113" y="671" width="598" height="373" preserveAspectRatio="xMidYMid meet" href="${ui}" clip-path="url(#ui-clip)"/>
  <g transform="translate(1118 74)"><rect width="285" height="42" rx="21" fill="#1c2631" fill-opacity=".9" stroke="#455668"/><circle cx="1142" cy="95" r="0"/><circle cx="23" cy="21" r="4" fill="#a9cdb9"/><text x="42" y="27" font-size="18" fill="#bed0df">${escape(c.preview)}</text></g>
</svg>`;
}

const browser=await chromium.launch({channel:'msedge',headless:true});
const results=[];
try{
  for(const locale of locales){
    const svg=makeSvg(locale), stem=`Litematica-Nova-Official-Cover-${locale}`;
    if((svg.match(/href="data:image\/png;base64,/g)||[]).length<3)throw new Error('Cover must embed the actual model, UI and icon.');
    if(/(?:href|src)="(?!data:|#)/.test(svg))throw new Error('Cover contains an external resource reference.');
    const svgPath=path.join(output,stem+'.svg');fs.writeFileSync(svgPath,svg);
    const page=await browser.newPage({viewport:{width:1920,height:1080},deviceScaleFactor:1,locale});
    await page.route('**/*',route=>route.request().url().startsWith('data:')?route.continue():route.abort());
    await page.setContent(`<html><head><meta charset="utf-8"><style>html,body{margin:0;width:1920px;height:1080px;overflow:hidden}svg{display:block}</style></head><body>${svg}</body></html>`);
    await page.evaluate(()=>document.fonts.ready);
    const boxes=await page.locator('[data-left-text]').evaluateAll(nodes=>nodes.map(node=>{const b=node.getBBox();return {id:node.getAttribute('data-left-text'),x:b.x,y:b.y,width:b.width,height:b.height};}));
    if(boxes.some(b=>b.x<96||b.x+b.width>760||b.y<200||b.y+b.height>550))throw new Error(`Cover typography exceeds its column: ${locale}`);
    await page.screenshot({path:path.join(output,stem+'.png'),type:'png'});
    await page.setViewportSize({width:320,height:180});
    await page.addStyleTag({content:'html,body{width:320px!important;height:180px!important}svg{width:320px!important;height:180px!important}'});
    const qa=path.join(output,'qa');fs.mkdirSync(qa,{recursive:true});
    await page.screenshot({path:path.join(qa,`cover-${locale}-320.png`),type:'png'});
    results.push({locale,svg:stem+'.svg',png:stem+'.png',size:[1920,1080],embeddedImages:4,layoutBoxes:boxes});
    await page.close();
  }
}finally{await browser.close();}
fs.writeFileSync(path.join(output,'cover-manifest.json'),JSON.stringify({schema:'nova.official.covers.v1',productVersion:'0.4.0',completedFilms:movies,covers:results,sourceAssets:{building:'actual Standard GPU capture',interface:uiEvidence.kind},selfContained:true},null,2));
console.log(JSON.stringify({covers:results.map(({locale,svg,png})=>({locale,svg,png})),size:[1920,1080],mode:'Standard'}));
