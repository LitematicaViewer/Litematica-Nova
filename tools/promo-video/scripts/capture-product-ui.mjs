import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {chromium} from 'playwright';
import assert from 'node:assert/strict';

// Fixed, isolated media capture of the actual released React UI, not a Nova
// process or the user's WebView. Native calls have explicit empty-demo replies.
const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repo = path.resolve(workspace, '../..');
const frontend = path.join(repo, 'desktop-nova');
const stockpilePicker = process.argv.includes('--stockpile-picker');
const output = path.join(workspace, stockpilePicker ? '.cache/product-ui-stockpile-picker' : '.cache/product-ui');
let source, tag;
if (!stockpilePicker) {
  source = spawnSync('git', ['rev-parse', 'HEAD'], {cwd: repo, encoding: 'utf8'}).stdout.trim();
  tag = spawnSync('git', ['rev-parse', 'beta-0.4.0^{}'], {cwd: repo, encoding: 'utf8'}).stdout.trim();
  const diff = spawnSync('git', ['diff', tag, '--', 'desktop-nova/src', 'desktop-nova/ui', 'desktop-nova/package.json'], {cwd: repo, encoding: 'utf8'});
  if (diff.status !== 0 || diff.stdout) throw new Error('Product UI differs from the published 0.4 tag.');
}
// Adapter-only debugging may reuse the just-built isolated bundle. The normal
// invocation always builds; the published-media mode never skips its build.
const reusePickerBuild = stockpilePicker && process.argv.includes('--reuse-build');
if (reusePickerBuild) {
  assert.ok(fs.existsSync(path.join(output,'index.html')), 'Run --stockpile-picker once before --reuse-build.');
} else {
  const build = spawnSync(process.execPath, [path.join(frontend, 'node_modules/vite/bin/vite.js'), 'build', '--outDir', output], {cwd: frontend, stdio: 'pipe', encoding: 'utf8'});
  if (build.status !== 0) throw new Error(build.stderr || build.stdout);
}
const mime = {'.html':'text/html','.js':'text/javascript','.css':'text/css','.png':'image/png','.svg':'image/svg+xml','.woff2':'font/woff2'};
const server = http.createServer((req, res) => {
  let filename = path.resolve(output, '.' + decodeURIComponent((req.url || '/').split('?')[0]));
  if (!filename.startsWith(output + path.sep) && filename !== output) {res.writeHead(403).end();return;}
  if (filename === output) filename = path.join(output, 'index.html');
  if (!fs.existsSync(filename) || !fs.statSync(filename).isFile()) {res.writeHead(404).end();return;}
  res.writeHead(200, {'Content-Type':mime[path.extname(filename)] || 'application/octet-stream'});
  fs.createReadStream(filename).pipe(res);
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));

