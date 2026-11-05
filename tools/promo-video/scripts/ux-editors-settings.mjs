import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';

// Full production entry, DOM controls and CSS. All invented values below are
// explicitly isolated native-IO responses, never real file/GPU acceptance.
export async function auditEditorsSettings({browser,baseURL,outputDir,installBoundary}) {
  const repo=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../../..');
  const templateRoot=path.join(repo,'data/generation-templates');
  const files={};
  if(fs.existsSync(templateRoot))for(const name of fs.readdirSync(templateRoot))if(name.endsWith('.json'))files['data/generation-templates/'+name]=fs.readFileSync(path.join(templateRoot,name),'utf8');
  const report={domain:'editors-settings',io:'isolated Tauri responses; bundled template JSON is read-only real data',nativeGui:false,scenarios:[],failures:[]};
  for(const route of ['home','generate','settings','properties','replace','composition','asset_manager_window','enumerator_window','demo_aux','format_import'].filter(route=>!process.env.NOVA_UX_ROUTE||route===process.env.NOVA_UX_ROUTE))for(const compact of [false,true]){
    const locale=compact?'en-US':'zh-CN',theme=compact?'Minecraft':'shadcn',width=compact?800:1280,height=compact?500:720;
    const context=await browser.newContext({viewport:{width,height},locale,colorScheme:'dark'});
    const source='X:\\Nova-UX-fixture\\超长投影文件名称_English_Schematic_With_Unicode_测试.litematic';
    await installBoundary(context,{route:route==='format_import'?'properties':route,theme,locale,currentFile:['properties','replace','format_import'].includes(route)?source:'',files,config:{material_list_window_behavior:'main_window_overlay',motion_preference:'off'}});
    await context.addInitScript(({source,route})=>{
      const s=window.__novaUX;
      if(route.endsWith('_window'))window.__TAURI_INTERNALS__.metadata.currentWindow.label=route.replace('_window','');
      if(route==='demo_aux')window.__TAURI_INTERNALS__.metadata.currentWindow.label='demo_window';
      const library={schemaVersion:5,entries:[],replicas:[],folders:[],categories:[],trash:[],redirects:[],favorites:[],recent:[]};
      const game={schema_version:1,id:'isolated-game',content_id:'fixture-version-1.21',game_version:'1.21',root:'X:/Nova-UX-fixture/game',resource_jar:'',source:'fixture'};
      const analysis={metadata:{name:'生产页面离屏夹具 Production UI fixture',author:'UX fixture',description:'This is isolated display data, not a rendered projection.',time_created:1700000000000,time_modified:1700000001000,litematic_version:6,litematic_subversion:1,minecraft_data_version:3953,total_blocks:8,total_volume:64,enclosing_size:{x:4,y:4,z:4}},regions:[{name:'main',position:{x:0,y:0,z:0},size:{x:4,y:4,z:4}}],structure_stats:{total_non_air_blocks:8}};
      const info=p=>({raw:p,normalized:p,parent_dir:'X:/Nova-UX-fixture',parent_exists:true,exists:!p.includes('missing'),is_file:!p.endsWith('/game'),is_dir:p.endsWith('/game'),is_absolute:true,has_litematic_ext:p.endsWith('.litematic')});
      s.handlers.get_path_info=({path})=>info(path);
      s.handlers.get_workspace_root=()=> 'X:/Nova-UX-fixture';
      s.handlers.game_data_context=()=>game;
      s.handlers.game_data_list=()=>({active:game,entries:[game]});
      s.handlers.game_data_language=()=>({values:{}});
      s.handlers.game_data_mods=()=>({context:game,mods:[],snapshotValid:false,snapshotFormat:'fixture'});
      s.handlers.read_file_string=({path})=>{
        if(path in s.files)return s.files[path];
        if(path.endsWith('derived/blockstates.json'))return JSON.stringify({blocks:{'minecraft:stone':{properties:{}},'minecraft:stone_bricks':{properties:{}},'minecraft:white_concrete':{properties:{}},'minecraft:black_concrete':{properties:{}}}});
        if(path.includes('/derived/enumerator/'))return JSON.stringify(['minecraft:stone','minecraft:stone_bricks']);
        if(path.includes('generation-templates/index'))return'[]';
        throw Error('Fixture file not configured: '+path);
      };
      s.handlers.library_content_command=({request})=>request.action==='query'?{entryIds:[],groups:[],paths:[],categoryCounts:{},untaggedCount:0,issueCount:0}:library;
      s.handlers.library_index_status=()=>({ready:true,indexedEntries:0,totalEntries:0});
      s.handlers.library_task_submit=()=> 'isolated-empty-index';
      s.handlers.library_task_status=()=>({pending:[],running:null});
      s.handlers.ai_get_config=()=>({provider:'mock',base_url:'',model:'',has_key:false,key_status:'unset',storage_note:''});
      s.handlers.application_get_close_request=()=>null;
      s.handlers['plugin:window|close']=()=>null;
      s.handlers.native_preview_set_background=()=>null; // HWND is not created in the isolated browser.
      s.handlers.open_iris_options_window=()=>null; // Only the launch request, not a native window acceptance.
      s.handlers.application_quick_preview_snapshot=()=>null;
      s.handlers.compute_litematic_checksum=()=>({sha256:'f'.repeat(64),byte_count:64}); // Fake isolated metadata identity, never an actual file hash.
      s.handlers.read_image_base64=()=>null; // Explicit missing icon IO; UI must retain readable fallback labels.
      s.handlers.application_inspect_project=()=>analysis;
      s.handlers.application_read_embedded_preview=()=>null;
      s.handlers.application_get_scene_view=()=>({applied:false,sourcePath:source});
      s.handlers.list_directory_entries=()=>[];
      s.handlers.application_query_block_database=()=>({version:'1.21',versions:[{version:'1.21',dataVersion:3953,blockCount:4,stateCount:4,legacy:false,outputExtension:'litematic'}],blocks:[]});
      s.handlers.application_world_export_profiles=()=>({profiles:[{version:'1.21',data_version:3953,family:'modern',min_y:-64,height:384,generator:'flat_modern',runtime_validation:'fixture-only'}],sourceDataVersion:3953,defaultVersion:'1.21'});
      s.handlers.application_replace_blocks=({input})=>({message:JSON.stringify({schema_version:2,input_file:source,output_file:input.outputPath,dry_run:input.dryRun,regions_scanned:1,palette_entries_scanned:2,palette_entries_changed:0,block_positions_affected:0,per_unit:input.stages.flat().map((_,i)=>({unit_index:i,hit_count:0,output_distribution:[],warnings:[],invalid_entries:[]})),warnings:[]})});
      const part={id:'fixture-part',name:'Fixture part 超长名称 '.repeat(6),sourcePath:source,snapshotId:'fixture',selection:null,regions:[],transform:{offset:[0,0,0],origin:[0,0,0],rotation:0,mirrorX:false,mirrorZ:false},visible:true,locked:false,clearAir:false};
      const composition={schema:1,id:'fixture-composition',generation:1,state:{name:'UX fixture',parts:[part],targetDataVersion:3953,conflictPolicy:'reject',splitBoxes:[]}};
      const report={blockCount:8,blockEntityCount:0,entityCount:0,sectionCount:1,collisionCount:0,collisions:[],issues:[],bounds:{min:[0,0,0],max:[3,3,3]},canExport:true};
      const compositionView=()=>({project:structuredClone(composition),parts:composition.state.parts.map(part=>({part:structuredClone(part),bounds:{min:[0,0,0],max:[3,3,3]},size:[4,4,4],blockCount:8,dataVersion:3953,regions:['main']})),canUndo:true,canRedo:false});
      s.handlers.application_data_query=({input})=>{
        if(input.kind==='versionConvert')return{sourceSha256:'isolated-fixture-only',sourceDataVersion:3953,sourceVersion:'1.21',effectiveReplacements:{},targetVersion:input.input.targetVersion,targetDataVersion:3953,entries:[],blockedPayloads:[],payloadChanges:[],writable:true,changedBlockCount:0,outputPath:null};
        if(input.kind==='format'&&route==='format_import'&&input.input.action==='runtimeInput')return{nativePassthrough:false,sourceDisplayName:'Imported structure fixture.nbt',originalSourcePath:source};
        if(input.kind==='format')return{nativePassthrough:true,sourcePath:source,sourceVersions:[{version:'1.21',dataVersion:3953}],capabilities:[{format:'litematic',canExport:true},{format:'structure',canExport:true},{format:'create',canExport:false,reason:'Fixture unsupported export'}]};
        if(input.kind==='replaceRules')return{rows:input.input.items.map((item,index)=>({index,unitIndex:index,label:item.label||'Fixture rule '+(index+1),kind:item._kind==='separator'?'separator':'rule'})),totalRules:input.input.items.length};
        if(input.kind==='composition'){
          const request=input.input;
          if(request.action==='list')return{projects:[structuredClone(composition)]};
          if(request.action==='open'||request.action==='create'||request.action==='save')return compositionView();
          if(request.action==='editorState')return{outputPath:'X:/Nova-UX-fixture/editor.litematic',editorScenePath:'X:/Nova-UX-fixture/editor-scene.json',editorScene:{schema:'nova.composition.editor-scene.v1',projectId:composition.id,generation:composition.generation,sceneKey:'isolated-scene',resources:[],instances:[],selectedInstanceId:request.selectedInstanceId??part.id,tool:request.tool??'translate',splitBoxes:[],eventPath:'X:/Nova-UX-fixture/editor-events.json',acknowledgedEventSequence:0}};
          if(request.action==='update'){Object.assign(part,request.patch);composition.generation++;return compositionView();}
          if(request.action==='report')return{report};
          if(request.action==='splitReport')return{outputs:[{name:'part_1',bounds:report.bounds,report}]};
        }
        if(input.kind==='stats'||input.kind==='statistics'||input.kind==='materials')return{items:[],materials:[],blocks:[],structural_block_counts:[{block_id:'minecraft:stone',count:8}]};
        throw Error('Fixture data query not configured: '+input.kind+'/'+input.input?.action);
      };
      s.handlers.application_recent_projects=()=>({projects:[{path:'X:/Nova-UX-fixture/missing.litematic',name:'已移动的项目 — Missing source '+ '超长名称LongName'.repeat(8),modifiedAt:1700000000000}]});
      s.handlers.application_generate_project=({input})=>({summary:{dry_run:true,regions_count:1,operation_count:input.plan.regions[0].operations.length,total_volume:16384,estimated_non_air_blocks:2048,palette_entries_per_region:[{region:'main',palette_entries:2}],material_summary:[{block_id:'minecraft:stone_bricks',properties:{},count:2048}],warnings:[],errors:[],wrote_file:false,output_size:null,verify_load_result:null},diagnostics:'isolated native generation response',analysis:null});
    },{source,route});
    const page=await context.newPage();page.setDefaultTimeout(6000);page.on('dialog',d=>d.dismiss());
    const scenario={route,locale,theme,width,height,steps:[],errors:[]};page.on('pageerror',e=>scenario.errors.push(e.message));
    const step=async(name,fn)=>{if(process.env.NOVA_UX_STEP&&!new RegExp(process.env.NOVA_UX_STEP).test(name))return;try{await fn();scenario.steps.push({name,status:'passed'});}catch(e){scenario.steps.push({name,status:'failed',error:e.message});report.failures.push({route,locale,name,error:e.message,layout:await page.locator('.properties-workspace,.properties-editor-scroll,.properties-editor,.properties-advanced,.properties-preview').evaluateAll(els=>els.map(el=>({cls:el.className,rect:el.getBoundingClientRect().toJSON(),display:getComputedStyle(el).display,grid:getComputedStyle(el).gridTemplateColumns,rows:getComputedStyle(el).gridTemplateRows,visibility:getComputedStyle(el).visibility}))).catch(()=>[])});await page.screenshot({path:path.join(outputDir,`editors-${route}-${locale}-${name}-failure.png`)}).catch(()=>{});}};
    const shot=async state=>{await page.evaluate(()=>document.fonts.ready);await page.screenshot({path:path.join(outputDir,`editors-${route}-${locale}-${state}.png`)});};
    const geometry=async()=>{
      const bounds=await page.evaluate(()=>({width:innerWidth,height:innerHeight,overflow:document.documentElement.scrollWidth>innerWidth+1,outside:[...document.querySelectorAll('body *')].filter(el=>{const r=el.getBoundingClientRect();return r.width&&r.right>innerWidth+1}).slice(-8).map(el=>({tag:el.tagName,cls:el.className,rect:el.getBoundingClientRect().toJSON(),min:getComputedStyle(el).minWidth,width:getComputedStyle(el).width})),dialogs:[...document.querySelectorAll('[role="dialog"]')].map(el=>el.getBoundingClientRect().toJSON())}));
      assert.equal(bounds.overflow,false,'app must not horizontally overflow: '+JSON.stringify(bounds.outside));
      for(const b of bounds.dialogs)assert.ok(b.x>=-1&&b.y>=-1&&b.right<=bounds.width+1&&b.bottom<=bounds.height+1,'dialog fits layout viewport');
    };
    try{
      await page.goto(baseURL+(route==='demo_aux'?'demo_window.html':route.endsWith('_window')?route.replace('_window','')+'.html':''),{waitUntil:'networkidle'});await page.waitForSelector(route==='asset_manager_window'?'.asset-manager-root':route==='enumerator_window'?'.enumerator-content':route==='demo_aux'?'.subwindow-standalone-page':'.app-shell');await page.waitForTimeout(120);
      await step('normal-layout',async()=>{await geometry();if(route==='home'){const placement=await page.locator('.home-page').evaluate(el=>({page:el.getBoundingClientRect().left,padding:parseFloat(getComputedStyle(el).paddingLeft),intro:el.querySelector('.home-introduction').getBoundingClientRect().left}));assert.ok(Math.abs(placement.intro-placement.page-placement.padding)<2,'home introduction aligns with the workspace padding');const rows=await page.locator('.home-workflows > button').evaluateAll(es=>es.map(el=>({rect:el.getBoundingClientRect().toJSON(),texts:[...el.querySelectorAll('strong,small')].map(t=>t.getBoundingClientRect().toJSON())})));for(let i=0;i<rows.length;i++){for(const b of rows[i].texts)assert.ok(b.top>=rows[i].rect.top-1&&b.bottom<=rows[i].rect.bottom+1,'home multiline row contains its text');if(rows[i].texts.length>1)assert.ok(rows[i].texts[1].top>=rows[i].texts[0].bottom-1,'home title and description do not overlap');if(i)assert.ok(rows[i].rect.top>=rows[i-1].rect.bottom-1,'home workflow rows do not overlap');}}if(route==='home'){const gaps=await page.locator('.home-workflow-copy').evaluateAll(es=>es.map(el=>el.getBoundingClientRect().left-el.previousElementSibling.getBoundingClientRect().right));assert.equal(gaps.length,3);for(const gap of gaps)assert.ok(gap>=8&&gap<=16,'home icon and multiline label stay adjacent');}if(route==='generate'){const styles=await page.locator('.generate-operation-select').evaluateAll(es=>es.map(el=>({display:getComputedStyle(el).display,border:getComputedStyle(el).borderTopWidth,background:getComputedStyle(el).backgroundColor})));for(const style of styles){assert.equal(style.display,'flex');assert.equal(style.border,'0px');assert.equal(style.background,'rgba(0, 0, 0, 0)');}}await shot('normal');});
      if(route==='home')await step('missing-recent-recovery',async()=>{await page.locator('.home-recent-list button').first().click();await page.locator('.home-recent [role="alert"]').waitFor();await shot('missing');assert.equal(await page.locator('.home-page').count(),1);});
      if(route==='generate'){
        await step('template-search-popover',async()=>{
          const before=await page.locator('.generate-workspace').boundingBox();await page.getByRole('button',{name:compact?'Templates':'模板库',exact:true}).click();
          await page.locator('.generate-template-popover').waitFor();await page.locator('.generate-template-popover input').first().fill('not-a-template');await shot('templates');await page.keyboard.press('Escape');
          assert.deepEqual(await page.locator('.generate-workspace').boundingBox(),before);
        });
        await step('operation-add-copy-reorder-select',async()=>{
          const rows=page.locator('.generate-operation-row');const before=await rows.count();await page.getByRole('button',{name:compact?'Add Operation':'添加操作',exact:true}).click();assert.equal(await rows.count(),before+1);
          if(compact)await page.locator('.generate-inspector-close').click();
          await rows.last().getByRole('button',{name:compact?'Duplicate':'复制',exact:true}).click();assert.equal(await rows.count(),before+2);
          await rows.last().getByRole('button',{name:compact?'Move Up':'上移',exact:true}).click();await rows.first().locator('.generate-operation-select').click();
          await shot('operation');if(compact)await page.locator('.generate-inspector-close').click();
        });
        await step('all-operation-types-and-parameter-return',async()=>{
          await page.locator('.generate-operation-select').first().click();
          const trigger=page.locator('.generate-parameter-inspector .generate-editor-toolbar [role="combobox"]');
          await trigger.click();const names=await page.locator('[role="option"]').allTextContents();await page.keyboard.press('Escape');
          for(let index=0;index<names.length;index++){await trigger.click();await page.locator('[role="option"]').nth(index).click();await geometry();}
          await shot('operation-last');if(compact)await page.keyboard.press('Escape');
        });
        await step('plan-settings-long-draft-cancel',async()=>{
          const before=await page.locator('.generate-workspace').boundingBox();await page.getByRole('button',{name:compact?'Plan settings':'方案设置',exact:true}).click();
          const dialog=page.locator('[role="dialog"]');await dialog.locator('input').first().fill('Long English 名称 '.repeat(16));await geometry();await shot('plan');await page.keyboard.press('Escape');assert.deepEqual(await page.locator('.generate-workspace').boundingBox(),before);
        });
        await step('preview-response-output',async()=>{await page.locator('.generate-primary-toolbar button').filter({hasText:compact?/^Preview Result$/:/^预览生成结果$/}).click();await page.locator('.generate-summary').waitFor();await geometry();await shot('result');await page.keyboard.press('Escape');});
        await step('ai-panel-and-parse-failure',async()=>{await page.getByRole('button',{name:compact?'Optional · AI-assisted Generation':'可选 · AI 辅助生成',exact:true}).click();await page.locator('.generate-ai-plan-input').fill('not valid JSON');await page.getByRole('button',{name:compact?'Parse and Preview':'解析并预览',exact:true}).click();await page.locator('.generate-error-list').waitFor();await geometry();await shot('ai');await page.getByRole('button',{name:compact?'Chat via API':'使用 API 对话',exact:true}).click();await page.locator('.generate-ai-dialog').waitFor();await geometry();await shot('ai-chat');await page.keyboard.press('Escape');await page.keyboard.press('Escape');});
      }
      if(route==='settings'){
        const categories=page.locator('.settings-categories > button');
        const selectCategory=async i=>compact?await page.locator('.settings-category-select').selectOption(['appearance','data','rendering','shaders','ai','diagnostics','maintenance'][i]):await categories.nth(i).click();
        for(let i=0;i<await categories.count();i++)await step('category-'+i,async()=>{await selectCategory(i);await geometry();await shot('category-'+i);});
        await step('search-focus-real-control',async()=>{await page.locator('.settings-search input').fill(compact?'Interface scale':'界面缩放');await page.locator('.settings-search-results button').first().click();assert.ok(await page.locator('[data-settings-category="appearance"]').first().isVisible());assert.ok(await page.evaluate(()=>document.activeElement?.closest('.ui-scale-row')!==null));await shot('search-focus');});
        await step('search-keyboard-return',async()=>{const search=page.locator('.settings-search input');await search.fill(compact?'scale':'缩放');await search.press('ArrowDown');await page.keyboard.press('Escape');assert.equal(await search.inputValue(),'');assert.equal(await search.evaluate(el=>document.activeElement===el),true);});
        await step('shader-draft-preserved-on-unrelated-save',async()=>{
          await selectCategory(3);const field=page.locator('[data-settings-category="shaders"] input').first();await field.fill('X:/LongShaderPath/尚未应用');await selectCategory(2);await page.locator('[data-settings-category="rendering"] [role="checkbox"]').first().click();await selectCategory(3);assert.equal((await field.inputValue()).replaceAll('\\','/'),'X:/LongShaderPath/尚未应用');
        });
        await step('iris-native-options-launch-request',async()=>{await page.locator('[data-settings-category="shaders"] input').nth(1).fill('fixture.zip');await page.locator('.iris-settings-panel button').click();assert.equal(await page.evaluate(()=>window.__novaUX.calls.some(c=>c.command==='open_iris_options_window')),true);});
        await step('resource-manager-all-categories',async()=>{
          await selectCategory(1);const opener=page.getByRole('button',{name:compact?'Game Resources':'游戏资源管理',exact:true});await opener.click();await page.locator('.asset-manager-dialog').waitFor();
          const nav=page.locator('.asset-manager-nav button');for(let i=0;i<await nav.count();i++){await nav.nth(i).click();await geometry();await shot('resources-'+i);const actions=page.locator('.asset-manager-table-wrap button');for(const action of await actions.all()){await action.scrollIntoViewIfNeeded();await action.focus();const hit=await action.evaluate(el=>{const r=el.getBoundingClientRect();return{visible:el.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2)),disabled:el.disabled,footerOverlap:(()=>{const f=document.querySelector('.asset-manager-footer')?.getBoundingClientRect();return !!f&&f.top<r.bottom&&f.bottom>r.top;})(),height:r.height,line:getComputedStyle(el).whiteSpace};});assert.equal(hit.footerOverlap,false,'resource footer never covers a table action');if(!hit.disabled)assert.equal(hit.visible,true,'resource row action remains reachable through local scrolling');assert.equal(hit.line,'nowrap','resource row action is not broken mid-word');await shot('resources-'+i+'-action');}await page.locator('.asset-manager-footer').scrollIntoViewIfNeeded();await shot('resources-'+i+'-footer');}
          await page.keyboard.press('Escape');assert.equal(await page.locator('.asset-manager-dialog').count(),0);assert.equal(await opener.evaluate(el=>document.activeElement===el),true);
        });
        await step('enumerator-keyboard-search-return',async()=>{
          const opener=page.getByRole('button',{name:compact?'Enumerator':'枚举器',exact:true});await opener.click();await page.locator('.enumerator-dialog').waitFor();await geometry();await shot('enumerator');
          const first=page.locator('.enumerator-collection-table-wrap tbody tr[tabindex="0"]').first();await first.focus();await page.keyboard.press('Enter');await page.locator('.enumerator-source-column').waitFor({state:'visible'});await shot('enumerator-source');
          if(compact)await page.locator('.enumerator-pane-select').selectOption('collections');await first.focus();await page.keyboard.press('Shift+F10');await page.locator('.enumerator-edit-column').waitFor({state:'visible'});await shot('enumerator-edit');
          await page.keyboard.press('Escape');assert.equal(await opener.evaluate(el=>document.activeElement===el),true);
        });
        await step('enumerator-draft-close-reopen',async()=>{const opener=page.getByRole('button',{name:compact?'Enumerator':'枚举器',exact:true});await opener.click();await page.locator('.enumerator-footer-actions button').first().click();const name=page.locator('.enumerator-name-input');await name.fill('Retained collection draft');await page.keyboard.press('Escape');await opener.click();if(compact)await page.locator('.enumerator-pane-select').selectOption('edit');assert.equal(await name.inputValue(),'Retained collection draft');await page.locator('.enumerator-footer-actions button').first().click();assert.equal(await name.inputValue(),'Retained collection draft');await shot('enumerator-retained');await page.keyboard.press('Escape');});
      }
      if(route==='properties'){
        await step('metadata-draft-and-undo',async()=>{const name=page.locator('.properties-editor input').first();const original=await name.inputValue();await name.fill('Long name 很长 '.repeat(20));await shot('draft');await page.getByRole('button',{name:compact?/Undo/:/撤销/}).first().click();assert.equal(await name.inputValue(),original);});
        await step('export-cancel-does-not-write',async()=>{const exportButton=page.locator('.properties-page').getByRole('button',{name:compact?'Other Formats':'其他格式',exact:true});await exportButton.click();await page.locator('[role="dialog"]').waitFor();const calls=await page.evaluate(()=>window.__novaUX.calls.length);await page.getByRole('button',{name:compact?/Save as/:/另存为/}).last().click();await geometry();await shot('export');assert.equal(await page.evaluate(n=>window.__novaUX.calls.slice(n).filter(c=>c.command==='application_data_query'&&c.args.input.input.action==='export').length,calls),0);await page.keyboard.press('Escape');});
        await step('world-export-input-and-cancel',async()=>{await page.getByRole('button',{name:compact?'Export Minecraft world':'导出 Minecraft 世界',exact:true}).click();await page.locator('[role="dialog"]').waitFor();await page.locator('[role="dialog"] input').first().fill('世界World long name '.repeat(16));await page.getByRole('button',{name:compact?'Choose folder…':'选择文件夹…',exact:true}).click();await page.locator('[role="dialog"] summary').click();const last=page.locator('.properties-world-export-origin input').last();await last.scrollIntoViewIfNeeded();await last.fill('32');await last.focus();assert.equal(await last.inputValue(),'32');await geometry();await shot('world');assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_export_world').length),0);await page.keyboard.press('Escape');});
        await step('version-conversion-return',async()=>{await page.locator('.properties-advanced > summary').click();await page.getByRole('button',{name:compact?'Convert Version':'转换版本',exact:true}).click();await page.locator('.properties-conversion-target').waitFor();await geometry();await shot('version');await page.keyboard.press('Escape');});
      }
      if(route==='replace'){
        await step('rule-list-and-editor',async()=>{if(compact)await page.locator('.replace-browser-toggle').click();await page.getByRole('button',{name:compact?'Add Replacement Rule':'添加替换规则',exact:true}).click();await page.locator('.replace-rule-list-item').first().waitFor();await page.locator('.replace-rule-list-item').first().click();await geometry();await shot('rule');});
        await step('rule-creative-picker-search-cancel',async()=>{await page.locator('.replace-add-buttons').first().locator('button').first().click();await page.locator('.flake-page__inventory-dialog').waitFor();const search=page.locator('.flake-page__creative-inventory-search input');await search.fill('no-match-long-name');await geometry();await shot('creative-picker');await page.keyboard.press('Escape');assert.equal(await page.locator('.flake-page__inventory-dialog').count(),0);});
        await step('rule-enumerator-picker-expression-cancel',async()=>{await page.locator('.replace-add-buttons').first().locator('button').nth(1).click();await page.locator('.enum-picker-content').waitFor();await page.locator('.enum-picker-input').fill('not_a_collection');await geometry();await shot('enum-picker');await page.keyboard.press('Escape');assert.equal(await page.locator('.enum-picker-content').count(),0);});
        await step('enumerator-read-error-remains-visible',async()=>{await page.evaluate(()=>{window.__novaUX.handlers.get_user_config_file_path=({relativePath})=>{if(relativePath.endsWith('/.dir'))throw Error('Isolated collection directory failure');return 'X:/Nova-UX-isolated/'+relativePath;};});try{await page.locator('.replace-add-buttons').first().locator('button').nth(1).click();const error=page.locator('.enum-picker-error[role="alert"]');await error.waitFor();assert.ok((await error.innerText()).includes('Isolated collection directory failure'));await page.locator('.enum-picker-input').fill('new expression');assert.ok((await error.innerText()).includes('Isolated collection directory failure'));await shot('enum-read-error');await page.keyboard.press('Escape');}finally{await page.evaluate(()=>delete window.__novaUX.handlers.get_user_config_file_path);}});
        await step('preview-confirm-cancel-no-write',async()=>{await page.locator('.replace-toolbar button').nth(1).click();await page.locator('.replace-toolbar button').last().waitFor({state:'visible'});await page.locator('.replace-toolbar button').last().click();await page.locator('[role="dialog"]').waitFor();await geometry();await shot('confirm');await page.keyboard.press('Escape');assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_replace_blocks'&&!c.args.input.dryRun).length),0);});
        await step('preset-and-empty-search-return',async()=>{await page.locator('.replace-preset-panel > summary').click();await page.locator('.replace-presets input').fill('UX preset 很长名称 '.repeat(8));await geometry();await shot('preset');await page.locator('.replace-preset-panel > summary').click();if(compact)await page.locator('.replace-browser-toggle').click();await page.locator('.replace-rule-search input').fill('no-match');await page.locator('.replace-rule-search input').fill('');if(compact)await page.keyboard.press('Escape');});
      }
      if(route==='composition')await step('inspector-overlay-return',async()=>{const before=await page.locator('.composition-workspace').boundingBox();const opener=page.getByRole('button',{name:compact?'Part properties':'部件属性',exact:true});if(await opener.count()){await opener.click();await page.locator('[role="dialog"]').waitFor();await geometry();await shot('inspector');await page.keyboard.press('Escape');assert.deepEqual(await page.locator('.composition-workspace').boundingBox(),before);}else{await page.locator('.composition-inspector').waitFor();await shot('inspector');}});
      if(route==='composition'){
        await step('composition-existing-part-edit-and-save',async()=>{await page.locator('.composition-header > select').selectOption('fixture-composition');await page.getByRole('button',{name:compact?'Part properties':'部件属性',exact:true}).click();await page.locator('.composition-drawer input').first().fill('Renamed fixture part');await page.locator('.composition-drawer input').first().press('Tab');await page.locator('.composition-drawer select').selectOption('90');await geometry();await shot('part-edit');await page.keyboard.press('Escape');await page.locator('.composition-toolbar').getByRole('button',{name:compact?'Save':'保存组合',exact:true}).click();assert.equal(await page.evaluate(()=>window.__novaUX.calls.some(c=>c.command==='application_data_query'&&c.args.input.input.action==='save')),true);});
        await step('composition-options-and-export-cancel',async()=>{const before=await page.locator('.composition-workspace').boundingBox();await page.getByRole('button',{name:compact?'Add options':'添加选项',exact:true}).click();await page.locator('.composition-options-popover').waitFor();await geometry();await shot('add-options');await page.keyboard.press('Escape');assert.deepEqual(await page.locator('.composition-workspace').boundingBox(),before);await page.getByRole('button',{name:compact?'Export…':'导出…',exact:true}).click();await page.getByRole('button',{name:compact?'Choose save location…':'选择保存位置…',exact:true}).click();await geometry();await shot('export');await page.keyboard.press('Escape');assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_data_query'&&c.args.input.input.action==='export').length),0);});
        await step('composition-split-review-return',async()=>{await page.locator('.composition-split > summary').click();await page.getByRole('button',{name:compact?'Review split outputs':'预览拆分清单',exact:true}).click();await page.locator('.composition-split tbody tr').waitFor();await page.locator('.composition-split tbody').scrollIntoViewIfNeeded();await geometry();await shot('split');await page.locator('.composition-split > summary').click();});
      }
      if(route==='asset_manager_window')await step('standalone-resource-categories-close-request',async()=>{const nav=page.locator('.asset-manager-nav button');for(let i=0;i<await nav.count();i++){await nav.nth(i).click();await geometry();await shot('category-'+i);const actions=page.locator('.asset-manager-table-wrap button');for(const action of await actions.all()){await action.scrollIntoViewIfNeeded();await action.focus();const hit=await action.evaluate(el=>{const r=el.getBoundingClientRect();return{visible:el.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2)),disabled:el.disabled,footerOverlap:(()=>{const f=document.querySelector('.asset-manager-footer')?.getBoundingClientRect();return !!f&&f.top<r.bottom&&f.bottom>r.top;})(),height:r.height,line:getComputedStyle(el).whiteSpace};});assert.equal(hit.footerOverlap,false,'resource footer never covers a table action');if(!hit.disabled)assert.equal(hit.visible,true,'resource row action remains reachable through local scrolling');assert.equal(hit.line,'nowrap','resource row action is not broken mid-word');await shot('category-'+i+'-action');}await page.locator('.asset-manager-footer').scrollIntoViewIfNeeded();await shot('category-'+i+'-footer');}await page.locator('.subwindow-close-button').click();assert.equal(await page.evaluate(()=>window.__novaUX.calls.some(c=>c.command==='plugin:window|close')),true);});
      if(route==='enumerator_window')await step('standalone-enumerator-keyboard',async()=>{await page.locator('.enumerator-collection-table-wrap tbody tr[tabindex="0"]').first().focus();await page.keyboard.press('Enter');await page.locator('.enumerator-source-column').waitFor({state:'visible'});await geometry();await shot('source');});
      if(route==='format_import')await step('import-version-and-cancel',async()=>{await page.locator('[role="dialog"]').waitFor();await page.locator('[role="dialog"] select').selectOption('3953');await geometry();await shot('import');await page.keyboard.press('Escape');assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_data_query'&&c.args.input.input.action==='decodeToSnapshot').length),0);});
      scenario.unknown=await page.evaluate(()=>window.__novaUX.unknown);scenario.calls=await page.evaluate(()=>window.__novaUX.calls.map(call=>call.command));
      if(scenario.errors.length)report.failures.push({route,locale,name:'unhandled-page-errors',errors:scenario.errors});
    }catch(e){scenario.failure=e.message;report.failures.push({route,locale,name:'entry',error:e.message});await shot('entry-failure').catch(()=>{});}
    report.scenarios.push(scenario);await context.close();
  }
  fs.writeFileSync(path.join(outputDir,process.env.NOVA_UX_ROUTE?`editors-${process.env.NOVA_UX_ROUTE}-recheck.json`:'editors-settings.json'),JSON.stringify(report,null,2));return report;
}
