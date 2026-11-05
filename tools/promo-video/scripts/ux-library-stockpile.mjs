import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';

// These are explicit native-response fixtures. They exercise production React,
// CSS, portals and event handlers; no disk, rendering or Rust query is simulated
// as an end-to-end success. Rust query semantics have separate directed tests.
export async function auditLibraryStockpile({browser,baseURL,outputDir,installBoundary}) {
  const icons=Object.fromEntries(['stone','oak_log','glass','redstone','chest','shulker_box','crafting_table','tnt','furnace','oak_sapling','iron_ingot','hopper','wheat'].map(id=>[id,'data:image/png;base64,'+fs.readFileSync(path.resolve('pack-in/item-icons-26.3',id+'.png')).toString('base64')]));
  const localeSource=fs.readFileSync(path.resolve('desktop-nova/src/services/locales/library.ts'),'utf8');
  const parseLocale=text=>Object.fromEntries([...text.matchAll(/"([^"\n]+)":\s*("(?:\\.|[^"\\])*")/g)].map(m=>[m[1],JSON.parse(m[2])]));
  const [zhSource,enSource]=localeSource.split('export const enUS');const zh=parseLocale(zhSource),en=parseLocale(enSource);
  for(const file of ['editor.ts','app.ts']){const [a,b]=fs.readFileSync(path.resolve('desktop-nova/src/services/locales',file),'utf8').split('export const enUS');Object.assign(zh,parseLocale(a));Object.assign(en,parseLocale(b||''));}
  const englishLabels=Object.fromEntries(Object.entries(zh).map(([key,value])=>[value,en[key]||value]));
  const report={domain:'library-stockpile',io:'isolated native responses; real production UI',nativeGui:false,scenarios:[],failures:[]};
  for(const route of (process.env.NOVA_UX_LIBRARY_ROUTES?.split(',')||['library','statistics','materials']))for(const compact of [false,true]){
    const theme=compact?'Minecraft':'shadcn',locale=compact?'en-US':'zh-CN',width=compact?(route==='materials'?720:800):1440,height=compact?600:900;
    const label=text=>locale==='en-US'?(englishLabels[text]||text):text;
    const key=id=>(locale==='en-US'?en:zh)[id]||id;
    const context=await browser.newContext({viewport:{width,height},locale,colorScheme:'dark'});
    await installBoundary(context,{route,theme,locale,currentFile:route==='statistics'?'X:/Nova-UX-fixture/Tower.litematic':'',config:{motion_preference:'off',material_list_window_behavior:'main_window_overlay',local_library_folder_window_behavior:'main_window_overlay',reden_library_window_behavior:'main_window_overlay'}});
    await context.addInitScript(({icons})=>{
      // These labels match main.rs auxiliary WebviewWindowBuilder entries.
      // Overlays remain in main; only standalone HTML entries change identity.
      const windowLabel=({'material_list.html':'material-list','local_library_folders.html':'local-library-folders','reden_library.html':'reden-library'})[location.pathname.split('/').at(-1)]||'main';
      window.__TAURI_INTERNALS__.metadata.currentWindow.label=windowLabel;
      window.__TAURI_INTERNALS__.metadata.currentWebview.label=windowLabel;
      const s=window.__novaUX,source='X:/Nova-UX-fixture/Tower.litematic';
      const entries=Array.from({length:16},(_,i)=>({id:'entry-'+i,status:'active',checksum:'isolated-'+i,displayName:i===0?'高塔 Tower with a very long visible title':i===1?'宽建筑 宽建筑 '+ 'Long name '.repeat(6):'投影 Projection '+i,internalName:'',author:'UX author',description:'Isolated display fixture',tags:[],categoryIds:i<8?['build']:[],stats:{totalBlocks:128+i,totalVolume:4096,regionCount:1,regionNames:['main'],enclosingSize:{x:16,y:16,z:16},minecraftDataVersion:3953},preview:{path:'',updatedAt:1},sortIndex:i,createdAt:1,updatedAt:1}));
      const replicas=entries.map((e,i)=>({id:'replica-'+i,entryId:e.id,path:'X:/Nova-UX-fixture/'+i+'.litematic',fileName:i+'.litematic',folderPath:'X:/Nova-UX-fixture',checksum:e.checksum,fileSize:1234,mtime:1,status:'active',lastAnalyzedAt:1,lastError:''}));
      replicas.push({...replicas[0],id:'replica-missing',path:'X:/Missing/0.litematic',folderPath:'X:/Missing',status:'missing',lastError:'Isolated missing file'});
      const library={schemaVersion:5,entries,replicas,folders:[{path:'X:/Nova-UX-fixture',type:'tracked',enabled:true,recursive:true,autoActivate:false,lastSyncedAt:1700000000,lastScanCount:16,lastError:''}],categories:[{id:'build',name:'建筑 Buildings',parentId:null,sortIndex:0,icon:{kind:'lucide',name:'Folder'}},{id:'machines',name:'机械 Machines',parentId:null,sortIndex:1,icon:{kind:'lucide',name:'Cog'}}],trash:[{id:'trash-1',name:'已删除示例 Deleted fixture',entryId:'retired',replicaId:'retired-copy',originalPath:'X:/Nova-UX-fixture/deleted.litematic',originalFileName:'deleted.litematic',trashPath:'X:/Nova-UX-fixture/trash/deleted.litematic',movedAt:1,status:'in_trash'}],redirects:[],favorites:['entry-0'],recent:[]};
      s.files['projection-library/js_library.json']=JSON.stringify(library);
      s.handlers.library_content_command=({request})=>{
        if(request.action==='query')return{entryIds:entries.map(e=>e.id),groups:[{key:'all',label:'',entryIds:entries.map(e=>e.id)}],paths:replicas.map(r=>r.path),categoryCounts:{build:8},untaggedCount:8,issueCount:0};
        return request.state;
      };
      s.handlers.library_index_status=()=>({ready:true,indexedEntries:16,totalEntries:16});
      s.handlers.library_index_query=()=>({ready:true,entryIds:['entry-0'],total:1});
      s.handlers.library_task_submit=()=> 'isolated-index';
      s.handlers.library_task_status=()=>({pending:[{taskId:'queued-scan',kind:'scan'}],running:{taskId:'running-index',kind:'index'}});
      s.handlers.library_task_cancel=()=>null;
      s.handlers.application_data_operation_status=({operationId,cancel})=>{if(cancel&&s.recipeOperation===operationId)s.finishRecipe?.(null);return{status:'cancelled'};};
      s.handlers.application_data_query=({input})=>{
        if(input.kind==='format')return{nativePassthrough:true,sourcePath:source};
        if(input.kind==='stats')return{source_revision:'fixture-rev-1',statisticsView:{totalNonAirBlocks:512,regionCount:1,enclosingSize:{x:16,y:16,z:16},density:0.125,buildingType:'ordinary',redstoneRatio:0,fluidRatio:0,entityCount:0,materialCount:4,regions:[{name:'main',bounds:null,size:{x:16,y:16,z:16},position:{x:0,y:0,z:0}}],layers:[],containerScan:{enabled:false,containers_scanned:0,item_stacks_scanned:0,warnings:[]}}};
        if(input.kind==='statisticsRules')return{rules:input.input.rules||[],results:[],hasErrors:false};
        if(input.kind==='materialRecipes'){if(s.recipeScenario==='error')throw Error('Isolated recipe preparation failure');s.recipeOperation=input.operationId;return new Promise(resolve=>{s.finishRecipe=resolve;});}
        if(input.kind==='thumbnail')return input.input.sourcePaths.map(sourcePath=>({sourcePath,image:null,error:null}));
        if(input.kind==='materials')return{rows:materials.map(m=>({id:m.id,itemId:m.id,name:m.display_name,count:128,blockCount:128,containerItemCount:0,totalCount:128,iconHint:m.id,stackSize:64,canFitContainer:true,materialStatus:'item',hasCustomStackSize:false,quantityText:'2 组'})),totalRows:4,totalMaterials:4,offset:0,limit:200,sourceRevision:'fixture-rev-1',includeContainerItems:false};
        if(input.kind==='materialDetail')return{material:materials.find(m=>m.id===input.input.materialId),recipeOptions:[{recipe_id:'fixture-recipe',recipe_type:'minecraft:crafting_shaped',output_count:1,display_name:'隔离配方 Fixture recipe',recipe_type_name:'Crafting'}],recipeTree:recipeTree(input.input.materialId),iconImages:{},itemNames:{},sourceSummary:{regionCount:1,minY:0,maxY:15,totalCount:128,unlocatedCount:0},sourceGroups:sourceGroups(input.input.sourceQuery)};
        throw Error('Unexpected fixture data request '+input.kind);
      };
      s.handlers.get_workspace_root=()=> 'X:/Nova-UX-fixture';
      s.handlers.check_file_exists=()=>true;
      s.handlers.get_path_info=({path:p})=>({raw:p,normalized:p,parent_dir:'X:/Nova-UX-fixture',parent_exists:true,exists:true,is_file:p.endsWith('.litematic'),is_dir:!p.endsWith('.litematic'),is_absolute:true,has_litematic_ext:p.endsWith('.litematic')});
      s.handlers.game_data_context=()=>({id:'isolated-game',content_id:'isolated-resource',game_version:'1.21',root:'X:/Nova-UX-fixture/game',model_root:'X:/Nova-UX-fixture/game'});
      s.handlers.game_data_list=()=>({active:s.handlers.game_data_context(),entries:[s.handlers.game_data_context()]});
      s.handlers.read_file_string=({path:p})=>p.endsWith('derived/blockstates.json')?JSON.stringify({blocks:{'minecraft:stone':{properties:{}}}}):JSON.stringify({property_keys:{},property_values:{}});
      s.handlers.game_data_language=()=>({values:{}});
      s.handlers.application_query_block_database=()=>({version:'1.21',versions:[{version:'1.21',dataVersion:3953,blockCount:4,stateCount:4,legacy:false,outputExtension:'litematic'}],blocks:[{id:'minecraft:stone',name:'Stone',properties:{},defaultState:'minecraft:stone'}]});
      s.handlers.read_image_base64=({path:p})=>icons[p.split(/[\\/]/).pop().replace(/\.png$/,'')]??null;
      s.handlers.read_game_data_icon=({id})=>icons[id.replace('minecraft:','')]??null;
      s.handlers.native_preview_set_background=()=>null;
      s.handlers.list_directory_entries=()=>[];s.handlers.choose_user_config_dir=()=>null;
      const preferences={search:'',category:'all',filter:'all',sort:'grouped',view:'compact',individualCounts:false,language:'auto',showAdvancedRecipeTree:true,showUnresolvedRecipes:true,showIconFallbackBadge:true};
      const materials=['stone','oak_log','glass','redstone'].map((name,i)=>({id:'minecraft:'+name,namespace_id:'minecraft:'+name,display_name:['石头 Stone','橡木原木 Oak Log','玻璃 Glass','红石 Redstone'][i],required_count:128,stack_size:64,stacks:2,remainder:0,shulker_boxes:1,category:'Blocks',category_icon:'',item_icon_key:'minecraft:'+name,icon_path:'',icon_available:true,normalized_from:[],display_names:{},source_regions:['main'],recipe_status:'unavailable',craft_complexity:0}));
      const recipeTree=(id,child=false)=>({item_id:id,display_name:child?'玻璃 Glass':'示例工艺 Fixture process',icon_key:id,needed_count:128,visual_kind:'item',unresolved:false,unresolved_reason:null,tag:null,possible_items:[],node_id:child?'child':'root',output_count:1,batch_count:128,extra_output:0,recipe_type:'minecraft:crafting_shaped',process_type:'craft',ingredients:[],children:child?[]:[recipeTree('minecraft:glass',true)],depth:child?1:0,recipe_type_name:'Crafting',requires_fuel:false,fuel_estimate:null,decorative_smithing:false});
      const sourceGroups=query=>[{region:'main',displayName:'长名称来源区域 Source region with a long title',minY:0,maxY:15,totalCount:128,layerCount:102,unlocatedCount:0,layers:[{layer:query?.offset||0,worldY:query?.offset||0,count:128,blockCount:128,containerItemCount:0}],offset:query?.offset||0,limit:100}];
      const items=materials.map(m=>({materialId:m.id,requiredCount:128,preparedCount:0,remainingCount:128,complete:false,status:'preparing',quantity:0,preparingCount:0,doneCount:0,overfilledCount:0,stackSize:64,publicNote:null,storageLocation:null,locked:false}));
      const rows=items.map((item,i)=>({...item,material:materials[i],displayName:materials[i].display_name,displayStatus:'not_started',requiredText:'2 组',preparedText:'0',remainingText:'2 组',preparingText:'0',overfilledText:'0',unitSteps:[1,64,1728],stacks:2,remainder:0,boxesRequired:1}));
      const task={taskId:'task-1',projectId:'project-1',name:'多投影备货 Multisource task',sourcePath:source,sourceRevision:'fixture-rev-1',sourceStatus:'current',multiplier:1,createdAt:1700000000,updatedAt:1700000000,sourceCount:1,totalCopies:1,context:{scope:{kind:'all',value:null},includeContainerItems:false},sources:[{sourceId:'source-1',sourcePath:source,projectId:'project-1',scope:{kind:'all',value:null},includeContainerItems:false,copies:1,label:'Tower',minecraftVersion:'1.21',resourceIdentity:'isolated-resource',sourceRevision:'fixture-rev-1',status:'current'}],demandSources:[],preferences,view:{rows,totalRows:4,offset:0,limit:200,categories:['Blocks'],capabilities:{backup:true,import:true,reset:true},requiredText:'8 组',preparedText:'0',remainingText:'8 组',completionPercent:0},materials:{schema_version:1,project:{source_file:source,created_at:1700000000,data_version:3953,regions:['main']},summary:{total_blocks:512,unique_materials:4,total_stacks:8,estimated_shulker_boxes:1},materials,diagnostics:[]},recipeTrees:{},iconImages:{},itemNames:{},items,requiredCount:512,preparedCount:0,remainingCount:512,preparingCount:0,overfilledCount:0,completedItems:0,totalItems:4};
      const workspace={legacyTasks:[],tasks:[task],selectedTaskId:task.taskId,task};
      s.stockpileWorkspace=workspace;
      s.handlers.application_stockpile_task=()=>structuredClone(workspace);
      s.handlers.application_stockpile_task_command=({input})=>{
        if(input.action==='details'){task.detailMaterialId=input.materialId;task.sourceSummary={regionCount:1,minY:0,maxY:15,totalCount:128,unlocatedCount:0};task.sourceGroups=sourceGroups(input.sourceQuery);task.recipeTrees[input.materialId]=recipeTree(input.materialId);task.demandSources=[{...task.sources[0],perCopyCount:128,requiredCount:128}];}
        if(input.action==='settings')Object.assign(task.preferences,input.preferences);
        return structuredClone(workspace);
      };
      // UI-only snapshots of every field emitted by material_export.rs::configure.
      // Real validation, persistence and export IO remain Rust responsibilities.
      s.handlers.application_export_material_list=({input})=>{
        const settings={source:'materials',format:'svg',scope:'all',region:'main',layer:0,multiplier:1,includeContainerItems:false,individualCounts:false,terminalRawMaterials:false,webMode:'single',accessPassword:'',adminPassword:'',whitelist:'',whitelistEnabled:false,allowGuestReadonly:true,adminPageEnabled:true,targets:['windows-x64'],...(input.action==='reset'?{}:input.settings||{})};
        const formats=settings.source==='task'?['copy','txt','csv','json']:settings.source==='taskMaterials'?['copy','txt','csv']:['svg','txt','html','csv','copy'];if(!formats.includes(settings.format))settings.format=formats[0];
        const taskSource=settings.source!=='materials', snapshot=settings.source==='task'&&settings.format==='json', web=!taskSource&&settings.format==='html', multi=web&&settings.webMode==='multi';
        const options=values=>values.map(value=>value==='main'?{value,label:'main'}:{value,labelKey:'stats.export.option.'+value});
        const field=(key,type,group,visible=true,enabled=true,values=[],reasonKey)=>({key,type,group,visible,enabled,value:settings[key],labelKey:'stats.export.field.'+key,options:options(values),reasonKey});
        const fields=[field('source','select','primary',true,true,['materials','taskMaterials','task']),field('format','select','primary',true,true,settings.source==='task'?['copy','txt','csv','json']:taskSource?['copy','txt','csv']:['svg','txt','html','csv','copy']),field('scope','select','common',true,!snapshot,taskSource?['all','visible','remaining','overfilled','prepared']:['all','region','layer'],snapshot?'stats.export.reason.snapshot':undefined),field('region','select','common',!taskSource&&settings.scope==='region',true,['main']),field('layer','number','common',!taskSource&&settings.scope==='layer'),field('multiplier','number','common',!taskSource),field('includeContainerItems','boolean','common',!taskSource),field('individualCounts','boolean','common',taskSource,!snapshot),field('terminalRawMaterials','boolean','advanced',!taskSource,false,[],'stats.export.reason.recipes'),field('webMode','select','web',web,true,['single','multi']),...['accessPassword','adminPassword','whitelist','whitelistEnabled','allowGuestReadonly','adminPageEnabled'].map(key=>field(key,key.endsWith('Password')?'password':key==='whitelist'?'multiline':'boolean','web',multi,multi)),field('targets','multiselect','web',multi,multi,['windows-x64','linux-x64','macos-x64','macos-arm64'])];
        return{settings,fields,valid:true,errors:[],defaultPath:'materials.'+settings.format,extension:settings.format,copy:settings.format==='copy',sourceRevision:'fixture-rev-1'};
      };
      const online={key:'fixture-machine',name:'可下载的完整建筑 Downloadable structure with a long title',type:'litematica',sourceId:'redenmc',summary:'资源详情与下载操作使用隔离 IO；布局、选择、取消与事件来自生产组件。',author:{username:'Fixture author'},downloads:12345,upVotes:12,updatedAt:1700000000,attachments:[{name:'Structure with a long file name.litematic',url:'https://example.invalid/structure.litematic',size:2048,description:'实体完整结构'}],hasX:true,hasY:true,hasZ:true,pageUrl:'https://example.invalid/fixture'};
      s.handlers.application_online_search=()=>({d:[online,{...online,key:'fixture-other',name:'第二结构 Second structure'}],count:2});
      s.handlers.application_online_detail=()=>online;
      s.handlers.application_online_cancel=()=>null;
      s.handlers.application_online_download=()=>{throw Error('Isolated IO download failure');};
      // Controllable IO completion, not a replacement for production UI handlers.
      // Draft bytes are a known fixture image: this is not GPU rendering evidence.
      const draft=()=>({preview_path:'isolated-preview.png',data_url:icons.stone,draft_id:'isolated-draft',source_identity:'isolated-input',display_mode:'standard',can_embed:s.previewScenario!=='readonly',persisted:false});
      s.handlers.generate_preview_images=({filePaths})=>{
        if(s.previewScenario==='error')throw Error('Isolated preview generation failure');
        if(s.previewScenario==='pending')return new Promise(resolve=>{s.finishPreview=resolve;});
        return{items:filePaths.map(file_path=>({file_path,image:draft(),error:null})),cancelled:false};
      };
      s.handlers.cancel_preview_generation=({action})=>{if(action==='cancel')s.finishPreview?.({items:[],cancelled:true});return null;};
      s.handlers.discard_generated_preview=()=>null;
      s.handlers.save_generated_preview=()=>{if(s.previewScenario==='save-error')throw Error('Isolated source write failure');return{...draft(),persisted:true};};
    },{icons});
    const page=await context.newPage();page.setDefaultTimeout(6000);const scenario={route,theme,locale,steps:[],errors:[]};page.on('pageerror',e=>scenario.errors.push(e.message));
    const step=async(name,fn)=>{if(process.env.NOVA_UX_LIBRARY_STEPS&&!new RegExp(process.env.NOVA_UX_LIBRARY_STEPS).test(name))return;try{await fn();const controls=await page.locator('button,input,select,textarea,[role="menuitem"],[role="option"],[role="combobox"]').evaluateAll(nodes=>nodes.filter(n=>n.getBoundingClientRect().width>0&&n.getBoundingClientRect().height>0).map(n=>({tag:n.tagName,role:n.getAttribute('role'),label:n.getAttribute('aria-label')||n.getAttribute('title')||n.textContent?.trim().slice(0,160),disabled:n.disabled===true||n.getAttribute('aria-disabled')==='true'})));scenario.steps.push({name,status:'passed',controls});}catch(e){scenario.steps.push({name,status:'failed',error:e.message});report.failures.push({route,theme,name,error:e.message});await shot('failure-'+name).catch(()=>{});}};
    const shot=async state=>{await page.evaluate(()=>document.fonts.ready);await page.screenshot({path:path.join(outputDir,`${route}-${theme}-${state}.png`)});};
    const box=selector=>page.locator(selector).first().boundingBox();
    try{
      await page.goto(route==='materials'?baseURL+'material_list.html?file='+encodeURIComponent('X:/Nova-UX-fixture/Tower.litematic'):baseURL,{waitUntil:'networkidle'});await page.waitForSelector(route==='library'?'.library-tile':route==='materials'?'.material-catalog-row':'.stockpile-row');
      await step('initial-layout',async()=>{scenario.windowLabel=await page.evaluate(()=>window.__TAURI_INTERNALS__.metadata.currentWindow.label);assert.equal(scenario.windowLabel,route==='materials'?'material-list':'main');assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth+1),false);await shot('initial');});
      if(route==='library'){
        const first=page.locator('.library-tile').first();
        await step('detail-overlay-preserves-grid-and-repeat-clears',async()=>{
          const before=await box('.library-card-grid'),card=await first.boundingBox();await first.locator('.library-tile-open').click();await page.locator('.library-inspector').waitFor();assert.deepEqual(await box('.library-card-grid'),before);assert.deepEqual(await first.boundingBox(),card);await shot('inspector');await first.locator('.library-tile-open').click();assert.equal(await page.locator('.library-inspector').count(),0);
        });
        await step('checkbox-independent-and-batch-reserved',async()=>{
          const before=await box('.library-card-grid');await first.getByRole('checkbox').click();assert.equal(await first.getAttribute('data-checked'),'true');assert.equal(await page.locator('.library-inspector').count(),0);assert.deepEqual(await box('.library-card-grid'),before);await first.getByRole('checkbox').focus();await page.keyboard.press('Escape');assert.equal(await first.getByRole('checkbox').getAttribute('aria-checked'),'false');
        });
        await step('list-keyboard-and-menus',async()=>{
          await page.keyboard.press('Escape');await page.getByRole('button',{name:label('列表视图'),exact:true}).click();const row=page.locator('.library-list-row').first();await row.focus();await page.keyboard.press('Enter');await page.locator('.library-inspector').waitFor();await row.focus();await page.keyboard.press('Escape');assert.equal(await page.locator('.library-inspector').count(),0);
          await page.getByRole('button',{name:label('库管理 / 更多'),exact:true}).click();await page.locator('.library-management-popover').waitFor();assert.ok(await page.locator('.library-management-popover button').count()>=6);await shot('management');await page.keyboard.press('Escape');assert.equal(await page.locator('.library-management-popover').count(),0);await page.getByRole('button',{name:label('网格视图'),exact:true}).click();
        });
        await step('filter-sort-group-columns-contract',async()=>{
          for(const name of ['筛选文件状态','投影排序']){const trigger=page.getByRole('combobox',{name:label(name),exact:true});await trigger.click();assert.ok(await page.locator('[role="option"]').count()>=3);await page.keyboard.press('ArrowDown');await page.keyboard.press('Enter');}
          await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='library_content_command'&&c.args.request.action==='query'&&c.args.request.query.sort!=='manual'));
          await page.getByRole('button',{name:label('库管理 / 更多'),exact:true}).click();const dimension=page.getByRole('combobox',{name:label('分组维度'),exact:true});await dimension.click();assert.equal(await page.locator('[role="option"]').count(),4);await page.keyboard.press('Escape');await page.keyboard.press('Escape');
          await page.getByRole('button',{name:label('列表视图'),exact:true}).click();await page.getByRole('button',{name:label('配置详细信息列'),exact:true}).click();assert.equal(await page.locator('.library-column-menu [role="checkbox"]').count(),9);const check=page.locator('.library-column-menu [role="checkbox"]').first();const before=await check.getAttribute('aria-checked');await check.click();assert.notEqual(await check.getAttribute('aria-checked'),before);await page.keyboard.press('Escape');await page.getByRole('button',{name:label('网格视图'),exact:true}).click();
        });
        await step('save-dialog-cancel-does-not-export',async()=>{
          await page.getByRole('button',{name:label('库管理 / 更多'),exact:true}).click();await page.locator('.library-management-popover').getByRole('button',{name:label('打包导出'),exact:true}).click();await page.locator('.library-export-dialog').waitFor();await page.getByRole('button',{name:label('选择目标…'),exact:true}).click();assert.equal(await page.getByRole('button',{name:label('开始导出'),exact:true}).isDisabled(),true);assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='library_export_zip').length),0);await shot('export');await page.keyboard.press('Escape');await page.keyboard.press('Escape');
        });
        await step('task-queue-status-and-cancel',async()=>{
          await page.getByRole('button',{name:label('库管理 / 更多'),exact:true}).click();await page.locator('.library-management-popover').getByRole('button',{name:label('任务中心'),exact:true}).click();await page.locator('.library-task-center').waitFor();await page.locator('[data-task-id="queued-scan"]').getByRole('button',{name:label('取消'),exact:true}).click();assert.ok(await page.evaluate(()=>window.__novaUX.calls.some(c=>c.command==='library_task_cancel'&&c.args.taskId==='queued-scan')));await page.keyboard.press('Escape');await page.keyboard.press('Escape');
        });
        if(!compact)await step('category-context-menu-and-cancel',async()=>{await page.locator('[data-category-tree-node]').first().click({button:'right'});await page.locator('.library-category-menu').waitFor();assert.equal(await page.locator('.library-category-menu [role="menuitem"]').count(),5);await page.locator('.library-category-menu [role="menuitem"]').nth(1).click();await page.locator('[role="dialog"]').waitFor();await page.keyboard.press('Escape');});
        if(!compact)await step('trash-irreversible-confirmation-cancel',async()=>{await page.locator('.library-trash-button').click();await page.locator('.library-trash-panel').waitFor();await page.locator('.library-trash-actions button').last().click();assert.equal(await page.locator('[role="dialog"]').count(),2);await page.keyboard.press('Escape');assert.equal(await page.locator('[role="dialog"]').count(),1);await page.locator('.library-trash-footer button').click();assert.equal(await page.locator('[role="dialog"]').count(),2);await shot('trash-confirm');await page.keyboard.press('Escape');assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='library_trash_purge').length),0);await page.keyboard.press('Escape');});
        if(!compact)await step('folder-manager-and-online-return',async()=>{await page.getByRole('button',{name:label('库管理 / 更多'),exact:true}).click();await page.locator('.library-management-popover').getByRole('button',{name:label('管理文件夹'),exact:true}).click();await page.locator('.library-window-dialog').waitFor();await shot('folders');await page.keyboard.press('Escape');await page.keyboard.press('Escape');await page.getByRole('button',{name:label('下载投影'),exact:true}).click();await page.locator('.library-window-dialog').waitFor();await shot('online');await page.locator('.library-reden-panel input').first().fill('fixture');await page.locator('.library-reden-panel input').first().press('Enter');await page.locator('.reden-result-card').first().waitFor();await page.locator('.reden-result-card').first().click();await page.locator('.reden-attachment-row').waitFor();await shot('online-detail');await page.locator('.reden-attachment-row button').first().click();await page.locator('.subwindow-status-text-error').waitFor();await shot('online-download-error');await page.keyboard.press('Escape');});
        if(compact)await step('small-category-overlay-preserves-grid',async()=>{await page.getByRole('button',{name:label('网格视图'),exact:true}).click();const before=await box('.library-card-grid');await page.locator('.library-categories-toggle').click();assert.equal(await page.locator('#library-categories').isVisible(),true);assert.deepEqual(await box('.library-card-grid'),before);await shot('categories');await page.locator('.library-categories-close').click();assert.equal(await page.locator('#library-categories').isVisible(),false);});
        await step('all-category-actions-icon-sources-and-cancel',async()=>{
          if(compact)await page.locator('.library-categories-toggle').click();
          const start=await page.evaluate(()=>window.__novaUX.calls.length);
          await page.locator('.library-category-tools button').click();await page.locator('#library-category-name').fill('取消的新分类 Cancelled new category');await shot('category-create');await page.keyboard.press('Escape');
          for(let index=0;index<5;index++){
            await page.locator('[data-category-tree-node]').first().click({button:'right'});await page.locator('.library-category-menu [role="menuitem"]').nth(index).click();await page.locator('[role="dialog"]').waitFor();
            if(index===3){
              for(let tab=0;tab<3;tab++){await page.locator('.library-icon-tabs button').nth(tab).click();await shot('category-icon-'+tab);}
              await page.locator('#library-icon-file').setInputFiles({name:'invalid.txt',mimeType:'text/plain',buffer:Buffer.from('isolated invalid icon')});await page.locator('[role="dialog"] [role="alert"]').waitFor();await shot('category-icon-invalid');
            }else await shot('category-action-'+index);
            await page.keyboard.press('Escape');
          }
          assert.equal(await page.evaluate(start=>window.__novaUX.calls.slice(start).filter(c=>c.command==='library_content_command'&&c.args.request.action!=='query').length,start),0);
          for(const id of ['library.ux.favorites','library.ux.recents','library.uncategorized','library.ux.allLibrary']){await page.locator('.library-categories').getByRole('button',{name:key(id),exact:false}).first().click();}
          if(compact)await page.locator('.library-categories-close').click();
        });
        await step('library-search-clear-native-import-cancel-and-drop',async()=>{
          const search=page.locator('.library-search input');await search.fill('高塔');await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='library_index_query'));await page.locator('.library-search button').click();assert.equal(await search.inputValue(),'');
          const before=await page.evaluate(()=>window.__novaUX.calls.length);await page.getByRole('button',{name:key('library.importSchematic'),exact:true}).click();
          assert.equal(await page.evaluate(start=>window.__novaUX.calls.slice(start).some(c=>c.command==='library_import_files'),before),false);
          const dt=await page.evaluateHandle(()=>{const transfer=new DataTransfer();const file=new File(['isolated data'],'Fixture.litematic',{type:'application/octet-stream'});Object.defineProperty(file,'path',{value:'X:/Nova-UX-fixture/drop.litematic'});transfer.items.add(file);return transfer;});
          await page.locator('.library-page').dispatchEvent('drop',{dataTransfer:dt});await page.getByRole('dialog',{name:key('library.ux.dropTitle')}).waitFor();await shot('drop-import');await page.keyboard.press('Escape');
        });
        await step('dispatch-draft-and-remove-confirmation-cancel',async()=>{
          await first.locator('.library-tile-open').click();await page.locator('.library-inspector').waitFor();
          await page.locator('.library-inspector').getByRole('button',{name:key('library.copyToMenu'),exact:true}).click();await page.locator('.library-dispatch-table').waitFor();
          const edit=page.locator('.library-dispatch-table input').first();await edit.fill('仍未提交的名称 Draft not yet submitted');await shot('dispatch-draft');await page.keyboard.press('Escape');
          await page.locator('.library-inspector').getByRole('button',{name:key('library.moveToCategoryMenu'),exact:true}).click();await page.locator('[role="dialog"]').waitFor();await shot('entry-category');await page.keyboard.press('Escape');
          await page.locator('.library-inspector').getByRole('button',{name:key('library.moveToTrash'),exact:true}).click();await page.locator('[role="dialog"]').waitFor();await shot('entry-trash');await page.keyboard.press('Escape');await page.locator('.library-inspector').getByRole('button',{name:key('library.closeDetails'),exact:true}).click();
        });
        await step('thumbnail-generation-control-draft-readonly-and-error',async()=>{
          await first.locator('.library-tile-open').click();const generate=()=>page.locator('.library-inspector').getByRole('button',{name:key('library.previewGenerate'),exact:true});
          await page.evaluate(()=>{window.__novaUX.previewScenario='pending';});await generate().click();await page.locator('.library-progress').waitFor();
          for(const id of ['library.previewPause','library.previewResume','library.previewCancel'])await page.locator('.library-progress').getByRole('button',{name:key(id),exact:true}).click();
          await page.locator('.library-progress').waitFor({state:'hidden'});const controls=await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='cancel_preview_generation').map(c=>c.args.action));assert.deepEqual(controls.slice(-3),['pause','resume','cancel']);
          for(const mode of ['readonly','save-error','normal']){
            await page.evaluate(mode=>{window.__novaUX.previewScenario=mode;},mode);await generate().click();await page.locator('.preview-draft-image').waitFor();
            const save=page.getByRole('button',{name:key('library.previewSaveEmbedded'),exact:true});
            if(mode==='readonly')assert.equal(await save.isDisabled(),true);
            if(mode==='save-error'){await save.click();await page.locator('[role="dialog"] [role="alert"]').waitFor();}
            await shot('thumbnail-'+mode);await page.getByRole('button',{name:key(mode==='normal'?'library.previewSaveEmbedded':'library.previewDiscard'),exact:true}).click();await page.locator('.preview-draft-image').waitFor({state:'hidden'});
          }
          await page.evaluate(()=>{window.__novaUX.previewScenario='normal';});await page.locator('.library-inspector').getByRole('button',{name:key('library.closeDetails'),exact:true}).click();
        });
        await step('folder-mount-options-browser-and-remove-cancel',async()=>{
          await page.getByRole('button',{name:key('library.ux.management'),exact:true}).click();await page.locator('.library-management-popover').getByRole('button',{name:key('library.manageFolders'),exact:true}).click();
          const panel=page.locator('.library-folders-panel');await panel.waitFor();await panel.getByRole('button',{name:key('library.addFolder'),exact:true}).click();
          const folder=panel.locator('.library-card').first();await folder.waitFor();await folder.getByRole('combobox').click();assert.equal(await page.getByRole('option').count(),2);await shot('folder-type-options');await page.keyboard.press('Escape');
          assert.equal(await folder.getByRole('checkbox').count(),3);await folder.getByRole('button',{name:key('library.browseFolder'),exact:true}).click();await shot('folder-browser-empty');
          await panel.getByRole('button',{name:key('library.backToFolders'),exact:true}).click();
          await panel.locator('.library-card').first().getByRole('button',{name:key('library.remove'),exact:true}).click();await page.getByRole('button',{name:key('library.removeMount'),exact:true}).waitFor();await shot('folder-remove-confirm');await page.keyboard.press('Escape');await page.keyboard.press('Escape');await page.keyboard.press('Escape');
        });
        await step('missing-replica-picker-cancel-and-error',async()=>{
          await first.locator('.library-tile-open').click();const reposition=page.locator('.library-inspector').getByRole('button',{name:key('library.ux.reposition'),exact:true});await reposition.click();assert.equal(await reposition.isEnabled(),true);
          await page.evaluate(()=>{window.__novaUX.handlers['plugin:dialog|open']=()=>{throw Error('Isolated picker failure');};});await reposition.click();await page.locator('.library-message').filter({hasText:'Isolated picker failure'}).waitFor();await shot('reposition-picker-error');
          await page.evaluate(()=>{delete window.__novaUX.handlers['plugin:dialog|open'];});await page.locator('.library-inspector').getByRole('button',{name:key('library.closeDetails'),exact:true}).click();
        });
        await step('scan-force-refresh-errors-preserve-library',async()=>{
          await page.evaluate(()=>{const s=window.__novaUX;s.previousTaskSubmit=s.handlers.library_task_submit;s.handlers.library_task_submit=args=>{if(args.kind==='scan')throw Error('Isolated scan launch failure');return s.previousTaskSubmit(args);};});
          for(const id of ['library.ux.rescan','library.ux.forceRecheck','library.refreshLibrary']){await page.getByRole('button',{name:key('library.ux.management'),exact:true}).click();await page.locator('.library-management-popover').getByRole('button',{name:key(id),exact:true}).click();await page.locator('.library-message').filter({hasText:'Isolated scan launch failure'}).waitFor();await page.keyboard.press('Escape');}
          assert.equal(await page.locator('.library-tile').count(),16);await shot('scan-error');await page.evaluate(()=>{const s=window.__novaUX;s.handlers.library_task_submit=s.previousTaskSubmit;});
        });
        await step('internal-category-drag-preserves-external-import-boundary',async()=>{
          if(compact)await page.locator('.library-categories-toggle').click();
          const dt=await page.evaluateHandle(()=>new DataTransfer());await first.dispatchEvent('dragstart',{dataTransfer:dt});const category=page.locator('[data-category-tree-node]').last();await category.dispatchEvent('dragover',{dataTransfer:dt});await shot('category-drag-target');await category.dispatchEvent('drop',{dataTransfer:dt});await first.dispatchEvent('dragend',{dataTransfer:dt});
          assert.equal(await page.getByRole('dialog',{name:key('library.ux.dropTitle')}).count(),0);if(compact)await page.locator('.library-categories-close').click();
        });
        await step('preview-read-not-repeated-by-selection',async()=>{await page.getByRole('button',{name:label('网格视图'),exact:true}).click();const count=await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_data_query'&&c.args.input.kind==='thumbnail').length);await first.locator('.library-tile-open').click();await first.locator('.library-tile-open').click();await page.waitForTimeout(120);assert.equal(await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_data_query'&&c.args.input.kind==='thumbnail').length),count);});
      }else if(route==='materials'){
        const row=page.locator('.material-catalog-row').first();
        await step('catalog-56px-select-and-clear',async()=>{assert.equal((await row.boundingBox()).height,56);await row.click();await page.waitForFunction(()=>document.querySelector('.material-catalog-workspace')?.getAttribute('data-detail-open')==='true');await shot('detail');if(await page.locator('.material-catalog-back').isVisible())await page.locator('.material-catalog-back').click();else{await row.click();await page.waitForFunction(()=>document.querySelector('.material-catalog-row')?.getAttribute('aria-pressed')==='false');}await row.click();await page.keyboard.press('Escape');await page.waitForFunction(()=>document.querySelector('.material-catalog-row')?.getAttribute('aria-pressed')==='false');});
        await step('catalog-native-query-parameters',async()=>{await page.locator('.material-catalog-toolbar input').fill('石头');await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_data_query'&&c.args.input.kind==='materials'&&c.args.input.input.search==='石头'));assert.equal(await page.evaluate(()=>window.__novaUX.calls.some(c=>c.command==='application_stockpile_task_command')),false);});
        await step('catalog-recipe-choice-tree-and-source-pagination',async()=>{
          await row.click();await page.locator('.material-catalog-detail').getByText(key('stockpile.recipeAndMaterials'),{exact:true}).click();await page.locator('.stockpile-recipe-node').first().waitFor();
          const choice=page.getByRole('combobox',{name:key('statistics.catalogRecipeChoice'),exact:true});await choice.click();await page.getByRole('option').last().click();
          await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_data_query'&&c.args.input.kind==='materialDetail'&&c.args.input.input.selectedRecipe==='fixture-recipe'));
          const toggle=page.locator('.stockpile-recipe-heading button').first();await toggle.click();assert.equal(await page.locator('.stockpile-recipe-node').count(),1);await toggle.click();
          await page.locator('.material-source-groups .material-catalog-detail-group summary').click();await page.locator('.material-catalog-detail-group .stockpile-pagination button').last().click();
          await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_data_query'&&c.args.input.kind==='materialDetail'&&c.args.input.input.sourceQuery?.offset===100));await shot('recipe-and-source');await page.keyboard.press('Escape');
        });
      }else{
        const first=page.locator('.stockpile-row').first();
        await step('40px-row-detail-toggle-stable-table',async()=>{const table=await box('.stockpile-table'),row=await first.boundingBox();assert.equal(row.height,40);await first.locator('.stockpile-cell-name').click();await page.locator('.stockpile-material-inspector').waitFor();assert.deepEqual(await box('.stockpile-table'),table);await shot('inspector');await first.locator('.stockpile-cell-name').click();assert.equal(await page.locator('.stockpile-material-inspector').count(),0);assert.equal(await first.getAttribute('aria-selected'),'false');});
        await step('quantity-escape-discards-draft',async()=>{const input=first.locator('input');const initial=await input.inputValue();const start=await page.evaluate(()=>window.__novaUX.calls.length);await input.fill('7');await input.press('Escape');assert.equal(await input.inputValue(),initial);await page.waitForTimeout(250);assert.equal(await page.evaluate(start=>window.__novaUX.calls.slice(start).filter(c=>c.command==='application_stockpile_task_command'&&c.args.input.action==='mutate').length,start),0);});
        await step('batch-checkbox-quantity-and-escape',async()=>{const table=await box('.stockpile-table');await first.getByRole('checkbox').click();assert.deepEqual(await box('.stockpile-table'),table);assert.equal(await page.locator('.stockpile-material-inspector').count(),0);const input=first.locator('input');await input.fill('7');assert.equal(await page.locator('.stockpile-material-inspector').count(),0);await first.focus();await page.keyboard.press('Escape');assert.equal(await first.getByRole('checkbox').getAttribute('aria-checked'),'false');await shot('batch');});
        await step('menu-escape-does-not-clear-selection',async()=>{await first.getByRole('checkbox').click();await first.locator('.stockpile-row-actions button').last().click();await page.locator('[role="menu"]').waitFor();await page.keyboard.press('Escape');assert.equal(await first.getByRole('checkbox').getAttribute('aria-checked'),'true');await first.focus();await page.keyboard.press('Escape');});
        await step('source-overlay-and-settings',async()=>{const before=await box('.stockpile-table');await page.locator('.stockpile-task-sources-trigger').click();await page.locator('.stockpile-task-sources').waitFor();assert.deepEqual(await box('.stockpile-table'),before);await page.keyboard.press('Escape');await page.locator('.stockpile-header-actions button').filter({hasText:compact?'Task settings':'任务设置'}).click();await page.locator('.stockpile-settings-dialog').waitFor();await shot('settings');await page.keyboard.press('Escape');});
        await step('filter-sort-option-sources',async()=>{const combos=page.locator('.stockpile-search-filters [role="combobox"]');for(let i=0;i<await combos.count();i++){await combos.nth(i).click();assert.ok(await page.locator('[role="option"]').count()>1);await page.keyboard.press('Escape');}assert.equal(await page.locator('.stockpile-material-inspector').count(),0);});
        await step('source-picker-cancel-has-no-mutation',async()=>{const start=await page.evaluate(()=>window.__novaUX.calls.length);await page.locator('.stockpile-title-group button').filter({hasText:compact?'Manage schematics':'管理投影'}).click();await page.locator('.stockpile-sources-dialog').waitFor();await shot('sources');await page.keyboard.press('Escape');assert.equal(await page.evaluate(start=>window.__novaUX.calls.slice(start).filter(c=>c.command==='application_stockpile_task_command'&&['setSources','addSources','create'].includes(c.args.input.action)).length,start),0);});
        await step('task-rename-reset-delete-cancel',async()=>{const start=await page.evaluate(()=>window.__novaUX.calls.length);await page.locator('.stockpile-title-group button').filter({hasText:compact?'Rename':'重命名'}).click();await page.locator('[role="dialog"] input').fill('取消的名称 Cancelled name');await page.keyboard.press('Escape');const more=page.locator('.stockpile-header-actions > button').last();for(const label of compact?['Reset progress…','Delete current task…']:['重置备货进度…','删除当前任务…']){await more.click();await page.getByRole('menuitem',{name:label,exact:true}).click();await page.locator('[role="dialog"]').waitFor();await page.keyboard.press('Escape');}assert.equal(await page.evaluate(start=>window.__novaUX.calls.slice(start).filter(c=>c.command==='application_stockpile_task_command'&&(c.args.input.action==='rename'||c.args.input.action==='delete'||c.args.input.mutation?.reset)).length,start),0);});
        await step('quantity-and-complete-send-native-command',async()=>{await first.locator('.stockpile-row-actions button').first().click();await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_stockpile_task_command'&&c.args.input.action==='mutate'));const calls=await page.evaluate(()=>window.__novaUX.calls.filter(c=>c.command==='application_stockpile_task_command'&&c.args.input.action==='mutate'));assert.ok(calls.every(c=>c.args.input.taskId==='task-1'&&c.args.input.sourceRevision==='fixture-rev-1'));});
        await step('stockpile-note-storage-units-recipe-and-source-details',async()=>{
          await first.locator('.stockpile-cell-name').click();const inspector=page.locator('.stockpile-material-inspector');await inspector.waitFor();
          await inspector.locator('.stockpile-detail-fields input').fill('A-12 / 高层箱组 Long storage reference');await inspector.locator('.stockpile-detail-fields textarea').fill('保留输入草稿\nA multiline note with long text '+ 'material '.repeat(12));
          await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_stockpile_task_command'&&c.args.input.mutation?.publicNote));
          for(const b of await inspector.locator('.stockpile-quick-add > button').all())await b.click();
          if(await page.locator('[role="menu"]').count()){await shot('stockpile-unit-menu');await page.keyboard.press('Escape');}
          for(const summary of await inspector.locator('details > summary').all())await summary.click();
          await inspector.getByRole('button',{name:key('stockpile.recipeAndMaterials'),exact:true}).click();await inspector.locator('.stockpile-recipe-node').first().waitFor();await shot('stockpile-details-expanded');await page.keyboard.press('Escape');
        });
        await step('batch-actions-and-backup-dialog-cancel',async()=>{
          const selectAll=page.locator('.stockpile-table thead [role="checkbox"]');await selectAll.click();assert.equal(await page.locator('.stockpile-row [role="checkbox"][aria-checked="true"]').count(),4);const slot=await box('.stockpile-batch-slot');assert.ok(slot.y>=0&&slot.y+slot.height<=height);assert.equal(await page.locator('.stockpile-task-controls .stockpile-batch-slot').count(),0);await shot('batch-actions');
          await page.locator('.stockpile-batch-toolbar button').first().click();await page.waitForTimeout(250);await page.locator('.stockpile-batch-toolbar button').last().click();
          const start=await page.evaluate(()=>window.__novaUX.calls.length);
          for(const id of ['stockpile.exportBackup','stockpile.importBackup']){await page.locator('.stockpile-header-actions > button').last().click();await page.getByRole('menuitem',{name:key(id),exact:true}).click();}
          assert.equal(await page.evaluate(start=>window.__novaUX.calls.slice(start).filter(c=>c.command==='application_stockpile_task_command'&&['import','export'].includes(c.args.input.action)).length,start),0);
        });
        await step('source-picker-options-copies-remove-create-and-confirm',async()=>{
          await page.locator('.stockpile-title-group button').filter({hasText:key('stockpile.sources.manage')}).click();const dialog=page.locator('.stockpile-sources-dialog');await dialog.waitFor();
          await dialog.locator('.stockpile-source-copies').fill('3');await dialog.locator('.stockpile-source-scope [role="combobox"]').first().click();await page.getByRole('option').last().click();await dialog.locator('.stockpile-source-scope input').fill('2');await dialog.locator('.stockpile-source-scope [role="checkbox"]').click();await shot('sources-edited');
          await dialog.locator('.dialog-footer button').last().click();await page.waitForFunction(()=>window.__novaUX.calls.some(c=>c.command==='application_stockpile_task_command'&&c.args.input.action==='setSources'&&c.args.input.sources[0]?.copies===3));
          await page.locator('.stockpile-title-group button').filter({hasText:key('stockpile.create')}).click();await dialog.waitFor();await dialog.locator('.stockpile-source-name input').fill('取消新任务 Cancelled new task');await dialog.locator('.stockpile-source-table button').last().click();await shot('sources-create-empty');await page.keyboard.press('Escape');
        });
        await step('complete-export-schema-and-reset',async()=>{
          await page.locator('.stockpile-header-actions button').filter({hasText:compact?'Export':'导出'}).first().click();await page.locator('.stockpile-export-dialog').waitFor();
          const select=async(key,value)=>{await page.locator('#material-export-'+key).click();await page.locator('[role="option"][data-state]').filter({hasText:value}).first().click();await page.waitForFunction(()=>!document.querySelector('.stockpile-export-dialog [role="status"]'));};
          await select('format','HTML');await select('webMode',compact?'Collaborative':'多人');await page.locator('#material-export-accessPassword').waitFor();await page.locator('#material-export-accessPassword').fill('temporary-ui-fixture');await page.locator('#material-export-adminPassword').fill('temporary-ui-fixture');await page.locator('#material-export-whitelist').fill('Fixture player');assert.equal(await page.locator('.material-export-targets [role="checkbox"]').count(),4);await shot('export-complete');
          await page.locator('.stockpile-export-dialog .dialog-footer button').first().click();await page.waitForFunction(()=>!document.querySelector('#material-export-accessPassword'));await page.keyboard.press('Escape');
        });
        await step('configured-export-save-cancel-no-output',async()=>{await page.locator('.stockpile-header-actions button').filter({hasText:compact?'Export':'导出'}).first().click();await page.locator('.stockpile-export-dialog').waitFor();await shot('export');const start=await page.evaluate(()=>window.__novaUX.calls.length);await page.locator('.stockpile-export-dialog .dialog-footer button').last().click();assert.equal(await page.evaluate(start=>window.__novaUX.calls.slice(start).filter(c=>c.command==='application_export_material_list'&&c.args.input.action==='export').length,start),0);await page.keyboard.press('Escape');});
        await step('export-all-source-format-scope-field-shapes',async()=>{
          await page.locator('.stockpile-header-actions button').filter({hasText:key('stockpile.openExport')}).first().click();await page.locator('.stockpile-export-dialog').waitFor();
          const choose=async(id,index)=>{await page.locator('#material-export-'+id).click();await page.getByRole('option').nth(index).click();await page.waitForFunction(()=>!document.querySelector('.stockpile-export-dialog [role="status"]'));};
          for(let source=0;source<3;source++){
            await choose('source',source);await page.locator('#material-export-format').click();const count=await page.getByRole('option').count();await page.keyboard.press('Escape');
            for(let format=0;format<count;format++){await choose('format',format);await shot('export-source-'+source+'-format-'+format);}
          }
          await choose('source',0);await choose('scope',1);await page.locator('#material-export-region').waitFor();await shot('export-region');await choose('scope',2);await page.locator('#material-export-layer').fill('-64');await shot('export-layer');await page.keyboard.press('Escape');
        });
        await step('recipe-preparation-error-and-cancel-receipt',async()=>{
          for(const mode of ['error','pending']){
            await page.evaluate(mode=>{window.__novaUX.recipeScenario=mode;},mode);await page.locator('.stockpile-header-actions button').filter({hasText:key('stockpile.openExport')}).first().click();await page.locator('.stockpile-export-dialog').waitFor();
            await page.getByRole('button',{name:key('stats.export.prepareRecipes'),exact:true}).click();
            if(mode==='error'){await page.locator('.stockpile-export-dialog [role="alert"]').waitFor();await shot('export-recipe-error');}
            else {await page.getByRole('button',{name:key('statistics.preparingRecipes'),exact:true}).waitFor();await shot('export-recipe-busy');}
            await page.keyboard.press('Escape');await page.locator('.stockpile-export-dialog').waitFor({state:'hidden'});
            if(mode==='pending')assert.ok(await page.evaluate(()=>window.__novaUX.calls.some(c=>c.command==='application_data_operation_status'&&c.args.cancel&&c.args.operationId===window.__novaUX.recipeOperation)));
          }
        });
        await step('legacy-bind-and-recovery-draft-cancel',async()=>{
          await page.evaluate(()=>{const s=window.__novaUX;s.stockpileWorkspace.legacyTasks=[{taskId:'legacy-1',preparedCount:16,preparingCount:0,itemCount:1,updatedAt:1700000000}];localStorage.setItem('nova.update-recovery.v1',JSON.stringify({'stockpile.pending:main:task-1':{taskId:'task-1',projectId:'project-1',sourceRevision:'older-fixture-revision',commands:[['item:minecraft:stone',{action:'mutate',mutation:{materialId:'minecraft:stone',quantity:7}}]]}}));});
          await page.locator('.stockpile-header-actions button').filter({hasText:key('stockpile.taskSettings')}).click();await page.locator('.stockpile-settings-dialog').getByRole('button',{name:key('stockpile.syncSnapshot'),exact:true}).click();await page.keyboard.press('Escape');
          await page.getByRole('button',{name:key('stockpile.bindLegacy'),exact:true}).click();await shot('legacy-bind');await page.keyboard.press('Escape');
          await page.getByRole('button',{name:key('stockpile.reviewRecovery'),exact:true}).click();await shot('draft-recovery');await page.keyboard.press('Escape');assert.ok(await page.evaluate(()=>JSON.parse(localStorage.getItem('nova.update-recovery.v1'))['stockpile.pending:main:task-1']));
        });
      }
      scenario.unknown=await page.evaluate(()=>window.__novaUX.unknown);if(scenario.errors.length)report.failures.push({route,theme,name:'page-errors',errors:scenario.errors});
    }catch(e){scenario.failure=e.message;report.failures.push({route,theme,name:'entry',error:e.message});await shot('entry-failure').catch(()=>{});}
    report.scenarios.push(scenario);await context.close();
  }
  fs.writeFileSync(path.join(outputDir,'library-stockpile.json'),JSON.stringify(report,null,2));return report;
}