// The picker mode uses the same full production Vite bundle and headless Edge
// as the media capture. Only the native boundary is isolated; no copied page,
// component, layout CSS, source-file writes or material calculations are used.
async function captureStockpilePicker(browser) {
  const artifacts = path.join(repo, 'output/playwright/stockpile-picker');
  fs.mkdirSync(artifacts, {recursive:true});
  const fixtureRoot = process.env.NOVA_PICKER_LIBRARY_ROOT || 'C:/Users/27232/Documents/投影测试';
  const files = fs.readdirSync(fixtureRoot, {withFileTypes:true})
    .filter(entry => entry.isFile() && /\.(?:litematic|schem|schematic|nbt)$/i.test(entry.name))
    .map(entry => {const filename = path.join(fixtureRoot, entry.name); const stat = fs.statSync(filename); return {path:filename,name:entry.name,size:stat.size,mtime:stat.mtimeMs};})
    .sort((a,b) => a.name.localeCompare(b.name, 'zh-CN'));
  assert.ok(files.length >= 10, 'At least ten real read-only projection files are required.');
  const metadata = files.map(file => ({...file, displayName:file.name.replace(/\.[^.]+$/, ''), fixtureOnly:false}));
  // Deliberately labelled metadata-only stress rows: these paths are never read
  // or submitted to the Host. The other rows retain real directory identities.
  const longName = '仅离屏元数据_多投影合并备货_超长中文名称与EnglishName_'.repeat(5);
  metadata.splice(3, 0, {...files[0], path:path.win32.join('X:\\Nova-picker-metadata-only', '多级来源目录'.repeat(8), 'secondary', `${longName.slice(0,150)}.litematic`), displayName:longName, fixtureOnly:true});
  metadata.splice(13, 0, {...files[1], path:path.win32.join('X:\\Nova-picker-metadata-only', 'deep', 'nested', 'a-very-long-resource-folder-'.repeat(6), 'same-prefix-identity-B.litematic'), displayName:longName + ' B', fixtureOnly:true});
  const library = {schemaVersion:5, entries:[],replicas:[],folders:[],categories:[],trash:[],redirects:[],favorites:[],recent:[]};
  for (const [index, file] of metadata.entries()) {
    const id = `offscreen-entry-${index}`;
    library.entries.push({id,createdAt:0,checksum:null,status:'active',categoryIds:[],displayName:file.displayName,author:'',description:'',tags:[],preview:{},stats:{totalBlocks:0,totalVolume:0,regionCount:0,enclosingSize:{x:0,y:0,z:0},minecraftDataVersion:0},sortIndex:index,lastAnalyzedAt:0,lastError:''});
    library.replicas.push({id:`offscreen-replica-${index}`,entryId:id,path:file.path,fileName:path.win32.basename(file.path),folderPath:path.win32.dirname(file.path),checksum:null,fileSize:file.size,mtime:file.mtime,status:'active',lastAnalyzedAt:0,lastError:''});
  }
  const report = {schema:1,kind:'production-StockpileSourcesDialog-with-isolated-native-library-adapter',appVersion:JSON.parse(fs.readFileSync(path.join(frontend,'package.json'),'utf8')).version,
    visibleGuiOperated:false,nativeNovaProcessLaunched:false,productionCssOverridden:false,rustStatisticsValidated:false,bundleBuiltAt:fs.statSync(path.join(output,'index.html')).mtime.toISOString(),reusedBuild:reusePickerBuild,
    library:{root:path.resolve(fixtureRoot),realFileCount:files.length,metadataOnlyCount:metadata.filter(file=>file.fixtureOnly).length,userDataWrites:0},
    scaleEmulation:'Tauri WebView zoom uses fewer layout CSS pixels at higher scale. Context viewport is physical size / ui_scale; deviceScaleFactor=ui_scale restores physical screenshot pixels. No CSS zoom/transform overrides.',scenarios:[]};
  for (const theme of ['shadcn','8bitcn','Minecraft']) for (const scale of [1,2]) {
    // A taller isolated viewport exposes ten complete rows even at 2x; width
    // remains 964 physical pixels to exercise the actual narrow-dialog cascade.
    const viewport = {width:Math.round(964/scale),height:1400};
    const context = await browser.newContext({viewport,locale:'zh-CN',deviceScaleFactor:scale,colorScheme:'dark'});
    const scenario = {theme,scale,cssViewport:viewport,physicalViewport:{width:viewport.width*scale,height:viewport.height*scale},checks:[],screenshots:[],pageErrors:[]};
    report.scenarios.push(scenario);
    const page = await context.newPage();
    page.on('pageerror', error=>scenario.pageErrors.push(error.message));
    try {
      await context.route('**/*', route => /^(?:data:|http:\/\/127\.0\.0\.1:)/.test(route.request().url()) ? route.continue() : route.abort());
      await context.addInitScript(({theme,scale,library,currentFile,appVersion}) => {
        localStorage.setItem('nova.ui.language','zh-CN');
        localStorage.setItem('theme',theme);
        localStorage.setItem('litematicanova-theme-id',theme);
        localStorage.setItem('litematicanova-color-mode','dark');
        localStorage.setItem('lba.appdataConfigMigrated.v1','1');
        localStorage.setItem('nova.update-recovery.v1',JSON.stringify({shell:{route:'statistics',currentFile}}));
        window.isTauri = true;
        let serial=0;
        const memoryFiles = new Map([['projection-library/js_library.json',JSON.stringify(library)]]);
        window.__novaPickerCapture = {memoryWrites:0,mutationCalls:[],libraryReads:0};
        const config={ui_scale:scale,ui_language:'zh-CN',ui_locale:'zh-CN',theme,color_mode:'dark',motion_preference:'off',camera_input_mode:'drag',camera_speed:12,camera_sensitivity:1,render_display_mode:'standard',preview_mode:'standard',graphics_backend:'auto',material_list_window_behavior:'independent_window',debug_mode:false,show_ui_test_page:false,local_library_tail_path_count:3,statistics_enumerator_info_rules:[],v5_compatibility:{backend:'auto',resource_packs:[],shaderpack_root:'',shaderpack_name:'',iris_fallback:'fail',fabric_mods_dir:''},viewer_controls:{allow_time_modification:true,allow_weather_modification:true}};
        window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'main'},currentWebview:{label:'main'}},convertFileSrc:()=>'',transformCallback:callback=>{const id=++serial;window[`_${id}`]=callback;return id;},unregisterCallback:id=>delete window[`_${id}`],invoke:async(command,args)=>{
          if(command==='get_user_config'||command==='save_user_config')return{config_dir:'X:\\Nova-picker-isolated',default_config_dir:'X:\\Nova-picker-isolated',config};
          if(command==='plugin:app|version')return appVersion;
          if(command==='plugin:event|listen')return++serial;
          if(command==='application_take_open_files')return[];
          if(command==='application_recent_projects')return{projects:[]};
          if(command==='application_render_get_snapshot')return{operations:[]};
          if(command==='application_close_request')return null;
          if(command==='read_user_config_file'){window.__novaPickerCapture.libraryReads++;return memoryFiles.get(args.relativePath)||null;}
          if(command==='write_user_config_file'){window.__novaPickerCapture.memoryWrites++;memoryFiles.set(args.relativePath,args.content);return null;}
          if(command==='get_user_config_file_path')return 'X:\\Nova-picker-isolated\\'+args.relativePath.replaceAll('/','\\');
          if(command==='read_language_resource')return{};
          if(command==='application_stockpile_task')return{tasks:[],legacyTasks:[],selectedTaskId:null,task:null};
          if(command.startsWith('application_stockpile_task_')){window.__novaPickerCapture.mutationCalls.push(command);throw new Error('Picker capture never commits stockpile mutations.');}
          if(command==='application_data_query'){
            if(args.input.kind==='format')return{nativePassthrough:true};
            // This test validates selection/layout, not invented Rust statistics.
            throw new Error('Isolated picker capture: statistics backend not connected.');
          }
          if(command==='application_library_index_query')return{entries:[],total:0};
          if(command.includes('is_maximized')||command.includes('is_minimized'))return false;
          if(command.includes('is_visible'))return true;
          if(command.includes('scale_factor'))return scale;
          if(command.includes('position'))return{x:0,y:0};
          return null;
        }};
        window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}};
      },{theme,scale,library,currentFile:files[0].path,appVersion:report.appVersion});
      await page.goto(`http://127.0.0.1:${server.address().port}/`,{waitUntil:'networkidle'});
      await page.waitForSelector('.app-shell',{timeout:15000});
      await page.getByRole('button',{name:'新建任务…',exact:true}).click();
      const dialog = page.locator('.stockpile-sources-dialog');
      await dialog.waitFor();
      await dialog.locator('.stockpile-source-library > summary').click();
      const list = dialog.locator('.stockpile-source-library-list');
      await page.waitForFunction(count => document.querySelectorAll('.stockpile-source-library-row').length === count, metadata.length);
      await page.evaluate(()=>document.fonts.ready);
      const measure = async (stage,expected,minVisible=1) => {
        await list.scrollIntoViewIfNeeded();
        await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
        const result = await list.evaluate((element,{stage,expected,minVisible})=>{
          const rect = node => {const r=node.getBoundingClientRect();return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,width:r.width,height:r.height};};
          const bounds=rect(element), style=getComputedStyle(element), tolerance=1, violations=[];
          const contentRight=bounds.left+element.clientLeft+element.clientWidth;
          const clip={top:Math.max(bounds.top,0),bottom:Math.min(bounds.bottom,innerHeight)};
          for(let parent=element.parentElement;parent;parent=parent.parentElement){const s=getComputedStyle(parent);if(/auto|scroll|hidden/.test(s.overflowY)){const r=rect(parent);clip.top=Math.max(clip.top,r.top);clip.bottom=Math.min(clip.bottom,r.bottom);}}
          const rows=[...element.querySelectorAll('.stockpile-source-library-row')].map(row=>{
            const name=row.querySelector('.stockpile-source-library-name'),path=row.querySelector('.stockpile-source-library-path'),checkbox=row.querySelector('[role="checkbox"]');
            const r=rect(row),n=rect(name),p=rect(path),c=rect(checkbox);
            const identity=row.dataset.sourcePath, expectedRow=expected.find(value=>value.path===identity);
            if(!expectedRow)violations.push('unknown identity '+identity);
            if(name.textContent!==expectedRow?.displayName||name.title!==name.textContent||path.title!==identity||path.textContent!==identity)violations.push('full name/path/title lost '+identity);
            if(checkbox.getAttribute('aria-label')!==`${name.textContent} ${identity}`)violations.push('incomplete accessible identity '+identity);
            for(const [label,box]of [['name',n],['path',p],['checkbox',c]])if(box.top<r.top-tolerance||box.bottom>r.bottom+tolerance||box.left<r.left-tolerance||box.right>Math.min(r.right,contentRight)+tolerance)violations.push(label+' outside row/gutter '+identity);
            if(n.bottom>p.top+tolerance||c.right>n.left+tolerance)violations.push('name/path/checkbox overlap '+identity);
            if(Math.abs(c.width-18)>0.75||Math.abs(c.height-18)>0.75)violations.push('checkbox must measure 18px square '+identity);
            for(const node of [name,path]){const s=getComputedStyle(node);if(s.whiteSpace!=='nowrap'||s.textOverflow!=='ellipsis'||s.overflowX!=='hidden')violations.push('single-line ellipsis missing '+identity);}
            return {identity,row:r,name:n,path:p,checkbox:c,checked:checkbox.getAttribute('aria-checked'),fullyVisible:r.top>=clip.top-tolerance&&r.bottom<=clip.bottom+tolerance};
          });
          for(let index=1;index<rows.length;index++)if(rows[index].row.top<rows[index-1].row.bottom-tolerance)violations.push('adjacent row overlap at '+index);
          if(rows.length!==expected.length)violations.push(`row count ${rows.length} != ${expected.length}`);
          if(new Set(rows.map(row=>row.identity)).size!==rows.length)violations.push('duplicate displayed identity');
          if(rows.filter(row=>row.fullyVisible).length<minVisible)violations.push('too few complete visible rows');
          if(element.scrollWidth>element.clientWidth+tolerance)violations.push('list horizontal overflow');
          if(getComputedStyle(document.documentElement).zoom!=='1')violations.push('unexpected CSS zoom');
          return {stage,rowCount:rows.length,fullyVisible:rows.filter(row=>row.fullyVisible).length,list:bounds,clientWidth:element.clientWidth,scrollTop:element.scrollTop,scrollHeight:element.scrollHeight,clientHeight:element.clientHeight,overflowY:style.overflowY,clip,rows,violations};
        },{stage,expected:expected.map(file=>({path:file.path,displayName:file.displayName})),minVisible});
        scenario.checks.push(result);
        const screenshot=path.join(artifacts,`${theme.toLowerCase()}-${scale}x-${stage}.png`);
        await page.screenshot({path:screenshot});scenario.screenshots.push(screenshot);
        return result;
      };
      await measure('first',metadata,10);
      const chosen=[metadata[0],metadata[3]];
      for(const file of chosen){ // Use the full accessible identity, never truncated display text.
        const row=list.locator('.stockpile-source-library-row').filter({hasText:file.displayName}).filter({has:page.getByRole('checkbox',{name:`${file.displayName} ${file.path}`,exact:true})});
        await row.getByRole('checkbox').check();
      }
      await measure('selected',metadata,10);
      await list.evaluate(element=>{element.scrollTop=(element.scrollHeight-element.clientHeight)/2;});
      const middle=await measure('middle',metadata);
      assert.ok(middle.scrollTop>0,'Middle scrolling must move the production list.');
      await list.evaluate(element=>{element.scrollTop=element.scrollHeight;});
      const bottom=await measure('bottom',metadata);
      assert.ok(Math.abs(bottom.scrollTop+bottom.clientHeight-bottom.scrollHeight)<=2,'Bottom must be reachable.');
      const search=dialog.locator('.stockpile-source-library > input');
      const filtered=metadata.filter(file=>!file.fixtureOnly);
      await search.fill(path.basename(fixtureRoot));
      await page.waitForFunction(count=>document.querySelectorAll('.stockpile-source-library-row').length===count,filtered.length);
      await list.evaluate(element=>{element.scrollTop=0;});
      await measure('search-first',filtered,10);
      await list.evaluate(element=>{element.scrollTop=element.scrollHeight;});
      await measure('search-bottom',filtered);
      await search.fill('no-offscreen-projection-can-match-this-value');
      await page.waitForFunction(()=>document.querySelectorAll('.stockpile-source-library-row').length===0);
      await search.fill('');
      await page.waitForFunction(count=>document.querySelectorAll('.stockpile-source-library-row').length===count,metadata.length);
      for(const file of chosen)assert.equal(await page.getByRole('checkbox',{name:`${file.displayName} ${file.path}`,exact:true}).getAttribute('aria-checked'),'true','Selection must survive scrolling/filter clearing.');
      await dialog.getByRole('button',{name:'添加库中所选投影',exact:true}).click();
      const sourcePaths=await dialog.locator('.stockpile-source-path').allTextContents();
      for(const file of chosen)assert.ok(sourcePaths.includes(file.path),'Added source identity must equal original DTO path.');
      scenario.adapter=await page.evaluate(()=>window.__novaPickerCapture);
      assert.deepEqual(scenario.adapter.mutationCalls,[],'Capture must not mutate a stockpile task.');
      assert.ok(scenario.adapter.libraryReads>0,'Production loadLibrary must read the native adapter.');
      assert.deepEqual(scenario.pageErrors,[],'Production page must remain mounted without runtime errors.');
      assert.deepEqual(scenario.checks.flatMap(check=>check.violations.map(value=>`${check.stage}: ${value}`)),[],`${theme} ${scale}x geometry`);
      scenario.passed=true;
      console.log(`PASS picker ${theme} ${scale}x: ${metadata.length} identities, first=${scenario.checks[0].fullyVisible}, middle/bottom/search/selection, no overlaps.`);
    } catch(error) {
      scenario.passed=false;scenario.error=String(error.stack||error);
      const screenshot=path.join(artifacts,`${theme.toLowerCase()}-${scale}x-failure.png`);
      await page.screenshot({path:screenshot}).catch(()=>undefined);scenario.screenshots.push(screenshot);
      console.error(`FAIL picker ${theme} ${scale}x: ${error.message}`);
    } finally {await context.close();}
  }
  for(const file of files){const stat=fs.statSync(file.path);assert.equal(stat.size,file.size);assert.equal(stat.mtimeMs,file.mtime);}
  report.passed=report.scenarios.every(scenario=>scenario.passed);
  const reportPath=path.join(artifacts,'stockpile-picker-bounds.json');
  fs.writeFileSync(reportPath,JSON.stringify(report,null,2));
  console.log(`Picker bounds and screenshots: ${reportPath}`);
  assert.ok(report.passed,'Stockpile picker geometry failed; inspect the bounds report.');
}

