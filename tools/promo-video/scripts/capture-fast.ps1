[CmdletBinding()]
param([string]$Repository=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path,[string]$SourceName='nova-observatory.litematic',[string]$OutputName='hero-fast.png',[int]$CaptureWidth=1920,[int]$CaptureHeight=1440)
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public static class NovaCaptureHost {
 [StructLayout(LayoutKind.Sequential)]public struct Msg{public IntPtr hwnd;public uint message;public UIntPtr wparam;public IntPtr lparam;public uint time;public int x;public int y;public uint priv;}
 [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)]public static extern IntPtr CreateWindowExW(uint ex,string cls,string name,uint style,int x,int y,int width,int height,IntPtr parent,IntPtr menu,IntPtr instance,IntPtr param);
 [DllImport("user32.dll")]public static extern bool DestroyWindow(IntPtr hwnd);
 [DllImport("user32.dll")]public static extern bool SetWindowPos(IntPtr hwnd,IntPtr after,int x,int y,int width,int height,uint flags);
 [DllImport("user32.dll")]public static extern IntPtr GetParent(IntPtr hwnd);
 [DllImport("user32.dll")]public static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
 [DllImport("user32.dll")]public static extern bool PostMessageW(IntPtr hwnd,uint message,IntPtr wparam,IntPtr lparam);
 [DllImport("user32.dll")]static extern bool PeekMessage(out Msg msg,IntPtr hwnd,uint min,uint max,uint remove);
 [DllImport("user32.dll")]static extern bool TranslateMessage(ref Msg msg);
 [DllImport("user32.dll")]static extern IntPtr DispatchMessage(ref Msg msg);
 [DllImport("user32.dll")]public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
 public static void Pump(){Msg m;while(PeekMessage(out m,IntPtr.Zero,0,0,1)){TranslateMessage(ref m);DispatchMessage(ref m);}}
}
'@
[void][NovaCaptureHost]::SetProcessDpiAwarenessContext([intptr](-4))
$hwnd=[NovaCaptureHost]::CreateWindowExW(0x08000000,'STATIC','Nova controlled offscreen capture',2248146944,0,0,$CaptureWidth,$CaptureHeight,[intptr]::Zero,[intptr]::Zero,[intptr]::Zero,[intptr]::Zero)
if($hwnd -eq [intptr]::Zero){throw 'Hidden capture host creation failed'}
$candidate=Join-Path $Repository '.tmp/release-beta040/windows-candidate';$backend=Join-Path $candidate 'bin/viewer-backend';$assets=Join-Path $Repository 'tools/promo-video/assets/original';$id='promo-fast-'+[guid]::NewGuid().ToString('N');$runtime=Join-Path $Repository ('.tmp/promo-capture/'+$id);$session=Join-Path $runtime 'session';New-Item -ItemType Directory -Path $session -Force|Out-Null
$envKeys=@('NOVA_GAME_DATA_SNAPSHOT','NOVA_USER_DATA_ROOT','NOVA_PROTECTED_TICKET_PATH','NOVA_PROTECTED_MODULE_PATH','LITEMATICA_NOVA_VIEWER_SESSION_DIRECTORY','LITEMATICA_NOVA_VIEWER_SESSION_ID');$before=@{};foreach($key in $envKeys){$before[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$viewer=$null;$sequence=0
function Write-Command($kind,$extra=@{}) {
 $script:sequence++
 $cmd=@{schema='litematica-nova.viewer-session-command.v2';sessionId=$id;sequence=$script:sequence;kind=$kind};foreach($k in $extra.Keys){$cmd[$k]=$extra[$k]}
 $path=Join-Path $session ('command-{0:d20}.json' -f $script:sequence);$tmp=$path+'.tmp';[IO.File]::WriteAllText($tmp,($cmd|ConvertTo-Json -Depth 12 -Compress));Move-Item -LiteralPath $tmp -Destination $path
 return $script:sequence
}
function Wait-CaptureFile([string]$Path,[int]$Seconds=90){$started=[DateTime]::UtcNow;while(!(Test-Path -LiteralPath $Path)){[NovaCaptureHost]::Pump();if($viewer.HasExited){throw 'Published Fast Viewer exited; private diagnostics retained.'};if(([DateTime]::UtcNow-$started).TotalSeconds -gt $Seconds){throw 'Controlled capture did not complete; no product timeout was changed.'};Start-Sleep -Milliseconds 16}}
try {
 $dataRoot=Join-Path $candidate 'pack-in/game-data/default';$package=Get-Content -LiteralPath (Join-Path $dataRoot 'manifest.json') -Raw|ConvertFrom-Json;$canonicalData='\\?\'+[IO.Path]::GetFullPath($dataRoot)
 $env:NOVA_GAME_DATA_SNAPSHOT=@{schema_version=1;id=$package.id;game_version=$package.game_version;content_id=$package.content_id;root=$canonicalData;resource_jar=(Join-Path $canonicalData 'resources.jar');source='bundled-default'}|ConvertTo-Json -Compress
 $env:NOVA_USER_DATA_ROOT=Join-Path $env:LOCALAPPDATA 'Litematica-Nova';$env:NOVA_PROTECTED_TICKET_PATH=$null;$env:NOVA_PROTECTED_MODULE_PATH=$null;$env:LITEMATICA_NOVA_VIEWER_SESSION_DIRECTORY=$session;$env:LITEMATICA_NOVA_VIEWER_SESSION_ID=$id
 $contract=@{schema='lba.viewer.launch.v1';requestId=$id;scene=(Join-Path $assets $SourceName);content=@{fabricModsDir=$null;resourcePacks=@()};renderer=$null;task=@{kind='quick_preview';initialVisible=$false;graphicsBackend='auto';preload=$true;host=@{hwnd=$hwnd.ToInt64().ToString();processId=$PID}}}
 $launch=Join-Path $runtime 'capture.launch.json';[IO.File]::WriteAllText($launch,($contract|ConvertTo-Json -Depth 12 -Compress))
 $viewer=Start-Process -FilePath (Join-Path $backend 'litematica_native_viewer.exe') -ArgumentList ('"--launch-contract='+$launch+'"') -WorkingDirectory $candidate -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $runtime 'viewer.stdout.log') -RedirectStandardError (Join-Path $runtime 'viewer.stderr.log')
 Wait-CaptureFile (Join-Path $session 'status-00000000000000000000.json')
 $ready=Get-Content -LiteralPath (Join-Path $session 'status-00000000000000000000.json') -Raw|ConvertFrom-Json
 $childHwnd=[intptr]([long]$ready.hwnd);[uint32]$childPid=0;[void][NovaCaptureHost]::GetWindowThreadProcessId($childHwnd,[ref]$childPid)
 if($childPid -ne $viewer.Id -or [NovaCaptureHost]::GetParent($childHwnd) -ne $hwnd){throw 'Capture resize must target only this script-owned child.'}
 # The normal presentation guard intentionally refuses to Show/Resize beneath
 # a hidden parent. Resize this script-owned child without showing either HWND.
 if(![NovaCaptureHost]::SetWindowPos($childHwnd,[intptr]::Zero,0,0,$CaptureWidth,$CaptureHeight,20)){throw 'Controlled capture child resize failed'}
 $settle=[DateTime]::UtcNow;while(([DateTime]::UtcNow-$settle).TotalMilliseconds -lt 350){[NovaCaptureHost]::Pump();Start-Sleep -Milliseconds 16}
 $attached=Write-Command 'attach_embedded' @{hostHwnd=$hwnd.ToInt64().ToString();width=$CaptureWidth;height=$CaptureHeight}
 Wait-CaptureFile (Join-Path $session ('status-{0:d20}.json' -f $attached))
 $captured=Write-Command 'capture_frame';$image=Join-Path $session ('capture-{0:d20}.png' -f $captured);Wait-CaptureFile $image
 Copy-Item -LiteralPath $image -Destination (Join-Path $assets $OutputName) -Force
 [pscustomobject]@{output=$OutputName;producer='Nova 0.4.0 released Fast NativeViewer';mode='Fast';capture='real GPU CaptureFrame';width=$CaptureWidth;height=$CaptureHeight;visibleHost=$false;bytes=(Get-Item -LiteralPath (Join-Path $assets $OutputName)).Length}|ConvertTo-Json -Compress
} finally {
 if($viewer -and !$viewer.HasExited){[void](Write-Command 'release');$stop=[DateTime]::UtcNow;while(!$viewer.HasExited -and ([DateTime]::UtcNow-$stop).TotalSeconds -lt 5){[NovaCaptureHost]::Pump();Start-Sleep -Milliseconds 20};if(!$viewer.HasExited){Stop-Process -Id $viewer.Id -Force}}
 [void][NovaCaptureHost]::DestroyWindow($hwnd)
 foreach($key in $envKeys){[Environment]::SetEnvironmentVariable($key,$before[$key],'Process')}
}
