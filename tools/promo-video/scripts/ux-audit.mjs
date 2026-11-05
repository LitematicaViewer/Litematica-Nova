import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';

// External IO boundary only. Production React, CSS, portals, handlers and focus
// management are bundled unchanged by capture-product-ui.mjs.
export async function installBoundary(context, options = {}) {
  await context.route('**/*', route => /^(?:data:|http:\/\/127\.0\.0\.1:)/.test(route.request().url()) ? route.continue() : route.abort());
  await context.addInitScript(({route='home',theme='shadcn',locale='zh-CN',currentFile='',config:patch={},files={},invokeReplies={}}) => {
    localStorage.setItem('nova.ui.language', locale);
    localStorage.setItem('theme', theme); localStorage.setItem('litematicanova-theme-id', theme);
    localStorage.setItem('litematicanova-color-mode',patch.color_mode || 'dark');
    localStorage.setItem('lba.appdataConfigMigrated.v1','1');
    localStorage.setItem('nova.update-recovery.v1',JSON.stringify({shell:{route,currentFile}}));
    window.isTauri=true;
    let serial=0;const listeners=new Map();
    const config={ui_scale:1,ui_language:locale,ui_locale:locale,theme,color_mode:'dark',motion_preference:'standard',camera_input_mode:'drag',camera_speed:12,camera_sensitivity:1,render_display_mode:'standard',preview_mode:'standard',graphics_backend:'auto',material_list_window_behavior:'overlay',debug_mode:true,show_ui_test_page:true,local_library_tail_path_count:3,statistics_enumerator_info_rules:[],v5_compatibility:{backend:'auto',resource_packs:[],shaderpack_root:'',shaderpack_name:'',iris_fallback:'fail',fabric_mods_dir:''},viewer_controls:{allow_time_modification:true,allow_weather_modification:true},...patch};
    const update={phase:'current',currentVersion:'0.4.1',version:null,date:null,notes:null,downloaded:0,total:null,source:null,message:null};
    const state=window.__novaUX={calls:[],unknown:[],writes:[],replies:invokeReplies,handlers:{},config,files,update,
      emit(event,payload){for(const [id,v] of listeners){if(v.event===event)window[`_${v.handler}`]?.({event,id,payload});}}};
    window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'main'},currentWebview:{label:'main'}},convertFileSrc:()=>'',transformCallback:callback=>{const id=++serial;window[`_${id}`]=callback;return id;},unregisterCallback:id=>delete window[`_${id}`],invoke:async(command,args={})=>{
      state.calls.push({command,args});
      if(state.handlers[command])return state.handlers[command](args);
      if(Object.hasOwn(state.replies,command))return structuredClone(state.replies[command]);
      if(command==='get_user_config')return{config_dir:'X:\\Nova-UX-isolated',default_config_dir:'X:\\Nova-UX-isolated',config:{...config}};
      if(command==='save_user_config'){Object.assign(config,args.input||args.config||args);const value={config_dir:'X:\\Nova-UX-isolated',default_config_dir:'X:\\Nova-UX-isolated',config:{...config}};state.emit('user-config-changed',value);return value;}
      if(command==='plugin:app|version')return'0.4.1';
      if(command==='plugin:event|listen'){const id=++serial;listeners.set(id,args);return id;}
      if(command==='plugin:event|unlisten'){listeners.delete(args.eventId);return;}
      if(command==='plugin:event|emit'){state.emit(args.event,args.payload);return;}
      if(command==='application_take_open_files')return[];
      if(command==='application_recent_projects')return{projects:[]};
      if(command==='application_render_get_snapshot')return{operations:[]};
      if(command==='application_close_request'||command==='application_get_close_request')return null;
      if(command==='read_user_config_file')return files[args.relativePath]??null;
      if(command==='write_user_config_file'){files[args.relativePath]=args.content;state.writes.push(args.relativePath);return;}
      if(command==='get_user_config_file_path')return'X:\\Nova-UX-isolated\\'+args.relativePath.replaceAll('/','\\');
      if(command==='read_language_resource'||command==='read_resource_pack_language')return{};
      if(command==='application_update_snapshot'||command==='application_update_check')return{...update};
      if(command==='application_update_cancel'){update.phase='cancelled';return{...update};}
      if(command==='application_update_skip'){update.phase='skipped';return{...update};}
      if(command==='application_library_content')return{entries:[],categories:[],folders:[],total:0};
      if(command==='application_library_index_query')return{entries:[],total:0};
      if(command==='application_stockpile_task')return{tasks:[],legacyTasks:[],selectedTaskId:null,task:null};
      if(command==='application_data_query'&&args.input?.kind==='format')return{nativePassthrough:true};
      if(command==='read_game_data_icon')return null;
      if(command==='game_data_list')return{versions:[],installed:[],mods:[],activeVersion:null};
      if(command==='ai_get_config')return{providers:[],active_provider_id:null};
      if(command==='plugin:window|set_background_color')return;
      if(command==='native_preview_set_background')return;
      if(command.startsWith('plugin:dialog|'))return null;
      if(command.includes('is_maximized')||command.includes('is_minimized'))return false;
      if(command.includes('is_visible')||command.includes('is_focused'))return true;
      if(command.includes('scale_factor'))return 1;
      if(command.includes('position'))return{x:0,y:0};
      if(command.includes('size'))return{width:innerWidth,height:innerHeight};
      state.unknown.push({command,args});throw new Error('Isolated UX boundary not configured: '+command);
    }};
    window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}};
  },options);
}

