import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';

const sourcePath = 'X:\\Nova-UX-isolated\\Jade-HUD.litematic';
const stableRect = async page => page.locator('.render-preview-host').evaluate(el => {
  const {x,y,width,height}=el.getBoundingClientRect();return {x,y,width,height};
});
const equalRect = (a,b) => { for(const key of ['x','y','width','height'])assert.ok(Math.abs(a[key]-b[key])<=1,`viewport ${key} moved: ${a[key]} -> ${b[key]}`); };

// Only the native IO is controlled. Production routes/components/CSS, Dialog,
// Popover, inventory handlers, focus and lifecycle subscriptions remain real.
async function installNativeFixture(context, installBoundary, {theme,locale,filePath=sourcePath}) {
  const icons=Object.fromEntries(['chest','oak_log','diamond','iron_ingot'].map(id=>[id,'data:image/png;base64,'+fs.readFileSync(path.resolve('pack-in/item-icons-26.3',id+'.png')).toString('base64')]));
  await installBoundary(context,{route:'render',theme,locale,currentFile:filePath,config:{motion_preference:'off'}});
  await context.addInitScript(({sourcePath,icons,locale})=>{
    const host=window.__novaUX;
    const state=host.scene={dataVersion:4189,gameVersion:'1.21.4',contentId:'ux-fixture-resources',sceneGeneration:'ux-fixture-scene',revision:0,mode:'observe',menuOpen:false,activeSlot:0,hotbar:Array.from({length:9},(_,i)=>i<2?{id:i?'minecraft:chest':'minecraft:oak_log',properties:{}}:null),dirty:false,canUndo:false,canRedo:false,pointer:[0.28,0.27],pointerLocked:false,hit:{displayName:locale==='en-US'?'Oak Log':'橡木原木',sourceName:'Minecraft',instanceId:'fixture',position:[12,64,-8],face:'up',distance:3.4,state:{id:'minecraft:oak_log',properties:{axis:'y'}},container:null},error:null};
    const operations=host.operations=['standard','full'].map(displayMode=>({operationId:'ux-fixture-'+displayMode,source:{path:sourcePath},displayMode,buildMode:'realtime',state:'viewer_prepared',progress:{phase:'viewer_prepared',activity:null,completed:1,total:1,elapsedMs:0},presentation:{mode:'embedded',visible:true,placement:{x:200,y:260,width:1080,height:460,dpiScale:1,focused:false}},presentationState:'visible',analysis:null,error:null}));
    operations.push({...structuredClone(operations[0]),operationId:'ux-layer-standard',layerView:true});
    const layerScene={sceneHandle:'ux-layer-scene',generation:1,sourceIdentity:'ux-source',resourceIdentity:'ux-fixture-resources',visualGeneration:1,origin:null,meta:{size_x:32,size_y:9,size_z:32,min_x:0,min_y:-4,min_z:0,minecraft_data_version:4189,version:'1.21.4',name:'UX layer IO fixture'}};
    host.handlers.application_read_layer_bounds=()=>[-4,4];
    host.handlers.application_read_project_markers=()=>({origin:null});
    host.handlers.application_data_query=({input})=>{
      if(input.kind==='format')return{nativePassthrough:true};
      if(input.kind!=='layerView')throw new Error('Unexpected native fixture query '+input.kind);
      const q=input.input;
      if(q.action==='openScene'||q.action==='getLayerMeta')return structuredClone(layerScene);
      if(q.action==='requestLayerView')return {...layerScene,requestId:q.requestId,y:q.y,worldY:q.y-4,viewRect:q.viewRect,imageUrl:icons.oak_log,pixelWidth:64,pixelHeight:64,blockCount:1,missingVisuals:0,visualDiagnostics:{readyStates:1,transparentStates:0,missingStates:0,failedStates:0,retryableStates:0,issues:[]}};
      if(q.action==='cancel'||q.action==='release')return null;
      if(q.action==='hitTest')return{sceneHandle:layerScene.sceneHandle,generation:1,requestId:q.requestId,hit:null};
      throw new Error('Unexpected layer action '+q.action);
    };
    const view={worldTime:6000,worldTimeLabel:'12:00',weather:'clear'};
    const ack=displayMode=>({sourcePath,displayMode,cameraLocked:state.pointerLocked,interaction:structuredClone(state),...view});
    host.handlers.open_iris_options_window=()=>null;
    host.handlers.native_preview_set_background=()=>null;
    host.handlers.application_quick_preview_snapshot=()=>null;
    host.handlers.render_preview_image=({displayMode})=>({preview_path:'X:\\Nova-UX-isolated\\preview.png',data_url:icons.chest,draft_id:'ux-native-preview',can_embed:true,persisted:false,display_mode:displayMode});
    host.handlers.discard_generated_preview=()=>null;
    host.handlers.save_generated_preview=()=>{throw new Error('Isolated source is read-only');};
    host.handlers.application_render_get_snapshot=()=>({operations:structuredClone(operations)});
    // Do not synthesize visibility events: the command reply is not the runtime event.
    // This fixture tests that surface changes do not issue hidden intents at all.
    host.handlers.application_render_update_presentation=({operationId,intent})=>({...structuredClone(operations.find(op=>op.operationId===operationId)),presentation:intent});
    host.handlers.application_get_scene_view=({displayMode})=>ack(displayMode);
    host.handlers.application_set_scene_view=({displayMode,controls})=>{
      if(host.failEnvironment&&controls.weather){host.failEnvironment=false;throw new Error('Isolated environment receipt failed');}
      const i=controls?.interaction;
      if(i){if(i.mode)state.mode=i.mode;if(i.menuOpen!==undefined)state.menuOpen=i.menuOpen;if(i.activeSlot!==undefined)state.activeSlot=i.activeSlot;if(i.setSlot)state.hotbar[i.setSlot.index]=i.setSlot.state;}
      if(controls.worldTime!==undefined){view.worldTime=controls.worldTime;const m=(controls.worldTime%24000)*60/1000+360;view.worldTimeLabel=String(Math.floor(m/60)%24).padStart(2,'0')+':'+String(Math.floor(m)%60).padStart(2,'0');}
      if(controls.weather)view.weather=controls.weather;
      if(controls.releaseCameraInput)state.pointerLocked=false;
      return ack(displayMode);
    };
    host.handlers.read_game_data_icon=({id})=>icons[id.replace('minecraft:','')]??null;
    host.handlers.game_data_creative_inventory=({input})=>{
      const base=[{id:'minecraft:oak_log',blockState:'minecraft:oak_log[axis=y]',state:{id:'minecraft:oak_log',properties:{axis:'y'}},itemId:'minecraft:oak_log',name:locale==='en-US'?'Oak Log':'橡木原木',sourceName:'Minecraft',category:'building_blocks'},{id:'minecraft:chest',blockState:'minecraft:chest[facing=north,type=single,waterlogged=false]',state:{id:'minecraft:chest',properties:{facing:'north',type:'single',waterlogged:'false'}},itemId:'minecraft:chest',name:locale==='en-US'?'Chest':'箱子',sourceName:'Minecraft',category:'building_blocks'}];
      // Page-size fixture only. Repeated known states avoid inventing resource IDs.
      const all=Array.from({length:54},(_,i)=>base[i%2]).filter(item=>!input.query||item.name.includes(input.query)||item.id.includes(input.query));
      return {contentId:state.contentId,version:state.gameVersion,offset:input.offset||0,total:all.length,categories:[{id:'building_blocks',label:locale==='en-US'?'Building Blocks':'建筑方块',count:54}],items:all.slice(input.offset||0,(input.offset||0)+(input.limit||45))};
    };
  },{sourcePath:filePath,icons,locale});
}

