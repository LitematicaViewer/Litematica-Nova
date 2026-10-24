"""One controlled media capture through Nova 0.4's existing public IPC.
No rendering/geometry implementation, product modifications or user processes.
"""
import argparse, ctypes, json, msvcrt, os, queue, secrets, struct, subprocess, threading, time
from pathlib import Path

p=argparse.ArgumentParser()
for name in ['candidate','runtime','source','output','ticket','module','parent-hwnd','parent-pid']:p.add_argument('--'+name,required=True)
a=p.parse_args(); candidate=Path(a.candidate);runtime=Path(a.runtime);runtime.mkdir(parents=True,exist_ok=True)
backend=candidate/'bin/viewer-backend';pipe_name=r'\\.\pipe\nova-promo-standard-'+secrets.token_hex(12);auth=secrets.token_hex(24)
env=os.environ.copy();env.update(NOVA_RENDER_SERVICE_AUTH_TOKEN=auth,NOVA_PROTECTED_TICKET_PATH=a.ticket,NOVA_PROTECTED_MODULE_PATH=a.module,NOVA_HEADLESS_E2E='1')
env.pop('LITEMATICA_NOVA_NATIVE_PARENT_HWND',None);env.pop('LITEMATICA_NOVA_NATIVE_PARENT_PID',None)
service=subprocess.Popen([str(backend/'nova-render-service.exe'),'--pipe='+pipe_name,'--runtime-root='+str(runtime),'--parent-pid='+str(os.getpid())],env=env,cwd=candidate,stdout=open(runtime/'service.stdout.log','wb'),stderr=open(runtime/'service.stderr.log','wb'),creationflags=subprocess.CREATE_NO_WINDOW)
children={};stream=None;sequence=0;write_lock=threading.Lock();messages=queue.Queue();responses={};builds={};viewers={};files=[]
receive_buffer=bytearray()
kernel=ctypes.WinDLL('kernel32',use_last_error=True)
kernel.PeekNamedPipe.argtypes=[ctypes.c_void_p,ctypes.c_void_p,ctypes.c_ulong,ctypes.c_void_p,ctypes.POINTER(ctypes.c_ulong),ctypes.c_void_p]
u=ctypes.WinDLL('user32',use_last_error=True);u.GetParent.argtypes=[ctypes.c_void_p];u.GetParent.restype=ctypes.c_void_p
u.GetWindowThreadProcessId.argtypes=[ctypes.c_void_p,ctypes.POINTER(ctypes.c_ulong)];u.GetWindowThreadProcessId.restype=ctypes.c_ulong
u.SetWindowPos.argtypes=[ctypes.c_void_p,ctypes.c_void_p,ctypes.c_int,ctypes.c_int,ctypes.c_int,ctypes.c_int,ctypes.c_uint]
u.EnumChildWindows.argtypes=[ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p]
u.EnumWindows.argtypes=[ctypes.c_void_p,ctypes.c_void_p]
def send(kind,data):
 global sequence
 with write_lock:
  sequence+=1;b=json.dumps({'schema':'litematica-nova.render-service.v1','sequence':sequence,'payload':{'type':kind,'data':data}},separators=(',',':')).encode();stream.write(struct.pack('<I',len(b))+b)
def request(name,args=None):
 rid='capture-'+secrets.token_hex(8);command={'command':name}
 if args is not None:command['args']=args
 send('request',{'requestId':rid,'protectedTask':{'ticketPath':a.ticket,'modulePath':a.module},'command':command});return rid
def exact(n):
 b=b''
 while len(b)<n:
  c=stream.read(n-len(b))
  if not c:raise EOFError('capture service pipe closed')
  b+=c
 return b
def reader():
 try:
  while True:
   n=struct.unpack('<I',exact(4))[0]
   if n>16*1024*1024:raise ValueError('invalid capture service frame')
   messages.put(json.loads(exact(n)))
 except Exception as e:messages.put(e)