export async function auditShell({browser,baseURL,outputDir,installBoundary}) {
  const report={kind:'production-routes-and-controls; isolated Tauri IO',nativeGui:false,scenarios:[],failures:[]};
  for(const [index,theme] of ['shadcn','8bitcn','Minecraft','Metro10','aero7'].entries())for(const locale of ['zh-CN','en-US'])for(const colorScheme of ['light','dark']){
    const deviceScaleFactor=[1,1.25,1.5,2][(index+(locale==='en-US'?1:0))%4];
    const context=await browser.newContext({viewport:{width:1280,height:720},locale,colorScheme,deviceScaleFactor});
    await installBoundary(context,{route:'ui_test',theme,locale,config:{color_mode:colorScheme,motion_preference:'off'}});
    const page=await context.newPage();page.setDefaultTimeout(7000);const errors=[];page.on('pageerror',e=>errors.push(e.message));
    const result={theme,locale,colorScheme,deviceScaleFactor,states:[],errors};
    try{
      await page.goto(baseURL,{waitUntil:'networkidle'});await page.waitForSelector('.app-shell');await page.evaluate(()=>document.fonts.ready);await page.getByRole('button',{name:locale==='en-US'?'UI Test':'UI 测试',exact:true}).click();
      const checkbox=page.locator('[role="checkbox"]').first();await checkbox.scrollIntoViewIfNeeded();result.choiceGeometry=await checkbox.evaluate(el=>{const c=getComputedStyle(el);return{rect:el.getBoundingClientRect().toJSON(),width:c.width,minWidth:c.minWidth,padding:c.padding,border:c.border,font:c.font}});assert.equal(result.choiceGeometry.rect.width,16);assert.equal(result.choiceGeometry.rect.height,16);await checkbox.click();assert.equal(await checkbox.getAttribute('aria-checked'),'true');
      await page.screenshot({path:path.join(outputDir,`shell-${theme}-${locale}-${colorScheme}-controls.png`)});
      if(theme==='aero7'){
        result.aeroContrast=await page.locator('input[data-slot="input"]').first().evaluate(el=>{
          const rgb=s=>s.match(/[\d.]+/g).map(Number),composite=(a,b)=>a.slice(0,3).map((v,i)=>v*(a[3]??1)+b[i]*(1-(a[3]??1)));
          const style=getComputedStyle(el),base=rgb(getComputedStyle(document.documentElement).backgroundColor);
          const background=composite(rgb(style.backgroundColor),base),foreground=rgb(style.color);
          const luminance=c=>c.slice(0,3).map(v=>v/255).map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4).reduce((sum,v,i)=>sum+v*[.2126,.7152,.0722][i],0);
          const a=luminance(foreground),b=luminance(background);
          return{foreground,background,ratio:(Math.max(a,b)+.05)/(Math.min(a,b)+.05)};
        });
        assert.ok(result.aeroContrast.ratio>=4.5,'Aero input text contrast remains readable in both palettes');
        const primary=page.locator('.button-variant-demo [data-variant="default"]').first();
        assert.notEqual(await primary.evaluate(el=>getComputedStyle(el).backgroundColor),'rgba(0, 0, 0, 0)','translucent Aero primary artwork has its semantic backing');
      }
      const mixed=page.locator('[role="checkbox"][aria-checked="mixed"]').first();assert.equal(await mixed.count(),1);assert.equal(await mixed.locator('[data-slot="checkbox-mixed"]').isVisible(),true);assert.equal(await mixed.locator('[data-slot="checkbox-check"]').isVisible(),false);
      assert.ok(await page.locator('[role="checkbox"]:disabled').count());
      await checkbox.press('Space');assert.equal(await checkbox.getAttribute('aria-checked'),'false');
      const beforeHover=await checkbox.boundingBox();await checkbox.hover();assert.deepEqual(await checkbox.boundingBox(),beforeHover);await page.mouse.down();assert.deepEqual(await checkbox.boundingBox(),beforeHover);await page.mouse.up();
      result.states.push({state:'checkbox-default-hover-pressed-selected-mixed-disabled-keyboard',stableGeometry:true});
      const radio=page.locator('[role="radiogroup"]').first();await radio.locator('[role="radio"]').first().focus();await page.keyboard.press('ArrowRight');assert.equal(await radio.locator('[aria-checked="true"]').count(),1);
      const select=page.locator('[role="combobox"]').first();await select.scrollIntoViewIfNeeded();await select.click();await page.waitForSelector('[role="listbox"]');await page.keyboard.press('ArrowDown');await page.keyboard.press('Enter');assert.equal(await page.locator('[role="listbox"]').count(),0);
      const label=locale==='en-US'?/Open.*Dialog/i:/打开.*对话框/;const opener=page.getByRole('button',{name:label}).first();await opener.click();await page.waitForSelector('[role="dialog"]');
      result.states.push(await page.locator('[role="dialog"]').evaluate(el=>({state:'dialog',rect:el.getBoundingClientRect().toJSON(),style:{overflow:getComputedStyle(el).overflow}})));
      await page.screenshot({path:path.join(outputDir,`shell-${theme}-${locale}-${colorScheme}-dialog.png`)});
      await page.keyboard.press('Escape');assert.equal(await page.locator('[role="dialog"]').count(),0);assert.equal(await opener.evaluate(el=>document.activeElement===el),true);
      await page.locator('[data-route="home"]').click();
      for(const [width,height] of [[800,500],[856,505],[960,600],[1600,900],[1920,1080],[2560,1440],[3840,2160]]){
        await page.setViewportSize({width,height});
        const g=await page.evaluate(()=>{const b=s=>document.querySelector(s).getBoundingClientRect().toJSON();return{viewport:{width:innerWidth,height:innerHeight},sidebar:b('.sidebar'),content:b('.content'),footer:b('.nav-list-bottom'),overflow:document.documentElement.scrollWidth>innerWidth+1}});
        assert.equal(g.overflow,false);assert.ok(g.footer.bottom<=height+1);assert.ok(g.content.width>0);result.states.push(g);
        if(width===800||width===1600){
          const cards=await page.locator('.home-workflows > button').evaluateAll(nodes=>nodes.map(el=>({rect:el.getBoundingClientRect().toJSON(),text:[...el.querySelectorAll('strong,small')].map(t=>t.getBoundingClientRect().toJSON())})));
          for(const [i,card] of cards.entries()){
            for(const text of card.text)assert.ok(text.top>=card.rect.top-1&&text.bottom<=card.rect.bottom+1,'home action text stays inside its row');
            if(i)assert.ok(cards[i-1].rect.bottom<=card.rect.top+1,'home action rows never overlap');
          }
          result.states.push({state:'home-multiline-row-geometry',width,cards});
          await page.screenshot({path:path.join(outputDir,`shell-${theme}-${locale}-${colorScheme}-${width}.png`)});
        }
      }
      result.unknown=await page.evaluate(()=>window.__novaUX.unknown);assert.equal(result.unknown.length,0,JSON.stringify(result.unknown));assert.equal(errors.length,0);
    }catch(e){result.failure=e.message;report.failures.push({theme,locale,colorScheme,error:e.message});await page.screenshot({path:path.join(outputDir,`shell-${theme}-${locale}-${colorScheme}-failure.png`)}).catch(()=>{});}
    report.scenarios.push(result);await context.close();
  }
  fs.writeFileSync(path.join(outputDir,'shell.json'),JSON.stringify(report,null,2));return report;
}