let browser;
try {
  browser = await chromium.launch({channel:'msedge', headless:true});
  if (stockpilePicker) {
    await captureStockpilePicker(browser);
  } else {
  for (const locale of ['zh-CN','en-US']) {
    const context = await browser.newContext({viewport:{width:1600,height:1000},locale,deviceScaleFactor:1,colorScheme:'dark'});
    await context.route('**/*', route => /^(?:data:|http:\/\/127\.0\.0\.1:)/.test(route.request().url()) ? route.continue() : route.abort());
    await context.addInitScript(({locale}) => {
      localStorage.setItem('nova.ui.language', locale);
      localStorage.setItem('theme', 'shadcn');
      localStorage.setItem('color_mode', 'dark');
      localStorage.setItem('lba.appdataConfigMigrated.v1','1');
      window.isTauri = true;
      let serial=0;
      const config={ui_scale:1,ui_language:locale,ui_locale:locale,theme:'shadcn',color_mode:'dark',render_display_mode:'standard',preview_mode:'standard',graphics_backend:'auto',material_list_window_behavior:'independent_window',debug_mode:false,show_ui_test_page:false,local_library_tail_path_count:3,statistics_enumerator_info_rules:[],v5_compatibility:{backend:'auto',resource_packs:[],shaderpack_root:'',shaderpack_name:'',iris_fallback:'fail',fabric_mods_dir:''},viewer_controls:{allow_time_modification:true,allow_weather_modification:true}};
      window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'main'},currentWebview:{label:'main'}},convertFileSrc:()=>'',transformCallback:callback=>{const id=++serial;window[`_${id}`]=callback;return id;},unregisterCallback:id=>delete window[`_${id}`],invoke:async(command,args)=>{
        if(command==='get_user_config'||command==='save_user_config')return{config_dir:'Demo',default_config_dir:'Demo',config};
        if(command==='plugin:app|version')return'0.4.0';
        if(command==='plugin:event|listen')return++serial;
        if(command==='application_take_open_files')return[];
        if(command==='application_recent_projects')return{projects:[]};
        if(command==='application_close_request')return null;
        if(command==='read_user_config_file')return null;
        if(command==='read_language_resource')return{};
        if(command==='application_data_query')return{projects:[],records:[],entries:[],categories:[],folders:[],items:[],total:0};
        if(command==='application_library_index_query')return{entries:[],total:0};
        if(command.includes('is_maximized')||command.includes('is_minimized'))return false;
        if(command.includes('is_visible'))return true;
        if(command.includes('scale_factor'))return 1;
        if(command.includes('position'))return{x:0,y:0};
        return null;
      }};
      window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}};
    },{locale});
    const page=await context.newPage();
    const errors=[];page.on('pageerror',error=>errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.address().port}/`,{waitUntil:'networkidle'});
    await page.waitForSelector('.app-shell', {timeout:15000});
    await page.evaluate(()=>document.fonts.ready);
    if(errors.length)throw new Error('Product UI failed to mount: '+errors.join('; '));
    await page.screenshot({path:path.join(workspace,`assets/original/product-ui-${locale}.png`)});
    await context.close();
  }
  fs.writeFileSync(path.join(workspace,'assets/original/product-ui-capture.json'),JSON.stringify({schema:1,appVersion:'0.4.0',releaseTag:'beta-0.4.0',publishedSource:tag,checkout:source,kind:'actual-published-React-UI-with-empty-demo-native-adapter',nativeNovaProcessLaunched:false,personalUserDataRead:false,width:1600,height:1000,files:['product-ui-zh-CN.png','product-ui-en-US.png']},null,2));
  console.log('Published React UI captured in an isolated browser context (empty demo data).');
  }
} finally {if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));}