def spawn_reply(data):
 cmd=data['command'];spec=cmd['data'];identity=spec['identity'];role=spec['role'];key=identity['childId']
 if cmd['operation']=='spawn':
  allow={'build_worker':'nova-build-worker.exe','native_viewer':'litematica_native_viewer.exe','core_helper':'litematica_core.exe'}
  if role not in allow:raise RuntimeError('capture allowlist rejected child role')
  if key in children and children[key]['process'].poll() is None:child=children[key]['process'];disposition='reused'
  else:
   child_env=env.copy();child_env.update(spec['environment'])
   def output(target,kind):
    if target['target']=='null':return subprocess.DEVNULL
    path=Path(target['path']);path.parent.mkdir(parents=True,exist_ok=True);f=open(path,'ab' if target.get('append') else 'wb');files.append(f);return f
   child=subprocess.Popen([str(backend/allow[role]),*spec['args']],env=child_env,cwd=spec.get('currentDirectory') or candidate,stdout=output(spec['stdout'],'stdout'),stderr=output(spec['stderr'],'stderr'),creationflags=subprocess.CREATE_NO_WINDOW)
   children[key]={'process':child,'role':role,'identity':identity,'notified':False,'terminated':False};disposition='created'
  outcome={'status':'spawned','data':{'pid':child.pid,'disposition':disposition}}
 elif cmd['operation']=='terminate':
  old=children.get(key);exited=not old or old['process'].poll() is not None
  if old and not exited:old['terminated']=True;old['process'].terminate()
  outcome={'status':'terminated','data':{'alreadyExited':exited}}
 else:raise RuntimeError('unknown capture Host process command')
 send('host_process_response',{'requestId':data['requestId'],'role':role,'identity':identity,'outcome':outcome})
def pump(timeout=.05):
 available=ctypes.c_ulong()
 if not kernel.PeekNamedPipe(msvcrt.get_osfhandle(stream.fileno()),None,0,None,ctypes.byref(available),None):raise ctypes.WinError(ctypes.get_last_error())
 if available.value:receive_buffer.extend(stream.read(available.value))
 while len(receive_buffer)>=4:
  size=struct.unpack('<I',receive_buffer[:4])[0]
  if size>16*1024*1024:raise ValueError('invalid capture service frame')
  if len(receive_buffer)<size+4:break
  messages.put(json.loads(receive_buffer[4:4+size]));del receive_buffer[:4+size]
 for child in list(children.values()):
  code=child['process'].poll()
  if code is not None and not child['notified']:
   child['notified']=True;send('host_child_exited',{'role':child['role'],'identity':child['identity'],'pid':child['process'].pid,'exitCode':code,'terminationRequested':child['terminated']})
 try:m=messages.get(timeout=timeout)
 except queue.Empty:return
 if isinstance(m,Exception):raise m
 payload=m['payload'];kind=payload['type'];data=payload.get('data')
 if kind=='host_process_request':spawn_reply(data)
 elif kind=='response':
  responses[data['requestId']]=data['result']
  result=data['result'].get('Ok',{})
  if result.get('kind')=='snapshot':
   for build in result['data'].get('builds',[]):builds[build['buildId']]=build
   for viewer in result['data'].get('viewers',[]):viewers[viewer['sessionId']]=viewer
 elif kind=='event':
  if data['event']=='build_state':builds[data['data']['buildId']]=data['data']
  if data['event']=='viewer_state':viewers[data['data']['sessionId']]=data['data']
  if data['event']=='snapshot':
   for build in data['data'].get('builds',[]):builds[build['buildId']]=build
   for viewer in data['data'].get('viewers',[]):viewers[viewer['sessionId']]=viewer
  if data['event']=='error':raise RuntimeError('Published capture service reported an error; see private logs')
def wait_response(rid,seconds=180):
 until=time.monotonic()+seconds
 while rid not in responses:
  if time.monotonic()>until:raise TimeoutError('controlled media request did not complete')
  pump()
 result=responses.pop(rid)
 if 'Err' in result:raise RuntimeError(str(result['Err']))
 return result['Ok']
def resize_owned_viewer(pid):
 found=[];callback=ctypes.WINFUNCTYPE(ctypes.c_bool,ctypes.c_void_p,ctypes.c_void_p)
 @callback
 def child(hwnd,_):
  owner=ctypes.c_ulong();u.GetWindowThreadProcessId(hwnd,ctypes.byref(owner))
  if owner.value==pid and not u.GetParent(hwnd):found.append(hwnd)
  return True
 u.EnumWindows(child,None)
 for hwnd in found:
  if not u.SetWindowPos(hwnd,None,0,0,1920,1440,20):raise ctypes.WinError(ctypes.get_last_error())
 return bool(found)