// The renderer/IO replies below are fixtures. The inspected dialog, shared
// modal registration, route handlers and CSS are the shipped production bundle.
async function auditThumbnail({browser,baseURL,outputDir,installBoundary,report}) {
  const filePath='X:\\Nova-UX-isolated\\'+('long-path-'.repeat(12))+'\\Jade.litematic';
  const context=await browser.newContext({viewport:{width:1000,height:720},locale:'en-US',deviceScaleFactor:1,colorScheme:'dark'});
  await installNativeFixture(context,installBoundary,{theme:'shadcn',locale:'en-US',filePath});
  await context.addInitScript(({filePath})=>{
    const io=window.__novaUX; io.previewSerial=0;io.discarded=[];io.persisted=[];io.holdCapture=false;io.holdSave=false;io.failSave=false;
    io.quick={previewId:'thumbnail-existing-fast',sourcePath:filePath,renderMode:'fast_experimental',state:'ready',presentation:{mode:'embedded',visible:true,placement:{x:0,y:0,width:640,height:360,dpiScale:1,focused:false}},presentationRevision:1,visible:true,error:null};
    io.handlers.application_quick_preview_snapshot=()=>structuredClone(io.quick);
    io.handlers.application_quick_preview_start=({input})=>{if(input.sourcePath!==io.quick.sourcePath)throw new Error('Unexpected new Fast scene');return structuredClone(io.quick);};
    io.handlers.application_quick_preview_present=({intent,revision})=>{io.quick={...io.quick,presentation:intent,presentationRevision:revision,visible:intent.visible};return structuredClone(io.quick);};
    const prior=io.handlers.render_preview_image;
    io.handlers.render_preview_image=args=>{
      const image={...prior(args),draft_id:'thumbnail-draft-'+(++io.previewSerial)};
      io.lastDraft=image;return io.holdCapture?new Promise(resolve=>{io.finishCapture=()=>resolve(image);}):image;
    };
    io.handlers.discard_generated_preview=({draftId})=>{io.discarded.push(draftId);};
    io.handlers.save_generated_preview=({filePath,draftId})=>{
      if(io.failSave){io.failSave=false;throw new Error('Isolated recoverable save failure');}
      const saved={...io.lastDraft,draft_id:null,persisted:true};const finish=()=>{io.persisted.push({filePath,draftId});return saved;};
      return io.holdSave?new Promise(resolve=>{io.finishSave=()=>resolve(finish());}):finish();
    };
    io.handlers.application_inspect_project=()=>({metadata:{name:'Jade',minecraft_data_version:4189},regions:[]});
    io.handlers.application_read_embedded_preview=()=>null;
    const data=io.handlers.application_data_query;
    io.handlers.application_data_query=({input})=>input.kind==='analysis'?{metadata:{name:'Jade',minecraft_data_version:4189},regions:[]}:input.kind==='thumbnail'?input.input.sourcePaths.map(sourcePath=>({sourcePath,image:null})):data({input});
  },{filePath});
  const page=await context.newPage();page.setDefaultTimeout(8000);const errors=[];page.on('pageerror',e=>errors.push(e.message));
  const dialog=page.locator('[role=dialog]').filter({has:page.locator('.preview-draft-image')});
  const commands=()=>page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_set_scene_view'&&Object.hasOwn(c.args.controls,'uiRegions')));
  const settle=()=>page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
  async function openDraft(){
    const selector=page.locator('.render-secondary-action').filter({hasText:'Set as Thumbnail'});
    if(await selector.isVisible())await selector.click();
    else {await page.locator('.render-overflow-trigger').click();await page.getByRole('menuitem',{name:'Set as Thumbnail',exact:true}).click();}
  }
  async function assertModal(label,hostSelector='.render-preview-host',expectedRect){
    const hostRect=()=>page.locator(hostSelector).evaluate(el=>{const {x,y,width,height}=el.getBoundingClientRect();return {x,y,width,height};});
    await dialog.waitFor();
    await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_set_scene_view'&&c.args.controls.uiInputBlocked===true));
    await settle();
    const before=await hostRect();if(expectedRect)equalRect(expectedRect,before);const trace=await commands();const last=trace.at(-1);
    assert.ok(last?.args.controls.uiRegions.length>0,label+' has nonempty native cutouts');
    const block=await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_set_scene_view'&&typeof c.args.controls.uiInputBlocked==='boolean').at(-1)?.args.controls.uiInputBlocked);
    assert.equal(block,true,label+' stays input-blocked');
    for(const key of ['Tab','Shift+Tab','Tab']){await page.keyboard.press(key);assert.equal(await dialog.evaluate(e=>e.contains(document.activeElement)),true,label+' focus trap');}
    const nativePointerBefore=await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_set_scene_view'&&c.args.controls.uiPointer).length);
    await dialog.locator('img').click();await page.keyboard.press('w');await page.mouse.wheel(0,90);
    assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_set_scene_view'&&c.args.controls.uiPointer).length),nativePointerBefore,'dialog input is not forwarded');
    equalRect(before,await hostRect());
    report.scenarios.push({name:label,lastSetView:last,trace,nativeInputBlocked:block,presentation:await page.evaluate(()=>({quick:window.__novaUX.quick,operations:window.__novaUX.operations}))});
  }
  try {
    await page.goto(baseURL,{waitUntil:'networkidle'});await page.waitForSelector('.render-preview-host');
    const forbidden=['application_quick_preview_start','application_render_launch_plan','application_render_start','application_render_close','application_quick_preview_close'];
    const countWork=()=>page.evaluate(names=>window.__novaUX.calls.filter(c=>names.includes(c.command)).length,forbidden);
    const baselineWork=await countWork();
    for(const [index,mode] of ['fast_experimental','standard','full'].entries()){
      if(index)await page.locator('.render-mode-switch button').nth(index).click();
      if(index)await page.waitForSelector('.scene-editor-actions');
      await page.evaluate(()=>window.__novaUX.holdCapture=true);await openDraft();await settle();
      const originalRect=await stableRect(page);
      await page.evaluate(()=>{window.__novaUX.holdCapture=false;window.__novaUX.finishCapture();});
      await assertModal('draft-'+mode,'.render-preview-host',originalRect);
      if(index===0){
        await page.evaluate(()=>window.__novaUX.failSave=true);
        await dialog.getByRole('button',{name:'Save to schematic',exact:true}).click();
        await dialog.getByRole('alert').waitFor();
        assert.equal(await page.evaluate(()=>window.__novaUX.discarded.length),0,'save error preserves draft');
        await dialog.getByRole('button',{name:'Save to schematic',exact:true}).click();
      }else if(index===1)await dialog.getByRole('button',{name:'Discard',exact:true}).click();
      else await page.keyboard.press('Escape');
      await dialog.waitFor({state:'detached'});await settle();
      assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_set_scene_view'&&typeof c.args.controls.uiInputBlocked==='boolean').at(-1)?.args.controls.uiInputBlocked),false,'final modal releases only the input lease');
      if(index!==0)equalRect(originalRect,await stableRect(page)); // A successful save publishes the existing status banner.
      report.scenarios.push({name:'closed-'+mode,lastSetView:(await commands()).at(-1)});
    }
    assert.equal(await countWork(),baselineWork,'opening/closing drafts does not build/restart viewers');
    // Open an existing production inventory dialog while the capture is pending.
    await page.evaluate(()=>window.__novaUX.holdCapture=true);await openDraft();
    await page.locator('.scene-editor-actions button').nth(2).click();await page.waitForSelector('.flake-page__inventory-dialog');
    await page.evaluate(()=>{window.__novaUX.holdCapture=false;window.__novaUX.finishCapture();});
    await assertModal('nested-draft-over-inventory');await page.keyboard.press('Escape');await dialog.waitFor({state:'detached'});
    await settle();assert.equal(await page.locator('.flake-page__inventory-dialog').isVisible(),true);
    assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_set_scene_view'&&typeof c.args.controls.uiInputBlocked==='boolean').at(-1)?.args.controls.uiInputBlocked),true,'closing one modal does not unblock remaining inventory');
    await page.keyboard.press('Escape');await page.waitForSelector('.flake-page__inventory-dialog',{state:'detached'});
    // Device metrics reproduce WebView zoom's logical viewport and raster density.
    await openDraft();const cdp=await context.newCDPSession(page);
    for(const scale of [0.5,5]){
      const width=Math.floor(1600/scale),height=Math.floor(1000/scale);
      await page.setViewportSize({width,height});await cdp.send('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:scale,mobile:false});
      await settle();const box=await dialog.evaluate(el=>({rect:el.getBoundingClientRect().toJSON(),scroll:el.scrollHeight,client:el.clientHeight}));
      assert.ok(box.rect.width<=width+1&&box.rect.height<=height+1,'dialog fits at '+scale);
      for(const name of ['Discard','Save to schematic']){const button=dialog.getByRole('button',{name,exact:true});await button.scrollIntoViewIfNeeded();assert.ok(await button.evaluate(el=>{const b=el.getBoundingClientRect();return b.top>=0&&b.bottom<=innerHeight+1;}),'reachable '+name+' at '+scale);}
      await page.screenshot({path:path.join(outputDir,'thumbnail-ui-zoom-'+scale+'.png')});report.scenarios.push({name:'scale',scale,box,lastSetView:(await commands()).at(-1)});
    }
    await cdp.detach();await page.setViewportSize({width:1000,height:720});await page.keyboard.press('Escape');await dialog.waitFor({state:'detached'});
    const switchSource=async value=>{
      await page.evaluate(path=>window.__novaUX.emit('projection-library-record-imported',{path}),value);
      await page.waitForFunction(path=>document.querySelector('.render-page-source')?.title===path,value);
    };
    const other=filePath.replace('Jade.litematic','Other.litematic');
    await page.evaluate(()=>window.__novaUX.holdCapture=true);await openDraft();
    const staleId=await page.evaluate(()=>window.__novaUX.lastDraft.draft_id);
    await switchSource(other);await switchSource(filePath);await page.evaluate(()=>{window.__novaUX.holdCapture=false;window.__novaUX.finishCapture();});
    await page.waitForFunction(id=>window.__novaUX.discarded.includes(id),staleId);
    assert.equal(await page.locator('.preview-draft-image').count(),0,'A-B-A never accepts the old readback');
    report.scenarios.push({name:'source-A-B-A',discarded:staleId});
    await openDraft();await dialog.waitFor();const savingId=await page.evaluate(()=>{window.__novaUX.holdSave=true;return window.__novaUX.lastDraft.draft_id;});
    await dialog.getByRole('button',{name:'Save to schematic',exact:true}).click();
    await page.waitForFunction(()=>typeof window.__novaUX.finishSave==='function');
    await switchSource(other);await switchSource(filePath);
    assert.equal(await page.evaluate(id=>window.__novaUX.discarded.includes(id),savingId),false,'do not delete a frame while Rust owns the save');
    await openDraft();await dialog.waitFor();const newerId=await page.evaluate(()=>window.__novaUX.lastDraft.draft_id);
    await page.evaluate(()=>{window.__novaUX.holdSave=false;window.__novaUX.finishSave();});
    await page.waitForFunction(id=>window.__novaUX.persisted.some(p=>p.draftId===id),savingId);
    await settle();assert.ok(await dialog.isVisible(),'late saved receipt must not close the newer draft');
    await dialog.getByRole('button',{name:'Discard',exact:true}).click();await dialog.waitFor({state:'detached'});
    assert.equal(await page.evaluate(id=>window.__novaUX.discarded.includes(id),newerId),true);
    report.scenarios.push({name:'save-then-source-change-retains-frame-until-receipt',savingId,newerId});
    // Leaving while a readback is pending discards its late result without UI resurrection.
    await page.evaluate(()=>window.__novaUX.holdCapture=true);await openDraft();
    await page.locator('[data-route="home"]').click();await page.waitForSelector('.render-page',{state:'detached'});
    await page.evaluate(()=>window.__novaUX.finishCapture());
    await page.waitForFunction(()=>window.__novaUX.discarded.includes(window.__novaUX.lastDraft.draft_id));
    assert.equal(await page.locator('.preview-draft-image').count(),0);
    report.scenarios.push({name:'late-capture-after-route-leave',discarded:await page.evaluate(()=>window.__novaUX.discarded)});
    // User-reported entry: the real PropertiesPage uses the same existing Fast
    // session, then capture -> confirmation -> discard and confirmed save.
    await page.evaluate(()=>window.__novaUX.holdCapture=false);
    await page.locator('[data-route="properties"]').click();await page.waitForSelector('.properties-preview-host');
    await page.getByRole('button',{name:'Show in Page',exact:true}).click();
    const propertyBefore=await page.evaluate(()=>({id:window.__novaUX.quick.previewId,state:window.__novaUX.quick.state,presentation:window.__novaUX.quick.presentation}));
    assert.equal(propertyBefore.state,'ready');assert.equal(propertyBefore.presentation.mode,'embedded');assert.equal(propertyBefore.presentation.visible,true);
    const propertyWork=await countWork();
    const propertyRect=()=>page.locator('.properties-preview-host').evaluate(el=>{const {x,y,width,height}=el.getBoundingClientRect();return {x,y,width,height};});
    const propertyResults=[];
    for(const action of ['Discard','Save to schematic']){
      await page.evaluate(()=>window.__novaUX.holdCapture=true);
      await page.getByRole('button',{name:'Set as Thumbnail',exact:true}).click();await settle();
      const originalRect=await propertyRect();
      await page.evaluate(()=>{window.__novaUX.holdCapture=false;window.__novaUX.finishCapture();});
      await assertModal('properties-fast-'+action,'.properties-preview-host',originalRect);
      if(action==='Save to schematic')await page.screenshot({path:path.join(outputDir,'properties-thumbnail-confirm.png')});
      await dialog.getByRole('button',{name:action,exact:true}).click();await dialog.waitFor({state:'detached'});await settle();
      const afterRect=await propertyRect();
      if(action==='Discard')equalRect(originalRect,afterRect);
      // A confirmed save publishes the preexisting success message above this
      // layout. That normal content reflow is distinct from modal occlusion.
      else await page.getByText('Thumbnail saved in the schematic file.',{exact:true}).waitFor();
      propertyResults.push({action,originalRect,afterRect});
    }
    assert.equal(await countWork(),propertyWork,'Properties captures never rebuild or close the existing Viewer');
    const propertyAfter=await page.evaluate(()=>({id:window.__novaUX.quick.previewId,state:window.__novaUX.quick.state,presentation:window.__novaUX.quick.presentation}));
    assert.deepEqual({id:propertyAfter.id,state:propertyAfter.state,mode:propertyAfter.presentation.mode,visible:propertyAfter.presentation.visible},{id:propertyBefore.id,state:propertyBefore.state,mode:propertyBefore.presentation.mode,visible:propertyBefore.presentation.visible},'Properties preserves scene/session and presentation intent');
    report.scenarios.push({name:'properties-existing-Fast-discard-and-save',source:filePath,before:propertyBefore,after:propertyAfter,geometry:propertyResults,lastSetView:(await commands()).at(-1),persisted:await page.evaluate(()=>window.__novaUX.persisted)});
    assert.deepEqual(errors,[]);
  }catch(error){report.failures.push({name:'thumbnail',error:error.stack,errors,body:await page.locator('body').innerText(),lastSetView:(await commands()).at(-1)});}
  finally {await context.close();}
  fs.writeFileSync(path.join(outputDir,'thumbnail.json'),JSON.stringify(report,null,2));return report;
}

export async function auditNative({browser,baseURL,outputDir,installBoundary}) {
  const report={domain:'native',kind:'production-RenderPage; native IO fixture explicitly isolated',nativeViewerValidated:false,visibleGuiOperated:false,scenarios:[],failures:[]};
  if(process.env.NOVA_UX_NATIVE_PART==='thumbnail')return auditThumbnail({browser,baseURL,outputDir,installBoundary,report});
  if(process.env.NOVA_UX_NATIVE_PART==='layers')return auditNativeLayerInset({browser,baseURL,outputDir,installBoundary,report});
  if(!['iris','preflight'].includes(process.env.NOVA_UX_NATIVE_PART))for(const [index,theme] of ['shadcn','8bitcn','Minecraft','Aero7','Metro10'].entries())for(const locale of ['zh-CN','en-US']){
    const viewport=index%2?{width:856,height:505}:{width:1280,height:720};
    const context=await browser.newContext({viewport,locale,deviceScaleFactor:index%2?1.25:1,colorScheme:'dark'});
    await installNativeFixture(context,installBoundary,{theme,locale});
    const page=await context.newPage();page.setDefaultTimeout(8000);const errors=[];page.on('pageerror',error=>errors.push(error.message));
    const result={theme,locale,viewport,states:[],errors};
    try{
      await page.goto(baseURL,{waitUntil:'networkidle'});await page.waitForSelector('.render-page');
      await page.locator('.render-mode-switch button').nth(1).click();await page.waitForSelector('.scene-info-hud');await page.evaluate(()=>document.fonts.ready);
      const before=await stableRect(page);assert.ok(before.width>100&&before.height>80);
      const hud=await page.locator('.scene-info-hud').evaluate(el=>el.getBoundingClientRect().toJSON());
      assert.ok(hud.x>=before.x&&hud.right<=before.x+before.width+1);assert.ok(hud.y>=before.y&&hud.bottom<=before.y+before.height+1);
      assert.equal(await page.locator('.scene-editor-dock button').count(),9);assert.equal(await page.locator('.scene-editor-dock .scene-editor-actions').count(),0);
      if(!process.env.NOVA_UX_NATIVE_FIX_BASELINE){
        const icon=await page.locator('.scene-hotbar-slot .block-icon-image').first().boundingBox();assert.ok(icon.width>=18&&icon.height>=18,'theme retains readable hotbar item size');
        const actionRow=await page.locator('.render-page-actions').first().evaluate(e=>({scroll:e.scrollWidth,client:e.clientWidth}));assert.ok(actionRow.scroll<=actionRow.client+1,'render primary toolbar fits; secondary actions use more menu');
      }
      result.states.push({state:'drag-pointer-hud',viewport:before,hud});
      await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-drag.png`)});
      await page.evaluate(()=>{window.__novaUX.scene.pointerLocked=true;window.__novaUX.scene.pointer=[0.5,0.5];});
      await page.waitForFunction(()=>!document.querySelector('.scene-info-hud')?.classList.contains('scene-info-hud--pointer'));
      const locked=await page.locator('.scene-info-hud').evaluate(el=>el.getBoundingClientRect().toJSON());
      assert.ok(Math.abs(locked.x+locked.width/2-(before.x+before.width/2))<2);result.states.push({state:'lock-center-hud',hud:locked});
      await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-locked.png`)});
      await page.locator('.scene-editor-actions button').nth(1).click();await page.waitForFunction(()=>window.__novaUX.scene.mode==='edit');
      await page.locator('.scene-hotbar-slot').nth(4).click();await page.waitForFunction(()=>window.__novaUX.scene.activeSlot===4);
      if(!process.env.NOVA_UX_NATIVE_FIX_BASELINE){
      const pointerCalls=await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_set_scene_view'&&c.args.controls?.uiPointer).length);
      await page.locator('.scene-info-hud').dispatchEvent('mousedown',{button:0,clientX:300,clientY:300});
      await page.locator('.scene-info-hud').dispatchEvent('mouseup',{button:0,clientX:300,clientY:300});
      await page.waitForTimeout(60);
      assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_set_scene_view'&&c.args.controls?.uiPointer).length),pointerCalls,'HUD clicks never forward into native edit');
      result.states.push({state:'edit-HUD-pointer-no-native-pass-through'});
      }else result.states.push({state:'edit-HUD-pointer-no-native-pass-through',status:'pending-new-bundle-6c62a180'});
      await page.locator('.scene-editor-actions button').nth(0).click();await page.waitForFunction(()=>window.__novaUX.scene.mode==='observe');
      if(index<2){
        await page.evaluate(()=>{const s=window.__novaUX.scene;s.pointerLocked=false;s.pointer=[0.98,0.94];s.hit.state.id='minecraft:chest';s.hit.displayName='Large container / 大型容器 — long label alignment';s.hit.container={status:'known',capacity:54,items:[{slot:0,id:'minecraft:diamond',displayName:'Diamond',count:64},{slot:53,id:'minecraft:iron_ingot',displayName:'Iron Ingot',count:12,variant:'Custom name / 自定义名称'}]};});
        await page.waitForSelector('.scene-hud-slot');const ch=await page.locator('.scene-info-hud').evaluate(e=>e.getBoundingClientRect().toJSON());assert.ok(ch.right<=before.x+before.width+1&&ch.bottom<=before.y+before.height+1,'container HUD edge clamp');
        await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-container.png`)});
        await page.locator('.scene-hud-details').click();await page.waitForSelector('.scene-hud-expanded');assert.equal(await page.locator('.scene-hud-expanded .scene-hud-slot').count(),54);equalRect(before,await stableRect(page));
        await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-container-details.png`)});await page.keyboard.press('Escape');
        await page.evaluate(()=>window.__novaUX.scene.hit.container={status:'known',capacity:27,items:[]});await page.waitForSelector('.scene-hud-empty');
        await page.evaluate(()=>window.__novaUX.scene.hit.container={status:'unknown',capacity:27,items:[],error:'Missing container metadata'});await page.waitForFunction(()=>document.querySelector('.scene-hud-empty')?.title==='Missing container metadata');
        await page.evaluate(()=>{const s=window.__novaUX.scene;s.hit.container=null;s.hit.state.id='minecraft:oak_log';s.hit.displayName='Oak Log';s.pointer=[0.28,0.27];});
        result.states.push({state:'container-known54/empty/unknown/details/edge-clamp',bounds:ch});
      }
      const beforeInventory=await page.evaluate(()=>window.__novaUX.calls.length);
      await page.locator('.scene-editor-actions button').nth(2).click();await page.waitForSelector('.flake-page__inventory-dialog');
      await page.locator('.flake-page__creative-inventory-grid button').first().waitFor();
      await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_set_scene_view'&&c.args.controls.uiInputBlocked===true));
      equalRect(before,await stableRect(page));
      const dialog=await page.locator('.flake-page__inventory-dialog').evaluate(el=>({rect:el.getBoundingClientRect().toJSON(),scroll:el.scrollHeight,client:el.clientHeight}));
      assert.ok(dialog.rect.width<=viewport.width&&dialog.rect.height<=viewport.height);
      await page.locator('.flake-page__creative-inventory-range button').last().click();await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='game_data_creative_inventory'&&c.args.input.offset===45));
      await page.waitForFunction(()=>document.querySelector('.flake-page__creative-inventory-grid button:not(:disabled)'));
      await page.locator('.flake-page__creative-inventory-grid button').first().press('PageUp');await page.waitForFunction(()=>window.__novaUX.calls.filter(c=>c.command==='game_data_creative_inventory').at(-1)?.args.input.offset===0);
      await page.waitForFunction(()=>document.querySelector('.flake-page__creative-inventory-grid button:not(:disabled)'));
      const item=page.locator('.flake-page__creative-inventory-grid button').first();await item.hover();await item.focus();await page.keyboard.press('3');
      await page.waitForFunction(()=>window.__novaUX.scene.hotbar[2]?.properties?.axis==='y');
      await page.locator('.flake-page__creative-hotbar-slot').nth(2).click({button:'right'});
      await page.waitForFunction(()=>window.__novaUX.scene.hotbar[2]===null);
      await item.dragTo(page.locator('.flake-page__creative-hotbar-slot').nth(8));await page.waitForFunction(()=>window.__novaUX.scene.hotbar[8]?.properties?.axis==='y');
      const search=page.locator('.flake-page__creative-inventory-search input');await search.fill('oak');await page.waitForTimeout(160);
      assert.ok(await page.evaluate(()=>window.__novaUX.calls.some(c=>c.command==='game_data_creative_inventory'&&c.args.input.query==='oak')));
      await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-inventory.png`)});
      if(!process.env.NOVA_UX_NATIVE_FIX_BASELINE){const icon=await item.locator('.block-icon-image').boundingBox();assert.ok(icon.width>=20&&icon.height>=20,'theme retains readable inventory item size');}
      // Escape closes the single production Dialog even while its search owns focus.
      await page.keyboard.press('Escape');await page.waitForSelector('.flake-page__inventory-dialog',{state:'detached'});
      equalRect(before,await stableRect(page));result.states.push({state:'inventory-slot/search/escape',dialog});
      assert.equal(await page.evaluate(from=>window.__novaUX.calls.slice(from).filter(c=>c.command==='application_render_update_presentation'&&c.args.intent?.visible===false).length,beforeInventory),0);
      const baseline=await page.evaluate(()=>window.__novaUX.calls.length);
      await page.locator('.scene-editor-actions button').nth(3).click();await page.waitForSelector('.scene-edit-menu');equalRect(before,await stableRect(page));
      assert.equal(await page.locator('.scene-edit-menu [role=menuitem]').count(),4);
      await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-edit-menu.png`)});
      await page.locator('.scene-edit-menu [role=menuitem]').last().click();
      await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command.startsWith('plugin:dialog|save')));
      assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_set_scene_view'&&c.args.controls.interaction?.command?.kind==='save').length),0);

      if(index<2){
        if(await page.locator('.render-overflow-trigger').isVisible()){await page.locator('.render-overflow-trigger').click();await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-view-menu.png`)});await page.locator('.scene-edit-menu [role=menuitem]').last().click();}
        else await page.locator('.render-secondary-action').last().click();
        await page.waitForSelector('.preview-draft-image');equalRect(before,await stableRect(page));
        await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-preview-draft.png`)});
        await page.locator('[role=dialog] footer button').last().click();await page.waitForSelector('[role=dialog] [role=alert]');
        await page.keyboard.press('Escape');await page.waitForSelector('.preview-draft-image',{state:'detached'});
        assert.ok(await page.evaluate(()=>window.__novaUX.calls.some(c=>c.command==='discard_generated_preview')));
        result.states.push({state:'thumbnail-draft/read-only-error/discard'});
      }
      const camera=page.locator('.render-view-tools > button').nth(0);await camera.click();await page.waitForSelector('.free-camera-group');
      equalRect(before,await stableRect(page));await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-camera.png`)});
      await page.locator('.free-camera-controls select').first().selectOption('32');await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_set_scene_view'&&c.args.controls.cameraSpeed===32));
      await page.locator('.free-camera-controls select').last().selectOption('1.5');await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_set_scene_view'&&c.args.controls.cameraSensitivity===1.5));
      await page.locator('.free-camera-controls button').last().click();await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_set_scene_view'&&c.args.controls.resetCamera===true));await page.keyboard.press('Escape');
      await page.locator('.render-mode-switch button').nth(2).click();await page.waitForSelector('.scene-info-hud');
      const fullRect=await stableRect(page);const afterMode=await page.evaluate(()=>window.__novaUX.calls.length);
      await page.locator('.render-view-tools > button').nth(1).click();await page.waitForSelector('.render-environment-time input');equalRect(fullRect,await stableRect(page));
      const slider=page.locator('.render-environment-time input');assert.equal(await slider.isDisabled(),false);await slider.fill('12000');await slider.dispatchEvent('input');await slider.dispatchEvent('change');
      await page.locator('.render-environment-weather select').selectOption('rain');await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_set_scene_view'&&c.args.controls.weather==='rain'));
      await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-environment.png`)});
      if(index===0){await page.evaluate(()=>window.__novaUX.failEnvironment=true);await page.locator('.render-environment-weather select').selectOption('snow');await page.waitForSelector('.render-environment-panel [role=alert]');assert.equal(await page.locator('.render-environment-weather select').inputValue(),'rain');equalRect(fullRect,await stableRect(page));await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-environment-error.png`)});await page.locator('.render-environment-weather select').selectOption('clear');await page.waitForSelector('.render-environment-panel [role=alert]',{state:'detached'});}
      await page.keyboard.press('Escape');equalRect(fullRect,await stableRect(page));
      const calls=await page.evaluate(start=>window.__novaUX.calls.slice(start),afterMode);
      assert.equal(calls.filter(c=>c.command==='application_render_update_presentation'&&c.args.intent?.visible===false).length,0);
      assert.equal(calls.filter(c=>/application_render_(start|cancel|close)|artifact.*build/.test(c.command)).length,0);
      result.states.push({state:'camera-and-environment-overlays',baseline,afterMode,viewport:fullRect,commands:calls.map(c=>c.command)});
      await page.locator('[data-route="flake"]').click();await page.waitForSelector('.flake-page__viewport');
      await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_data_query'&&c.args.input?.input?.action==='requestLayerView'));
      await page.waitForTimeout(160);
      const layerRect=async()=>page.locator('.flake-page__viewport').evaluate(el=>{const{x,y,width,height}=el.getBoundingClientRect();return{x,y,width,height};});
      const twoD=await layerRect();
      const requests=await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_data_query'&&['openScene','requestLayerView'].includes(c.args.input?.input?.action)).length);
      await page.locator('.flake-page__advanced-controls summary').click();await page.waitForSelector('.flake-page__advanced-content');equalRect(twoD,await layerRect());
      await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-layers-advanced.png`)});
      await page.locator('.flake-page__advanced-controls summary').press('Escape');equalRect(twoD,await layerRect());
      assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_data_query'&&['openScene','requestLayerView'].includes(c.args.input?.input?.action)).length),requests);
      await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-layers-2d.png`)});
      await page.locator('.flake-page__topbar .flake-page__edit-toggle [role=checkbox]').click();await page.waitForSelector('.flake-page__tool-palette');
      const tools=page.locator('.flake-page__tool-palette button:enabled');const toolCount=await tools.count();for(let tool=0;tool<toolCount;tool++)await tools.nth(tool).click();
      if(!process.env.NOVA_UX_NATIVE_FIX_BASELINE){
      await page.locator('.flake-page__advanced-controls summary').click();
      const stateHints=page.locator('.flake-page__advanced-actions [role=checkbox]');const oldHints=await stateHints.getAttribute('aria-checked');
      await stateHints.focus();await page.keyboard.press('Space');assert.notEqual(await stateHints.getAttribute('aria-checked'),oldHints,'Space activates focused advanced checkbox');
      const chosen=await page.locator('.flake-page__tool-palette button[aria-pressed=true]').getAttribute('aria-label');
      await page.locator('.flake-page__advanced-controls summary').press('Escape');
      const chosenTool=page.locator('.flake-page__tool-palette button[aria-pressed=true]');await chosenTool.focus();await page.keyboard.down('Space');
      assert.equal(await page.locator('.flake-page__tool-palette button[aria-pressed=true]').getAttribute('aria-label'),chosen,'focused button Space is not global hand-tool shortcut');await page.keyboard.up('Space');
      result.states.push({state:'2d-Space-control-ownership',selectedTool:chosen});
      }else result.states.push({state:'2d-Space-control-ownership',status:'pending-new-bundle-6c62a180'});
      await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-layers-tools.png`)});
      if(!process.env.NOVA_UX_NATIVE_FIX_BASELINE){
        const lastAction=page.locator('.flake-page__tool-panel-actions button').last();await lastAction.scrollIntoViewIfNeeded();
        assert.ok(await lastAction.evaluate(el=>{const r=el.getBoundingClientRect();return el.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2));}),'2d last action is not covered by quickbar');
        assert.equal(await page.locator('.flake-page__viewport .flake-page__quickbar').count(),1);
        await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-layers-actions.png`)});
      }
      await page.locator('.flake-page__topbar .flake-page__edit-toggle [role=checkbox]').click();await page.waitForSelector('.flake-page__tool-palette',{state:'detached'});
      result.states.push({state:'2d-tools-selected',toolCount});
      const y=page.locator('.flake-page__y-field input');await y.fill('0');await y.dispatchEvent('change');
      await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_data_query'&&c.args.input?.input?.action==='requestLayerView'&&c.args.input.input.y===4));
      await page.locator('.layer3d-view-tabs button').nth(1).click();await page.waitForSelector('.layer3d-viewport');
      const threeD=await page.locator('.layer3d-viewport').evaluate(el=>el.getBoundingClientRect().toJSON());
      const starts=await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_render_start').length);
      const layerControls=page.locator('.layer3d-controls');await layerControls.locator('button').nth(1).click();await layerControls.locator('input').nth(1).fill('2');await layerControls.locator('input').nth(1).dispatchEvent('change');
      await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_set_scene_view'&&c.args.controls.layerMaxY===2));
      await layerControls.locator('button').nth(2).click();await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_set_scene_view'&&c.args.controls.layerView&&c.args.controls.layerMinY===null&&c.args.controls.layerMaxY===null));
      await page.locator('.layer3d-camera-toolbar > button').click();await page.waitForSelector('.free-camera-group');
      equalRect(threeD,await page.locator('.layer3d-viewport').evaluate(el=>el.getBoundingClientRect().toJSON()));
      await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-layers-3d.png`)});await page.keyboard.press('Escape');
      assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_render_start').length),starts);
      result.states.push({state:'2d-advanced-no-reparse/3d-range-all-camera-no-restart',twoD,threeD,requests,starts});
      result.unknown=await page.evaluate(()=>window.__novaUX.unknown);assert.equal(errors.length,0);assert.equal(result.unknown.length,0);
    }catch(error){result.failure=error.stack||String(error);report.failures.push({theme,locale,error:result.failure});await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-failure.png`)}).catch(()=>{});}
    report.scenarios.push(result);await context.close();
  }
  if(process.env.NOVA_UX_NATIVE_PART!=='iris')for(const [displayMode,locale] of [['standard','zh-CN'],['full','en-US']]) {
    const context=await browser.newContext({viewport:{width:856,height:505},locale,colorScheme:'dark'});
    await installNativeFixture(context,installBoundary,{theme:displayMode==='full'?'Minecraft':'shadcn',locale});
    await context.addInitScript(()=>{
      const io=window.__novaUX;io.operations.length=0;io.preflightStarts=[];io.preflightDiscards=[];
      io.handlers.application_render_launch_plan=()=>({sourceBytes:1048576,directRealtime:false,preflight:structuredClone(io.currentPreflight)});
      io.handlers.application_render_discard_preflight=({preflightId})=>{io.preflightDiscards.push(preflightId);return null;};
      io.handlers.application_render_use_basic_vulkan=()=>({...io.currentPreflight,preflightId:'basic-'+io.currentPreflight.preflightId,severity:'warning',allowedActions:['continue_prebuilt','switch_realtime','cancel']});
      io.handlers.application_render_start=({input})=>{io.preflightStarts.push(input);return {operationId:'ux-preflight-'+io.preflightStarts.length,source:{path:input.sourcePath},displayMode:input.displayMode,buildMode:input.buildMode,state:'starting',progress:{phase:'starting'},presentation:{mode:'hidden',visible:false,placement:null},presentationState:'hidden',error:null};};
      io.handlers.application_render_cancel=()=>null;
      io.handlers.application_quick_preview_snapshot=()=>io.quick??null;
      io.handlers.application_quick_preview_start=({input})=>io.quick={previewId:'ux-fast-no-gate',sourcePath:input.sourcePath,renderMode:'fast_experimental',state:'ready',presentation:{mode:'hidden',visible:false,placement:null},presentationRevision:0,visible:false,error:null,preload:input.preload};
      io.handlers.application_quick_preview_present=({intent,revision})=>io.quick={...io.quick,presentation:intent,presentationRevision:revision,visible:intent.visible};
    });
    const page=await context.newPage();page.setDefaultTimeout(6500);const result={window:'render-preflight',displayMode,locale,states:[]};
    try{
      await page.goto(baseURL,{waitUntil:'networkidle'});await page.locator('.render-mode-switch button').nth(displayMode==='full'?2:1).click();
      await page.locator('.render-page-actions [role=checkbox]').click();
      const cases=[['normal','auto'],['warning','confirm'],['warning','realtime'],['hard','lower'],['unmeasured','realtime'],['invalid','cancel']];
      for(const[severity,action]of cases){
        const id=severity+'-'+action;
        await page.evaluate(({severity,id})=>{window.__novaUX.currentPreflight={preflightId:id,severity,conservativeVramBytes:severity==='unmeasured'?null:1024**3,safeAvailableVramBytes:2*1024**3,pressurePercent:severity==='hard'?98:severity==='warning'?85:50,components:null,invalidReason:['unmeasured','invalid'].includes(severity)?'Isolated resource identity is unavailable':null,allowedActions:severity==='normal'||severity==='warning'?['continue_prebuilt','switch_realtime','cancel']:severity==='hard'?['switch_realtime','use_basic_vulkan','cancel']:severity==='unmeasured'?['switch_realtime','cancel']:['cancel']};},{severity,id});
        const starts=await page.evaluate(()=>window.__novaUX.preflightStarts.length);
        await page.locator('.render-page-actions > button').first().click();
        if(action==='auto'){await page.waitForFunction(n=>window.__novaUX.preflightStarts.length===n+1,starts);assert.equal(await page.evaluate(()=>window.__novaUX.preflightStarts.at(-1).buildMode),'prebuilt');}
        else{
          const panel=page.locator('.render-preview-help').filter({has:page.locator('.render-page-actions')});await panel.waitFor();
          const buttons=panel.locator('button');assert.equal(await buttons.count(),4);assert.equal(await buttons.nth(0).isEnabled(),severity==='warning');
          if(action==='confirm'){await buttons.nth(0).click();await page.waitForFunction(n=>window.__novaUX.preflightStarts.length===n+1,starts);assert.equal(await page.evaluate(()=>window.__novaUX.preflightStarts.at(-1).resourceWarningConfirmed),true);}
          if(action==='realtime'){await buttons.nth(1).click();await page.waitForFunction(n=>window.__novaUX.preflightStarts.length===n+1,starts);assert.equal(await page.evaluate(()=>window.__novaUX.preflightStarts.at(-1).buildMode),'realtime');}
          if(action==='lower'){await buttons.nth(2).click();await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_render_use_basic_vulkan'));await page.waitForFunction(()=>[...document.querySelectorAll('.render-preview-help button')][0]?.disabled===false);await buttons.nth(0).click();await page.waitForFunction(n=>window.__novaUX.preflightStarts.length===n+1,starts);assert.equal(await page.evaluate(()=>window.__novaUX.preflightStarts.at(-1).preflightId),'basic-'+id);}
          if(action==='cancel'){if(!process.env.NOVA_UX_NATIVE_FIX_BASELINE)assert.match(await panel.getByRole('alert').innerText(),/Isolated resource identity/);await page.screenshot({path:path.join(outputDir,`native-preflight-${displayMode}-invalid.png`)});await buttons.nth(3).click();await panel.waitFor({state:'detached'});assert.ok(await page.evaluate(id=>window.__novaUX.preflightDiscards.includes(id),id));assert.equal(await page.evaluate(()=>window.__novaUX.preflightStarts.length),starts);}
        }
        result.states.push({severity,action});
      }
      await page.locator('.render-mode-switch button').first().click();await page.locator('.render-page-actions [role=checkbox]').click();
      const gateCalls=await page.evaluate(()=>window.__novaUX.calls.filter(c=>/application_render_launch_plan|application_quick_preview_resource_decision/.test(c.command)).length);
      await page.locator('.render-page-actions > button').first().click();await page.waitForFunction(()=>window.__novaUX.quick?.state==='ready');
      assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(c=>/application_render_launch_plan|application_quick_preview_resource_decision/.test(c.command)).length),gateCalls);
      result.states.push({state:'Fast-preload-no-resource-gate'});result.unknown=await page.evaluate(()=>window.__novaUX.unknown);assert.equal(result.unknown.length,0);
    }catch(error){result.failure=error.stack;report.failures.push({window:'render-preflight',displayMode,error:result.failure});await page.screenshot({path:path.join(outputDir,`native-preflight-${displayMode}-failure.png`)}).catch(()=>{});}
    report.scenarios.push(result);await context.close();
  }
  if(process.env.NOVA_UX_NATIVE_PART!=='preflight')for(const locale of ['zh-CN','en-US']) {
    const context=await browser.newContext({viewport:{width:800,height:500},locale,colorScheme:'dark'});
    await installBoundary(context,{theme:locale==='zh-CN'?'Minecraft':'shadcn',locale,config:{motion_preference:'off'}});
    await context.addInitScript(()=>{
      const io=window.__novaUX;window.__TAURI_INTERNALS__.metadata.currentWindow.label='iris-options';
      const snapshot={schema:'nova.iris-settings.v1',root:'X:\UX-shaders',pack:'UX fixture',pack_identity:'isolated-shaderpack',packs:[{name:'UX fixture'}],profiles:[{id:'high',label:'High',values:{CLOUDS:'true',QUALITY:'2'}}],selected_profile:'high',saved_values:{},applied_to_active_viewer:true,selected_for_rendering:true,
        options:[{id:'CLOUDS',kind:'boolean',label:'Clouds / 云',description:'Volumetric clouds',default_value:'true',value:'true',values:['false','true']},{id:'QUALITY',kind:'string',label:'Cloud quality / 云质量',default_value:'2',value:'2',values:['1','2','3','4'],slider:true},{id:'SHADOW',kind:'string',label:'Shadow / 阴影',default_value:'low',value:'low',values:['low','high']},{id:'INFO',kind:'string',label:'Information',description:'Read-only pack information.',default_value:'info',value:'info',values:['info'],presentation:'information'}],
        screens:[{id:'',title:'Categories',columns:1,elements:[{kind:'profile'},{kind:'link',screen:'lighting',label:'Lighting / 光照'}]},{id:'lighting',title:'Lighting / 光照',columns:1,elements:[{kind:'option',option:'CLOUDS'},{kind:'option',option:'QUALITY'}]}]};
      io.iris=snapshot;io.irisWrites=[];io.handlers.iris_shaderpack_settings=({args})=>{
        const save=args.indexOf('--save-json');if(save>=0){if(io.failIrisSave){io.failIrisSave=false;throw new Error('Isolated write denied');}const input=JSON.parse(args[save+1]);io.irisWrites.push(input);for(const option of snapshot.options)if(input.values[option.id]!==undefined)option.value=input.values[option.id];snapshot.selected_profile=input.profile;snapshot.saved_values={...input.values};}
        return structuredClone(snapshot);
      };
      io.handlers.close_iris_options_window=()=>null;io.handlers.native_preview_set_background=()=>null;
    });
    const page=await context.newPage();page.setDefaultTimeout(7000);const result={theme:locale==='zh-CN'?'Minecraft':'shadcn',locale,window:'iris_options',states:[]};const errors=[];page.on('pageerror',error=>errors.push(error.message));
    try {
      await page.goto(baseURL+'iris_options.html?root=X%3A%5CUX-shaders&pack=UX%20fixture',{waitUntil:'networkidle'});await page.waitForSelector('.iris-screen-link');
      await page.locator('.iris-screen-link').click();await page.waitForSelector('#iris-CLOUDS');
      await page.locator('#iris-CLOUDS').click();const save=page.locator('.iris-settings-actions button').last();await save.click();await page.waitForFunction(()=>window.__novaUX.irisWrites.length===1);
      assert.equal(await page.evaluate(()=>window.__novaUX.iris.options[0].value),'false');
      await page.locator('#iris-QUALITY').fill('2');await page.locator('#iris-QUALITY').dispatchEvent('input');await page.locator('#iris-QUALITY').dispatchEvent('change');
      await page.evaluate(()=>window.__novaUX.failIrisSave=true);await save.click();await page.waitForSelector('.iris-error');assert.equal(await page.evaluate(()=>window.__novaUX.irisWrites.length),1);
      await save.click();await page.waitForFunction(()=>window.__novaUX.irisWrites.length===2);
      await page.locator('.iris-menu-heading button').first().click();await page.waitForSelector('.iris-screen-link');
      await page.locator('.iris-menu-heading button').last().click();await page.waitForSelector('#iris-SHADOW');await page.locator('#iris-SHADOW').click();await page.getByRole('option',{name:'high',exact:true}).click();
      const footer=await page.locator('.iris-window-footer').evaluate(el=>el.getBoundingClientRect().toJSON());assert.ok(footer.bottom<=501);
      await page.screenshot({path:path.join(outputDir,`native-iris-${locale}.png`)});
      await page.locator('.iris-window-header button').click();assert.ok(await page.evaluate(()=>window.__novaUX.calls.some(c=>c.command==='close_iris_options_window')));
      assert.equal(await page.evaluate(()=>window.__novaUX.irisWrites.length),2,'close does not save a dirty draft');
      result.states.push({flow:'categories/options/actual-save-receipt/failure/retry/all-options/close-without-save',footer});result.unknown=await page.evaluate(()=>window.__novaUX.unknown);assert.equal(errors.length,0);assert.equal(result.unknown.length,0);
    }catch(error){result.failure=error.stack;report.failures.push({window:'iris_options',locale,error:result.failure});await page.screenshot({path:path.join(outputDir,`native-iris-${locale}-failure.png`)}).catch(()=>{});}
    report.scenarios.push(result);await context.close();
  }
  fs.writeFileSync(path.join(outputDir,process.env.NOVA_UX_NATIVE_PART?`native-${process.env.NOVA_UX_NATIVE_PART}-ui.json`:'native-ui.json'),JSON.stringify(report,null,2));return report;
}


async function auditNativeLayerInset({browser,baseURL,outputDir,installBoundary,report}) {
  for(const[theme,locale,viewport]of [['Minecraft','zh-CN',{width:1280,height:720}],['Aero7','en-US',{width:856,height:505}]]){
    const context=await browser.newContext({viewport,locale,colorScheme:'dark'});await installNativeFixture(context,installBoundary,{theme,locale});
    await context.addInitScript(sourcePath=>localStorage.setItem('nova.update-recovery.v1',JSON.stringify({shell:{route:'flake',currentFile:sourcePath}})),sourcePath);
    const page=await context.newPage();page.setDefaultTimeout(6500);const result={theme,locale,viewport,states:[]};
    try{
      await page.goto(baseURL,{waitUntil:'networkidle'});await page.waitForSelector('.flake-canvas');await page.locator('.flake-page__topbar .flake-page__edit-toggle [role=checkbox]').click();await page.waitForSelector('.flake-page__quickbar');
      await page.locator('.flake-page__quickbar-slot').nth(4).click();
      const geometry=await page.locator('.flake-page__quickbar-slot.is-selected').evaluate(el=>{const r=el.getBoundingClientRect(),v=el.closest('.flake-page__viewport').getBoundingClientRect(),p=getComputedStyle(el,'::after');return{selectionBottom:r.y+r.height/2+parseFloat(p.height)/2,viewportBottom:v.bottom,slot:r.toJSON()};});
      result.toolPanel=await page.locator('.flake-page__tool-panel-body').evaluate(el=>{const v=el.querySelector('[data-slot=scroll-area-viewport]'),i=v?.firstElementChild;return{panel:el.clientWidth,viewport:v?.clientWidth,scroll:v?.scrollWidth,inner:i?.getBoundingClientRect().width,display:i&&getComputedStyle(i).display,values:[...el.querySelectorAll('.flake-page__tool-panel-kv > :last-child')].map(n=>({width:n.getBoundingClientRect().width,whiteSpace:getComputedStyle(n).whiteSpace,wrap:getComputedStyle(n).overflowWrap}))};});
      result.geometry=geometry;await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-layers-inset-final.png`)});
      assert.ok(geometry.selectionBottom<=geometry.viewportBottom-1,'2D selected frame bottom remains inside viewport');
      await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-layers-inset-final.png`)});
      const action=page.locator('.flake-page__tool-panel-actions button').last();await action.scrollIntoViewIfNeeded();assert.ok(await action.evaluate(el=>{const r=el.getBoundingClientRect();return el.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2));}));
      await page.screenshot({path:path.join(outputDir,`native-${theme}-${locale}-layers-actions-final.png`)});
      result.states.push({state:'selected-outline-unclipped/last-tool-action-uncovered',geometry});result.unknown=await page.evaluate(()=>window.__novaUX.unknown);assert.equal(result.unknown.length,0);
    }catch(error){result.failure=error.stack;report.failures.push({theme,locale,error:result.failure});}
    report.scenarios.push(result);await context.close();
  }
  fs.writeFileSync(path.join(outputDir,'native-layers-ui.json'),JSON.stringify(report,null,2));return report;
}