export async function auditShellBoundaries({browser,baseURL,outputDir,installBoundary}) {
  const report={kind:'production-shell-scale-motion-updater; isolated IO',nativeGui:false,scenarios:[],failures:[]};
  const context=await browser.newContext({viewport:{width:800,height:500},locale:'en-US'});
  await installBoundary(context,{theme:'shadcn',locale:'en-US',config:{motion_preference:'system'}});
  const page=await context.newPage();page.setDefaultTimeout(6000);
  try {
    await page.goto(baseURL,{waitUntil:'networkidle'});
    const scales=[];
    for(let tick=10;tick<=100;tick++) {
      const scale=tick/20,width=Math.floor(800/scale),height=Math.floor(500/scale);
      await page.setViewportSize({width,height});
      const geometry=await page.evaluate(()=>{const q=s=>document.querySelector(s).getBoundingClientRect().toJSON();return{rootOverflow:document.documentElement.scrollWidth>innerWidth+1,sidebar:q('.sidebar'),content:q('.content'),footer:q('.nav-list-bottom')};});
      assert.equal(geometry.rootOverflow,false,`root overflow at ${scale}`);
      assert.ok(geometry.content.width>0 && geometry.footer.bottom<=height+1,`unreachable shell at ${scale}`);
      scales.push({scale,viewport:{width,height},...geometry});
    }
    report.scenarios.push({name:'all-91-scale-geometry',method:'Tauri zoom equivalent layout viewport, no CSS transforms',scales});
    await page.setViewportSize({width:800,height:500});
    await page.emulateMedia({reducedMotion:'reduce'});
    await page.waitForFunction(()=>document.documentElement.dataset.motion==='reduced');
    await page.emulateMedia({reducedMotion:'no-preference'});
    await page.waitForFunction(()=>document.documentElement.dataset.motion==='standard');
    for(const preference of ['off','reduced','standard','rich','system']) {
      await page.evaluate(preference=>{window.__novaUX.config.motion_preference=preference;window.__novaUX.emit('user-config-changed',{config:{...window.__novaUX.config}});},preference);
      await page.waitForFunction(preference=>document.documentElement.dataset.motion===(preference==='system'?'standard':preference),preference);
    }
    report.scenarios.push({name:'five-motion-modes-live-and-system-change',passed:true});
    await page.getByRole('button',{name:'UI Test',exact:true}).click();
    const dialogOpener=page.getByRole('button',{name:/Open.*Dialog/i}).first();
    await dialogOpener.click();await page.waitForSelector('[role="dialog"]');
    const motion=await page.locator('[role="dialog"]').evaluate(async el=>{
      const animation=el.getAnimations().find(a=>a.effect?.target===el);
      if(!animation)throw new Error('Production dialog entry animation not present');
      animation.pause();const duration=Number(animation.effect.getComputedTiming().duration),frames=[];
      for(const time of [0,duration/2,duration]){animation.currentTime=time;await new Promise(requestAnimationFrame);const style=getComputedStyle(el);frames.push({time,opacity:Number(style.opacity),transform:style.transform,rect:el.getBoundingClientRect().toJSON()});}
      animation.finish();return {duration,frames};
    });
    assert.ok(motion.frames[0].opacity<motion.frames[1].opacity && motion.frames[1].opacity<=motion.frames[2].opacity);
    await page.keyboard.press('Escape');assert.equal(await page.locator('[role="dialog"]').count(),0);
    await dialogOpener.click();await page.keyboard.press('Escape');assert.equal(await page.locator('[role="dialog"]').count(),0);
    assert.equal(await dialogOpener.evaluate(el=>document.activeElement===el),true);
    report.scenarios.push({name:'actual-dialog-animation-start-middle-end-and-interrupted-dismiss',...motion});
    await page.locator('[data-route="home"]').click();

    const open=page.locator('.updater-check');await open.click();
    await page.waitForSelector('[role=dialog]');
    await page.keyboard.press('Escape');
    await page.evaluate(()=>Object.assign(window.__novaUX.update,{phase:'available',version:'99.0.0-fixture',date:'2026-10-31',notes:'## Updates\n- Real fixture note\n## Known Bugs\n- GPU issue'}));
    await open.click();await page.waitForSelector('[role=dialog]');
    await page.getByRole('button',{name:/Skip this version/i}).click();
    assert.equal(await page.locator('[role=dialog]').count(),0);
    assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(x=>x.command==='application_update_skip').length),1);
    await page.evaluate(()=>{window.__novaUX.update.phase='available';window.__novaUX.handlers.application_update_download=async()=>{throw new Error('Signature verification failed (isolated fixture)');};});
    await open.click();await page.getByRole('button',{name:/Update.*restart|Install.*restart/i}).click();
    await page.getByRole('alert').filter({hasText:'Signature verification failed'}).waitFor();
    assert.equal(await page.evaluate(()=>window.__novaUX.calls.some(x=>x.command==='application_update_install')),false);
    await page.keyboard.press('Escape');
    report.scenarios.push({name:'update-current-skip-and-invalid-signature-retain-app',passed:true,network:'blocked'});
    const deviceMetrics=await context.newCDPSession(page);
    for(const scale of [.5,.75,1,1.25,1.5,2,3,5]) {
      await page.setViewportSize({width:Math.floor(800/scale),height:Math.floor(500/scale)});
      // Match WebView zoom's raster density as well as its logical viewport.
      // This changes browser device emulation, never production DOM or CSS.
      await deviceMetrics.send('Emulation.setDeviceMetricsOverride',{width:Math.floor(800/scale),height:Math.floor(500/scale),deviceScaleFactor:scale,mobile:false});
      await open.scrollIntoViewIfNeeded();await open.click();
      const dialog=page.locator('[role=dialog]');await dialog.waitFor();
      const dimensions=await dialog.evaluate(el=>({rect:el.getBoundingClientRect().toJSON(),scrollHeight:el.scrollHeight,clientHeight:el.clientHeight,overflow:getComputedStyle(el).overflowY,bodyHeight:el.querySelector('.dialog-body')?.clientHeight}));
      assert.ok(dimensions.scrollHeight<=dimensions.clientHeight+2 || /auto|scroll/.test(dimensions.overflow),`dialog content clipped at ${scale}`);
      const cancel=dialog.getByRole('button',{name:'Cancel',exact:true});await cancel.scrollIntoViewIfNeeded();
      const reachable=await cancel.evaluate(el=>{const r=el.getBoundingClientRect();return r.top>=0&&r.bottom<=innerHeight+1;});
      assert.ok(reachable,`dialog cancel inaccessible at ${scale}: ${JSON.stringify(dimensions)}`);
      await page.screenshot({path:path.join(outputDir,`shell-zoom-${scale}.png`)});
      await cancel.click();report.scenarios.push({name:'dialog-scale',scale,dimensions});
    }
    await deviceMetrics.detach();
    await page.setViewportSize({width:800,height:500});
    await page.evaluate(()=>{
      window.__novaUX.handlers.application_confirm_close=async()=>undefined;
      window.__novaUX.emit('nova-close-requested',{requestId:701});
    });
    await page.waitForFunction(()=>window.__novaUX.calls.some(x=>x.command==='application_confirm_close'&&x.args.requestId===701&&x.args.draftsPreserved===true));
    assert.ok(await page.evaluate(()=>JSON.parse(localStorage.getItem('nova.update-recovery.v1')).shell));
    await page.evaluate(()=>{
      const original=Storage.prototype.setItem;
      window.__restoreUXStorage=()=>{Storage.prototype.setItem=original;};
      Storage.prototype.setItem=function(key,value){if(key==='nova.update-recovery.v1')throw new DOMException('Isolated storage quota fixture','QuotaExceededError');return original.call(this,key,value);};
      window.__novaUX.emit('nova-close-requested',{requestId:702});
    });
    await page.getByRole('alert').filter({hasText:'Drafts could not be retained'}).waitFor();
    assert.equal(await page.evaluate(()=>window.__novaUX.calls.some(x=>x.command==='application_confirm_close'&&x.args.requestId===702&&x.args.draftsPreserved===true)),false);
    await page.evaluate(()=>{
      window.__restoreUXStorage();delete window.__restoreUXStorage;
      window.__novaUX.handlers.application_request_close=async()=>{window.__novaUX.emit('nova-close-requested',{requestId:703});};
    });
    await page.getByRole('button',{name:/Retry.*clos/i}).click();
    await page.waitForFunction(()=>window.__novaUX.calls.some(x=>x.command==='application_confirm_close'&&x.args.requestId===703&&x.args.draftsPreserved===true));
    assert.equal(await page.getByRole('alert').filter({hasText:'Drafts could not be retained'}).count(),0);
    report.scenarios.push({name:'close-draft-preservation-storage-failure-and-user-retry',passed:true,nativeWindow:'IO boundary only; no desktop window closed'});
  } catch(error) {report.failures.push({error:error.stack});await page.screenshot({path:path.join(outputDir,'shell-boundaries-failure.png')}).catch(()=>{});}
  await context.close();fs.writeFileSync(path.join(outputDir,'shell-boundaries.json'),JSON.stringify(report,null,2));return report;
}

export async function runUxAudit({browser,baseURL,outputDir}){
  fs.mkdirSync(outputDir,{recursive:true});const report={started:new Date().toISOString(),nativeGui:false,reports:[]};
  const only=process.env.NOVA_UX_DOMAIN;
  if(!only||only==='shell-boundaries')report.reports.push(await auditShellBoundaries({browser,baseURL,outputDir,installBoundary}));
  if(!only||only==='shell')report.reports.push(await auditShell({browser,baseURL,outputDir,installBoundary}));
  for(const [domain,file,name] of [['library','ux-library-stockpile.mjs','auditLibraryStockpile'],['editors','ux-editors-settings.mjs','auditEditorsSettings'],['native','ux-native.mjs','auditNative']]){
    if(only&&only!==domain)continue;
    try{const m=await import('./'+file);report.reports.push(await m[name]({browser,baseURL,outputDir,installBoundary}));}catch(e){report.reports.push({domain,error:e.stack});}
  }
  fs.writeFileSync(path.join(outputDir,'ux-results.json'),JSON.stringify(report,null,2));
  console.log('Production UX results: '+outputDir);if(report.reports.some(r=>r?.error||r?.failures?.length))process.exitCode=1;
}