try:
 until=time.monotonic()+30
 while stream is None:
  try:stream=open(pipe_name,'r+b',buffering=0)
  except OSError:
   if service.poll() is not None or time.monotonic()>until:raise RuntimeError('published render service did not open its private pipe')
   time.sleep(.03)
 # A single synchronous Windows pipe handle cannot have an outstanding
 # blocking ReadFile while its owner writes Hello. Poll available bytes in
 # this one coordinator loop instead of building a second transport service.
 send('hello',{'protocolVersion':21,'authToken':auth,'role':'application_host','instanceId':'promo-standard-host','pid':os.getpid(),'applicationOwnerEpoch':int(time.time()*1000)})
 start={'sourcePath':a.source,'displayMode':'standard','buildMode':'realtime','content':{'fabricModsDir':None,'resourcePacks':[],'assetInputs':[]},'buildOptions':{'displayMode':'standard'},'presentation':{'backend':'vulkan','shaderpack':None,'renderSettings':{'effectsProfile':'unlit','irisFallback':'fail','precipitation':'none','allowTimeModification':'false','allowWeatherModification':'false','width':'1920','height':'1440'}}}
 accepted=wait_response(request('start_build',{'build':start,'admission':None}))
 if accepted['kind']!='build_accepted':raise RuntimeError('capture did not receive an actual build identity')
 bid=accepted['data']['buildId'];sid=accepted['data']['viewerSessionId'];until=time.monotonic()+180;ready=None
 # The Service queues this normal user presentation intent until the first
 # real GPU Cell is ready. Existing headless media mode drives the identical
 # Popup frame loop without displaying a desktop window.
 wait_response(request('show_popup',{'session_id':sid}))
 next_snapshot=0
 while time.monotonic()<until:
  if time.monotonic()>=next_snapshot:request('get_snapshot');next_snapshot=time.monotonic()+.5
  pump();build=builds.get(bid,{});candidate_view=viewers.get(sid,{})
  if build.get('state')=='failed' or candidate_view.get('runtimeState')=='failed':raise RuntimeError('Standard production preparation failed; see private service logs')
  if build.get('state') in ['artifact_ready','build_completed'] and candidate_view.get('runtimeState')=='viewer_prepared':ready=candidate_view;break
 if ready is None:raise TimeoutError('Standard complete production preparation did not finish during media capture')
 resized=resize_owned_viewer(ready['pid']);until=time.monotonic()+.5
 while time.monotonic()<until:pump(.02)
 output=Path(a.output)
 if output.exists():raise RuntimeError('capture destination must be a new file')
 wait_response(request('capture_viewer',{'session_id':sid,'output_path':str(output)}))
 until=time.monotonic()+45
 while time.monotonic()<until:
  pump()
  if output.is_file() and output.stat().st_size>32 and b'IEND' in output.read_bytes()[-16:]:break
 else:raise TimeoutError('actual Standard CaptureFrame did not produce a complete PNG')
 b=output.read_bytes();w,h=struct.unpack('>II',b[16:24])
 result={'mode':'Standard','effectsProfile':'unlit','producer':'Nova 0.4.0 released RenderService + signed production Viewer','capture':'real production Session CaptureFrame','geometrySource':Path(a.source).name,'width':w,'height':h,'allSourceBuildCompleted':True,'nativeChildResized':resized,'visibleHost':False}
 (runtime/'capture-result.json').write_text(json.dumps(result,indent=2),encoding='utf-8');print(json.dumps(result))
 wait_response(request('release_viewer',{'session_id':sid}),15)
finally:
 if stream:
  try:request('shutdown')
  except Exception:pass
  until=time.monotonic()+3
  while service.poll() is None and time.monotonic()<until:
   try:pump(.03)
   except Exception:break
 for child in children.values():
  if child['process'].poll() is None:
   child['process'].terminate()
   try:child['process'].wait(3)
   except subprocess.TimeoutExpired:child['process'].kill()
 if service.poll() is None:service.terminate()
 try:service.wait(3)
 except subprocess.TimeoutExpired:service.kill()
 if stream:stream.close()
 for f in files:f.close()
