import assert from 'node:assert/strict';
import test from 'node:test';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
const source=readFileSync(new URL('../ui/windows/main/useFullPreview.ts',import.meta.url),'utf8');
const normal={preflightId:'grant',severity:'normal',allowedActions:['continue_prebuilt']};
const warning={...normal,severity:'warning',allowedActions:['continue_prebuilt','switch_realtime','use_basic_vulkan','cancel']};
const hard={...warning,severity:'hard',allowedActions:['switch_realtime','use_basic_vulkan','cancel']};
const unmeasured={...warning,severity:'unmeasured',conservativeVramBytes:null,components:null,pressurePercent:null,allowedActions:['switch_realtime','use_basic_vulkan','cancel']};
const unavailable={...hard,severity:'invalid',allowedActions:['switch_realtime','cancel']};
const manualChoice={...unmeasured,allowedActions:['switch_realtime','use_basic_vulkan','cancel']};
const pathIdentity = {};
vm.runInNewContext(ts.transpileModule(readFileSync(new URL('../src/services/filePathIdentity.ts', import.meta.url), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText, { exports: pathIdentity });
const tick=()=>new Promise(done=>setImmediate(done));
test('属性和渲染页保留未知显存的真实原因，按路径显示规则处理且不重复通用说明',()=>{
 for(const [page,binding] of [['properties/PropertiesPage','fullPreview'],['render/RenderPage','full']]) {
  const text=readFileSync(new URL(`../ui/windows/main/pages/${page}.tsx`,import.meta.url),'utf8');
  const parsed=ts.createSourceFile(`${page}.tsx`,text,ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
  let expression;
  function visit(node){
   if(ts.isJsxExpression(node)&&node.expression&&ts.isBinaryExpression(node.expression)
    &&node.expression.left.getText(parsed)===`${binding}.preflight.severity === "unmeasured"`)expression=node.expression;
   ts.forEachChild(node,visit);
  }
  visit(parsed);assert(expression,`${page} must render the unmeasured assessment`);
  const compiled=ts.transpileModule(`const output=(${expression.getText(parsed)});`,{compilerOptions:{jsx:ts.JsxEmit.React,target:ts.ScriptTarget.ES2022}}).outputText;
  for(const reason of ['完整资源需求预估未完成：resource demand calculation exceeded its 10000ms planning deadline',null]) {
   const calls=[];
   const result=vm.runInNewContext(`${compiled};output`,{[binding]:{preflight:{severity:'unmeasured',invalidReason:reason}},
    React:{createElement:(type,props,...children)=>({type,props,children})},displayPathText:value=>{calls.push(value);return `显示：${value}`;}});
   assert.equal(result.type,'p');assert.equal(result.props.role,'alert');
   assert.equal(result.children.length,1);
   assert.equal(result.children[0],reason?`显示：${reason}`:'需求或当前预算尚未确认，预加载不可用，可选择实时加载。');
   assert.deepEqual(calls,reason?[reason]:[]);
  }
 }
});
function harness({preload=false,plan={directRealtime:true,preflight:null},deferredPlan,deferredStart,prepared=true,displayMode='full',initialOperation=null,viewportSize=[2316,1178]}={}){
 let props={source:'C:/scene.litematic',preload,blocked:false,displayMode},snapshot={operations:initialOperation?[initialOperation]:[]},cursor=0,slots=[],effects=[],output,next=0,requestedId=null;
 const calls={start:[],present:[],discard:[],cancel:[],basic:[],plan:[]};
 const hooks={useState(initial){const i=cursor++;if(!(i in slots))slots[i]={value:initial};return[slots[i].value,v=>{slots[i].value=typeof v==='function'?v(slots[i].value):v;}];},useRef(value){const i=cursor++;if(!(i in slots))slots[i]={current:value};return slots[i];},useSyncExternalStore(_subscribe,get){cursor++;return get();},useEffect(effect,deps){const i=cursor++,previous=slots[i];if(previous&&deps?.length===previous.deps?.length&&deps.every((v,j)=>Object.is(v,previous.deps[j])))return;slots[i]={deps,cleanup:previous?.cleanup};effects.push(()=>{previous?.cleanup?.();slots[i].cleanup=effect();});}};
 const facade={windowsPathKey:pathIdentity.windowsPathKey,subscribeApplicationRender:()=>()=>{},getApplicationRenderState:()=>snapshot,findRenderOperationForSource:(path,_snapshot,settings)=>[...snapshot.operations].reverse().find(op=>pathIdentity.windowsPathKey(op.source.path)===pathIdentity.windowsPathKey(path)&&(!settings||(op.displayMode===settings.displayMode&&op.buildMode===settings.buildMode)))||null,isApplicationViewerPrepared:op=>['viewer_prepared','realtime_ready'].includes(op?.state),isApplicationPresentationVisible:op=>op?.presentationState==='visible',isApplicationRenderPending:op=>op?.state==='running',ensureApplicationRenderSubscription:async()=>{},prepareApplicationRenderLaunch:async(...input)=>{calls.plan.push(input);return deferredPlan?await deferredPlan:plan;},resourcePreflightAllows:(grant,action)=>grant.allowedActions.includes(action),discardApplicationRenderPreflight:async id=>{calls.discard.push(id);},useBasicVulkanForApplicationRender:async id=>{calls.basic.push(id);return{...normal,preflightId:'basic'};},startApplicationRender:async input=>{calls.start.push(input);if(deferredStart)await deferredStart;const op={operationId:`op${++next}`,source:{path:input.sourcePath},displayMode:input.displayMode,buildMode:input.buildMode,state:prepared?'viewer_prepared':'running',presentationState:'hidden',presentation:{mode:'hidden',visible:false,placement:null}};snapshot={operations:[...snapshot.operations,op]};return op;},updateApplicationRenderPresentation:async(id,intent)=>{calls.present.push({id,intent});snapshot={operations:snapshot.operations.map(op=>op.operationId===id?{...op,presentation:intent}:op)};},cancelApplicationRender:async id=>{calls.cancel.push(id);}};
 const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,module={exports:{}};
 vm.runInNewContext(`(function(require,module,exports){${compiled}})`)(name=>name==='react'?hooks:name.endsWith('embeddedViewerGeometry')?{elementToNativeChildPlacement:async()=>({width:viewportSize[0],height:viewportSize[1],dpiScale:2})}:name.endsWith('useEmbeddedViewer')?{useEmbeddedViewer(op,blocked){
   hooks.useEffect(()=>{if(op&&requestedId===op.operationId&&facade.isApplicationViewerPrepared(op)&&!blocked)void facade.updateApplicationRenderPresentation(op.operationId,{mode:'embedded',visible:true,placement:{x:10,y:20,width:800,height:600,dpiScale:1,focused:false}});},[op?.operationId,op?.state,blocked]);
   hooks.useEffect(()=>()=>{if(op)void facade.updateApplicationRenderPresentation(op.operationId,{mode:'hidden',visible:false,placement:null});},[op?.operationId]);
   return {hostRef:{current:{}},error:'',requestPresentationWhenReady(id){requestedId=id;},showOverlay:async()=>{},setPresentation:async intent=>{if(op)await facade.updateApplicationRenderPresentation(op.operationId,intent);}};
  }}:facade,module,module.exports);
 function render(){cursor=0;effects=[];output=module.exports.useFullPreview(props.source,props.preload,props.blocked,props.displayMode);for(const effect of effects)effect();}
 async function settle(){await tick();render();await tick();render();}
 render();return{calls,get result(){return output;},settle,setProps(next){props={...props,...next};render();},patchOperation(patch){snapshot={operations:snapshot.operations.map(op=>({...op,...patch}))};render();},unmount(){for(const slot of slots)slot?.cleanup?.();},remount(){slots=[];requestedId=null;render();},ready(){snapshot={operations:snapshot.operations.map(op=>({...op,state:'viewer_prepared'}))};render();}};
}
test('小文件直接Realtime不带Prebuilt admission',async()=>{const h=harness();await h.result.start();await h.settle();assert.equal(h.calls.start[0].buildMode,'realtime');assert.equal(h.calls.start[0].preflightId,undefined);assert.equal(h.calls.present[0].intent.mode,'embedded');});
test('大文件低警线按勾选区分真正Prebuilt与冻结Realtime决策',async()=>{for(const preload of[false,true]){const h=harness({preload,plan:{directRealtime:false,preflight:normal}});await h.result.start();await h.settle();const start=h.calls.start[0];assert.equal(start.buildMode,preload?'prebuilt':'realtime');assert.equal(start.preflightId,preload?'grant':undefined);assert.equal(start.realtimeDecisionId,preload?undefined:'grant');}});
test('有效低压力直接开始，80%警告只在明确确认后预加载，未知及95%不放行预加载',async()=>{
 for(const grant of[normal,warning,hard,unmeasured])for(const preload of[false,true]){
  const h=harness({preload,plan:{directRealtime:false,preflight:grant}});await h.result.start();await h.settle();
  if(preload&&grant!==normal){
   assert.equal(h.calls.start.length,0);assert.equal(h.result.preflight.severity,grant.severity);
   if(grant===warning){await h.result.choose('prebuilt');await h.settle();assert.equal(h.calls.start[0].resourceWarningConfirmed,true);}
   else{await h.result.choose('prebuilt');await h.settle();assert.equal(h.calls.start.length,0);await h.result.choose('realtime');await h.settle();}
  }
  assert.equal(h.calls.start.length,1);assert.equal(h.calls.start[0].buildMode,preload&&(grant===normal||grant===warning)?'prebuilt':'realtime');
 }
});
test('真正来源或能力无效仍不启动预加载，保留明确加载与取消操作',async()=>{const h=harness({plan:{directRealtime:false,preflight:unavailable}});await h.result.start();await h.settle();await h.result.choose('prebuilt');await h.settle();assert.equal(h.calls.start.length,0);assert(h.result.error);await h.result.choose('realtime');await h.settle();assert.equal(h.calls.start[0].realtimeDecisionId,'grant');});
test('关闭Iris后的冻结决策不会重新读取默认配置',async()=>{const h=harness({preload:true,plan:{directRealtime:false,preflight:manualChoice}});await h.result.start();await h.settle();await h.result.choose('basic_vulkan');await h.settle();assert.equal(h.calls.start.length,0);await h.result.choose('realtime');await h.settle();assert.equal(h.calls.start[0].realtimeDecisionId,'basic');});
test('ViewerPrepared之前不展示Full窗口',async()=>{const h=harness({prepared:false});await h.result.start();await h.settle();assert.equal(h.calls.present.length,0);h.ready();await h.settle();assert(h.calls.present.some(call=>call.intent.visible));});
test('迟到显存预检只丢弃原决策，不启动新投影',async()=>{let done;const deferredPlan=new Promise(resolve=>{done=resolve;});const h=harness({deferredPlan});const pending=h.result.start();await tick();h.setProps({source:'C:/new.litematic'});done({directRealtime:false,preflight:warning});await pending;await h.settle();assert.equal(h.calls.start.length,0);assert.deepEqual(h.calls.discard,['grant']);});
test('离页之后迟到start只隐藏Presentation，不销毁Viewer/GPU',async()=>{let done;const deferredStart=new Promise(resolve=>{done=resolve;});const h=harness({deferredStart});const pending=h.result.start();await tick();h.unmount();done();await pending;assert.equal(h.calls.present.length,1);assert.equal(h.calls.present[0].intent.visible,false);assert.equal(h.calls.cancel.length,0);});
test('明确取消尚未收到operationId的预加载，接受后发实际Cancel而非只Hide',async()=>{
 let done;const deferredStart=new Promise(resolve=>{done=resolve;});
 const h=harness({preload:true,displayMode:'standard',prepared:false,plan:{directRealtime:false,preflight:normal},deferredStart});
 const start=h.result.start();await h.settle();assert.equal(h.result.pending,true);assert.equal(h.result.operation,null);
 await h.result.close();await h.settle();assert.equal(h.calls.cancel.length,0);
 done();await start;await h.settle();assert.deepEqual(h.calls.cancel,['op1']);
 assert.equal(h.calls.present.filter(call=>call.intent.visible).length,0);
});
test('明确取消当前上下文不取消已经离开的旧来源加载，旧来源只隐藏',async()=>{
 let done;const deferredStart=new Promise(resolve=>{done=resolve;});const h=harness({deferredStart});
 const start=h.result.start();await tick();h.setProps({source:'C:/new.litematic'});await h.result.close();
 done();await start;await h.settle();assert.equal(h.calls.cancel.length,0);
 assert.equal(h.calls.present.at(-1).intent.visible,false);
});
test('预检尚未完成时取消，不接受迟到grant也不启动构建',async()=>{
 let done;const deferredPlan=new Promise(resolve=>{done=resolve;});const h=harness({preload:true,deferredPlan});
 const start=h.result.start();await tick();await h.result.close();
 done({directRealtime:false,preflight:normal});await start;await h.settle();
 assert.equal(h.calls.start.length,0);assert.deepEqual(h.calls.discard,['grant']);
});
test('切回快速模式只隐藏Full Presentation，不释放runtime',async()=>{const h=harness();await h.result.start();await h.settle();h.setProps({blocked:true});await h.settle();assert.equal(h.calls.present.at(-1).intent.visible,false);assert.equal(h.calls.cancel.length,0);});

test('Standard冻结Unlit模式并与Full保持各自加载决策',async()=>{const h=harness({displayMode:'standard'});await h.result.start();await h.settle();assert.equal(h.calls.plan[0][2],'standard');assert.equal(h.calls.start[0].displayMode,'standard');h.setProps({displayMode:'full'});await h.settle();assert.equal(h.calls.present.at(-1).intent.visible,false);assert.equal(h.calls.cancel.length,0);await h.result.start();assert.equal(h.calls.start.at(-1).displayMode,'full');});


test('返回属性页不以初始化Hide覆盖route lease恢复',async()=>{
 const op={operationId:'retained',source:{path:'C:/scene.litematic'},displayMode:'full',buildMode:'realtime',state:'realtime_ready',presentation:{mode:'embedded',visible:true,placement:{x:10,y:20,width:800,height:600,dpiScale:1,focused:false}}};
 const h=harness({initialOperation:op});await h.settle();
 assert.equal(h.calls.start.length,0);assert.equal(h.calls.cancel.length,0);
 assert.equal(h.calls.present.filter(call=>!call.intent.visible).length,0);
});


test('同路径Standard切Full不会复挂旧mode，返回Standard复用其已有runtime',async()=>{
 const h=harness({displayMode:'standard'});await h.result.start();await h.settle();const standardId=h.result.operation.operationId;
 h.setProps({displayMode:'full'});await h.settle();assert.equal(h.result.operation,null);assert.equal(h.result.ready,false);assert(h.calls.present.some(call=>call.id===standardId&&!call.intent.visible));
 await h.result.start();await h.settle();const fullId=h.result.operation.operationId;assert.notEqual(fullId,standardId);
 h.setProps({displayMode:'standard'});await h.settle();assert.equal(h.result.operation.operationId,standardId);assert.equal(h.calls.cancel.length,0);
});

test('同路径预加载和Realtime资源不串用，用户显存决策允许明确覆盖复选框',async()=>{
 const h=harness();await h.result.start();await h.settle();const realtimeId=h.result.operation.operationId;
 h.setProps({preload:true});await h.settle();assert.equal(h.result.operation,null);assert.equal(h.result.ready,false);assert.equal(h.calls.cancel.length,0);
 const decision=harness({preload:true,plan:{directRealtime:false,preflight:manualChoice}});await decision.result.start();await decision.settle();await decision.result.choose('realtime');await decision.settle();
 assert.equal(decision.result.operation.buildMode,'realtime');assert.equal(decision.result.ready,true);assert(decision.calls.present.some(call=>call.intent.visible));assert.equal(h.calls.start.length,1);assert(realtimeId);
});


test('显存明确改选Realtime后返回页面保持真实加载契约，不以预加载勾选复挂其他资源',async()=>{
 const h=harness({preload:true,plan:{directRealtime:false,preflight:manualChoice}});await h.result.start();await h.settle();await h.result.choose('realtime');await h.settle();
 const id=h.result.operation.operationId;h.unmount();h.remount();await h.settle();assert.equal(h.result.operation.operationId,id);assert.equal(h.result.operation.buildMode,'realtime');assert.equal(h.calls.start.length,1);assert.equal(h.calls.cancel.length,0);
 h.setProps({preload:false});await h.settle();assert.equal(h.result.operation.operationId,id); // explicit checkbox now matches the same Realtime geometry contract
 h.setProps({preload:true});await h.settle();assert.equal(h.result.operation.operationId,id); // original explicit VRAM decision remains remembered
});

test('CPU完成47/47不代表画面可见，Prepared后仍等待正式Present回执',async()=>{
 const h=harness({prepared:false,displayMode:'standard'});await h.result.start();await h.settle();
 h.patchOperation({progress:{phase:'mesh_compile',completed:47,total:47},presentationState:'hidden'});
 assert.equal(h.result.ready,false);assert.equal(h.result.visible,false);assert.equal(h.calls.present.length,0);
 assert.equal(h.result.progressLabel,'正在准备画面…');
 h.patchOperation({state:'realtime_ready'});await h.settle();
 assert.equal(h.result.ready,true);assert.equal(h.result.visible,false);assert.equal(h.result.progressLabel,'正在显示画面…');
 assert(h.calls.present.some(call=>call.intent.visible));
 h.patchOperation({presentationState:'visible'});await h.settle();assert.equal(h.result.visible,true);
 h.patchOperation({presentationState:'hidden'});await h.settle();assert.equal(h.result.visible,false);
 await h.result.hide();await h.settle();assert.equal(h.result.progressLabel,'预览已隐藏');
 assert.equal(h.calls.start.length,1);assert.equal(h.calls.cancel.length,0);
});

test('正式BuildFailed结束等待并显示重新加载操作，不被47/47旧进度吞掉',async()=>{
 const h=harness({prepared:false,displayMode:'standard'});await h.result.start();await h.settle();
 h.patchOperation({state:'failed',progress:{phase:'failed',completed:47,total:47},error:'streamed prebuild entity diagnostics rejected',presentationState:'hidden'});
 await h.settle();assert.equal(h.result.loading,false);assert.equal(h.result.ready,false);assert.equal(h.result.visible,false);
 assert.equal(h.result.error,'预览加载失败，请重新加载。');assert.equal(h.calls.present.length,0);
 // Even a late post-Present field cannot replace the matching terminal task.
 h.patchOperation({presentationState:'visible'});await h.settle();
 assert.equal(h.result.visible,false);assert.equal(h.result.error,'预览加载失败，请重新加载。');
});

test('关闭与释放终态不把旧0/21458继续显示为加载，保留重新启动',async()=>{
 for(const state of ['cancelled','runtime_released']) {
  const h=harness({prepared:false,displayMode:'standard'});await h.result.start();await h.settle();
  h.patchOperation({state,progress:{phase:state,completed:0,total:21458},presentationState:'hidden'});
  await h.settle();assert.equal(h.result.loading,false);assert.equal(h.result.ready,false);
  assert.equal(h.result.progress,undefined);assert.equal(h.result.progressLabel,'预览已关闭，可重新加载。');
  await h.result.start();await h.settle();assert.equal(h.calls.start.length,2);assert.equal(h.result.loading,true);
 }
});

test('取消等待使用真实取消阶段，不显示旧区块计数',async()=>{
 const h=harness({prepared:false});await h.result.start();await h.settle();
 h.patchOperation({state:'running',progress:{phase:'cancelling',completed:48,total:21458}});
 assert.equal(h.result.loading,true);assert.equal(h.result.progress,undefined);
 assert.equal(h.result.progressLabel,'正在取消预览…');
});

test('标准预加载使用正式非空气构建进度，不把完整目录当工作计数',async()=>{
 const h=harness({prepared:false,preload:true,displayMode:'standard',plan:{directRealtime:false,preflight:normal}});
 await h.result.start();await h.settle();
 h.patchOperation({progress:{phase:'compile_existing_cells',completed:1737,total:21458}});
 assert.equal(h.result.progressLabel,'正在构建 8%（1,737 / 21,458）');
 assert.equal(h.result.progress.total,21458);assert.equal(h.result.ready,false);
});

test('共享光照使用真实阶段和光照区段计数，完成后几何阶段独立从零开始',async()=>{
 const h=harness({preload:true,prepared:false,plan:{directRealtime:false,preflight:normal}});
 await h.result.start();await h.settle();
 for(const phase of ['prepare_shared_lighting']) {
  h.patchOperation({progress:{phase,activity:'light_sections',completed:0,total:8006,elapsedMs:29000}});
  assert.equal(h.result.progressLabel,'正在求解共享光照（0 / 8,006 光照区段）…');
  assert.equal(h.result.progressRange,null,'尚无已完成光照区段时使用不定进度');
  h.patchOperation({progress:{phase,activity:'light_sections',completed:1,total:8006,elapsedMs:29500}});
  assert.equal(h.result.progressLabel,'正在求解共享光照（1 / 8,006 光照区段，<1%）');
  assert.equal(h.result.progressRange.completed,1);assert.equal(h.result.progressRange.total,8006);
  h.patchOperation({progress:{phase,activity:'light_sections',completed:12,total:48,elapsedMs:30000}});
  assert.equal(h.result.progressLabel,'正在求解共享光照（12 / 48 光照区段，25%）');
  assert.equal(h.result.progressRange.completed,12);assert.equal(h.result.progressRange.total,48);
  assert.equal(h.result.ready,false);
  h.patchOperation({progress:{phase,activity:'light_sections',completed:48,total:48,elapsedMs:31000}});
  assert.equal(h.result.progressLabel,'正在求解共享光照（48 / 48 光照区段，100%）');
 }
 h.patchOperation({progress:{phase:'compile_existing_cells',activity:'sections',completed:0,total:8006,elapsedMs:31001}});
 assert.equal(h.result.progressLabel,'正在构建 0%（0 / 8,006）');
 assert.equal(h.result.progressRange.completed,0);assert.equal(h.result.progressRange.total,8006);
 assert.equal(h.calls.start.length,1,'阶段变化不能启动第二个任务');
});

test('资源准备的局部1/1不冒充画面就绪或全局百分比',async()=>{
 const h=harness({preload:true,prepared:false,plan:{directRealtime:false,preflight:normal}});
 await h.result.start();await h.settle();
 for(const [phase,label] of [
  ['load_litematic','正在读取投影'],['normalize_canonical_ir','正在整理投影'],
  ['load_mc26_catalog','正在读取方块目录'],['build_mc26_schemas','正在准备方块状态'],
  ['load_resource_packs','正在加载资源'],['bind_world','正在关联场景资源'],['load_biome_tint','正在准备生物群系颜色'],
 ]) for(const completed of [0,1]) {
  h.patchOperation({progress:{phase,activity:'stage_steps',completed,total:1}});
  assert.equal(h.result.progressLabel,`${label}…`);
  assert.equal(h.result.progressRange,null);assert.equal(h.result.ready,false);
 }
 h.patchOperation({progress:{phase:'artifact_publishing',completed:1,total:1}});
 assert.equal(h.result.progressLabel,'正在准备画面…');assert.equal(h.result.progressRange,null);
});

test('共享光照未知或非法分母保持不定进度，不借用其他阶段计数',async()=>{
 const h=harness({preload:true,prepared:false,plan:{directRealtime:false,preflight:normal}});
 await h.result.start();await h.settle();
 for(const [activity,completed,total] of [
  ['light_sections',0,null],['light_sections',0,0],['light_sections',1,NaN],
  ['light_sections',1,Infinity],['light_sections',-1,48],['light_sections',49,48],
  ['light_sections',0.5,48],['stage_steps',1,1],['sections',0,8006],
 ]) {
  h.patchOperation({progress:{phase:'prepare_shared_lighting',activity,completed,total}});
  assert.equal(h.result.progressLabel,'正在求解共享光照…');assert.equal(h.result.progressRange,null);
 }
});

test('渲染页实际progress元素使用统一范围，未知阶段不画伪零进度',()=>{
 const text=readFileSync(new URL('../ui/windows/main/pages/render/RenderPage.tsx',import.meta.url),'utf8');
 const parsed=ts.createSourceFile('RenderPage.tsx',text,ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
 let element;
 function visit(node){if(ts.isJsxSelfClosingElement(node)&&node.tagName.getText(parsed)==='progress')element=node;ts.forEachChild(node,visit);}
 visit(parsed);assert(element);
 const compiled=ts.transpileModule(`const output=(${element.getText(parsed)});`,{compilerOptions:{jsx:ts.JsxEmit.React,target:ts.ScriptTarget.ES2022}}).outputText;
 for(const progressRange of [null,{completed:12,total:48}]) {
  const result=vm.runInNewContext(`${compiled};output`,{full:{progressRange},React:{createElement:(type,props)=>({type,props})}});
  assert.equal(result.type,'progress');assert.equal(result.props.max,progressRange?.total);
  assert.equal(result.props.value,progressRange?.completed);
 }
});

test('预加载构建完成到真实Prepared和Present只推进同一任务，不自动重复构建',async()=>{
 const h=harness({prepared:false,preload:true,displayMode:'standard',plan:{directRealtime:false,preflight:normal}});
 await h.result.start();await h.settle();const id=h.result.operation.operationId;
 for(const completed of [48,1737,21457]) {
  h.patchOperation({progress:{phase:'compile_existing_cells',completed,total:21458}});await h.settle();
  assert.equal(h.result.operation.operationId,id);assert.match(h.result.progressLabel,/^正在构建 /);
  assert.equal(h.result.ready,false);assert.equal(h.calls.present.length,0);
 }
 h.patchOperation({progress:{phase:'compile_existing_cells',completed:21458,total:21458}});await h.settle();
 assert.equal(h.result.progressLabel,'正在准备画面…');
 h.patchOperation({progress:{phase:'artifact_publishing'}});await h.settle();
 assert.equal(h.result.progressLabel,'正在准备画面…');assert.equal(h.result.ready,false);
 h.patchOperation({state:'viewer_prepared',progress:{phase:'viewer_prepared'}});await h.settle();
 assert.equal(h.result.progress,undefined);assert.equal(h.result.progressLabel,'正在显示画面…');
 h.patchOperation({presentationState:'visible'});await h.settle();
 assert.equal(h.result.visible,true);assert.equal(h.calls.start.length,1);assert.equal(h.calls.plan.length,1);
 assert.equal(h.calls.cancel.length,0);
});

test('Prepared后真实GPU失败保持失败终态，仅用户重新加载才创建新构建',async()=>{
 const h=harness({preload:true,displayMode:'standard',plan:{directRealtime:false,preflight:normal}});
 await h.result.start();await h.settle();h.patchOperation({presentationState:'visible'});await h.settle();
 h.patchOperation({state:'failed',progress:{phase:'failed'},error:'UnpresentableResolve',presentationState:'hidden'});
 await h.settle();await h.settle();
 assert.equal(h.result.visible,false);assert.equal(h.result.ready,false);assert.equal(h.result.progress,undefined);
 assert.equal(h.result.error,'预览加载失败，请重新加载。');assert.equal(h.calls.start.length,1);
 await h.result.start();await h.settle();assert.equal(h.calls.start.length,2);
});

test('Standard和Full预加载使用当前容器物理尺寸而非CSS或默认窗口值',async()=>{for(const displayMode of ['standard','full'])for(const preload of [false,true]){const h=harness({displayMode,preload,viewportSize:[2316,1178],plan:{directRealtime:false,preflight:normal}});await h.result.start();assert.deepEqual(Array.from(h.calls.plan[0][3]),[2316,1178]);assert.equal(h.calls.start.length,1);}});
test('缺失或非法物理视口不提交预检或构建',async()=>{for(const viewportSize of [[0,720],[1280,0],[16385,720],[1280,NaN]]){const h=harness({preload:true,viewportSize,plan:{directRealtime:false,preflight:normal}});await h.result.start();assert.equal(h.calls.plan.length,0);assert.equal(h.calls.start.length,0);assert(h.result.error || (await h.settle(),h.result.error));}});

test('冷预加载预估等待显示真实前置阶段，不沿用旧构建百分比或关闭状态',async()=>{
 for(const state of ['viewer_prepared','cancelled']) {
  let done;const deferredPlan=new Promise(resolve=>{done=resolve;});
  const initialOperation={operationId:'old',source:{path:'C:/scene.litematic'},displayMode:'standard',buildMode:'prebuilt',state,
   progress:{phase:'compile_existing_cells',completed:22638,total:45276},presentationState:'hidden',presentation:{mode:'hidden',visible:false,placement:null}};
  const h=harness({preload:true,displayMode:'standard',prepared:false,deferredPlan,initialOperation});
  const start=h.result.start();await h.settle();assert.equal(h.result.progressLabel,'正在预估显存…');assert.equal(h.result.progress,undefined);
  await h.result.close();done({directRealtime:false,preflight:normal});await start;await h.settle();
  assert.equal(h.calls.start.length,0);assert.notEqual(h.result.progressLabel,'正在预估显存…');
 }
});
