import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {chromium} from 'playwright';

// Fixed, isolated media capture of the actual released React UI, not a Nova
// process or the user's WebView. Native calls have explicit empty-demo replies.
const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repo = path.resolve(workspace, '../..');
const frontend = path.join(repo, 'desktop-nova');
const output = path.join(workspace, '.cache/product-ui');
const source = spawnSync('git', ['rev-parse', 'HEAD'], {cwd: repo, encoding: 'utf8'}).stdout.trim();
const tag = spawnSync('git', ['rev-parse', 'beta-0.4.0^{}'], {cwd: repo, encoding: 'utf8'}).stdout.trim();
const diff = spawnSync('git', ['diff', tag, '--', 'desktop-nova/src', 'desktop-nova/ui', 'desktop-nova/package.json'], {cwd: repo, encoding: 'utf8'});
if (diff.status !== 0 || diff.stdout) throw new Error('Product UI differs from the published 0.4 tag.');
const build = spawnSync(process.execPath, [path.join(frontend, 'node_modules/vite/bin/vite.js'), 'build', '--outDir', output], {cwd: frontend, stdio: 'pipe', encoding: 'utf8'});
if (build.status !== 0) throw new Error(build.stderr || build.stdout);
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
let browser;
try {
  browser = await chromium.launch({channel:'msedge', headless:true});
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
} finally {if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));}
