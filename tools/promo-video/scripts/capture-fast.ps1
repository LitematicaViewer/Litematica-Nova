[CmdletBinding()]
param([string]$Repository=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path,[string]$SourceName='nova-observatory.litematic',[string]$OutputName='hero-fast.png',[int]$CaptureWidth=1920,[int]$CaptureHeight=1440,
 [string]$CandidateRoot,[string]$OutputDirectory,[string]$UserDataRoot,[string]$SessionPath,[switch]$VerifyNativeOverlay)
$ErrorActionPreference='Stop'
$candidate=if($CandidateRoot){(Resolve-Path -LiteralPath $CandidateRoot).Path}else{Join-Path $Repository '.tmp/release-beta040/windows-candidate'}
$backend=Join-Path $candidate 'bin/viewer-backend';$assets=Join-Path $Repository 'tools/promo-video/assets/original'
if(!$OutputDirectory){$OutputDirectory=$assets};$OutputDirectory=[IO.Path]::GetFullPath($OutputDirectory)
if([IO.Path]::GetFileName($OutputName) -ne $OutputName){throw 'OutputName must be a file name'}
$id='promo-fast-'+[guid]::NewGuid().ToString('N')
$runtime=if($SessionPath){[IO.Path]::GetFullPath($SessionPath)}else{Join-Path $OutputDirectory $id}
if(Test-Path -LiteralPath $runtime){throw 'Capture session must be a new task-owned directory'}
$session=Join-Path $runtime 'session';New-Item -ItemType Directory -Path $session -Force|Out-Null
if(!$UserDataRoot){$UserDataRoot=Join-Path $runtime 'user-data'};$UserDataRoot=[IO.Path]::GetFullPath($UserDataRoot)
if($UserDataRoot.TrimEnd('\') -ieq (Join-Path $env:LOCALAPPDATA 'Litematica-Nova').TrimEnd('\')){throw 'Capture requires isolated user data'}
New-Item -ItemType Directory -Path $OutputDirectory,$UserDataRoot -Force|Out-Null
$destination=Join-Path $OutputDirectory $OutputName
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public static class NovaCaptureHost {
 [StructLayout(LayoutKind.Sequential)]public struct Msg{public IntPtr hwnd;public uint message;public UIntPtr wparam;public IntPtr lparam;public uint time;public int x;public int y;public uint priv;}
 [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)]public static extern IntPtr CreateWindowExW(uint ex,string cls,string name,uint style,int x,int y,int width,int height,IntPtr parent,IntPtr menu,IntPtr instance,IntPtr param);
 [DllImport("user32.dll")]public static extern bool DestroyWindow(IntPtr hwnd);
 [DllImport("user32.dll")]public static extern bool SetWindowPos(IntPtr hwnd,IntPtr after,int x,int y,int width,int height,uint flags);
 [DllImport("user32.dll")]public static extern IntPtr GetParent(IntPtr hwnd);
 [DllImport("user32.dll")]public static extern IntPtr GetWindow(IntPtr hwnd,uint cmd);
 [DllImport("user32.dll")]public static extern bool GetLayeredWindowAttributes(IntPtr hwnd,out uint key,out byte alpha,out uint flags);
 [DllImport("user32.dll")]public static extern bool IsWindowVisible(IntPtr hwnd);
 [DllImport("user32.dll")]public static extern bool IsWindowEnabled(IntPtr hwnd);
 [DllImport("user32.dll")]public static extern int GetWindowRgn(IntPtr hwnd,IntPtr region);
 [DllImport("gdi32.dll")]public static extern IntPtr CreateRectRgn(int left,int top,int right,int bottom);
 [DllImport("gdi32.dll")]public static extern bool PtInRegion(IntPtr region,int x,int y);
 [DllImport("gdi32.dll")]public static extern bool DeleteObject(IntPtr obj);
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
$envKeys=@('NOVA_GAME_DATA_SNAPSHOT','NOVA_USER_DATA_ROOT','NOVA_PROTECTED_TICKET_PATH','NOVA_PROTECTED_MODULE_PATH','LITEMATICA_NOVA_VIEWER_SESSION_DIRECTORY','LITEMATICA_NOVA_VIEWER_SESSION_ID');$before=@{};foreach($key in $envKeys){$before[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$viewer=$null;$sequence=0;$childHwnd=[intptr]::Zero;$releaseRequested=$false;$forced=$false;$capturedImage=$false;$overlayVerified=$false;$hiddenChecks=0
function Assert-Hidden {
 if([NovaCaptureHost]::IsWindowVisible($hwnd) -or ($childHwnd -ne [intptr]::Zero -and [NovaCaptureHost]::IsWindowVisible($childHwnd))){throw 'Capture host or child became visible'}
 $script:hiddenChecks++
}
function Write-Command($kind,$extra=@{}) {
 $script:sequence++
 $cmd=@{schema='litematica-nova.viewer-session-command.v2';sessionId=$id;sequence=$script:sequence;kind=$kind};foreach($k in $extra.Keys){$cmd[$k]=$extra[$k]}
 $path=Join-Path $session ('command-{0:d20}.json' -f $script:sequence);$tmp=$path+'.tmp';[IO.File]::WriteAllText($tmp,($cmd|ConvertTo-Json -Depth 12 -Compress));Move-Item -LiteralPath $tmp -Destination $path
 return $script:sequence
}
function Wait-CaptureFile([string]$Path,[int]$Seconds=90){$started=[DateTime]::UtcNow;while(!(Test-Path -LiteralPath $Path)){[NovaCaptureHost]::Pump();if($viewer.HasExited){throw 'Published Fast Viewer exited; private diagnostics retained.'};if(([DateTime]::UtcNow-$started).TotalSeconds -gt $Seconds){throw 'Controlled capture did not complete; no product timeout was changed.'};Start-Sleep -Milliseconds 16}}
try {
 Assert-Hidden
 $dataRoot=Join-Path $candidate 'pack-in/game-data/default';$package=Get-Content -LiteralPath (Join-Path $dataRoot 'manifest.json') -Raw|ConvertFrom-Json;$canonicalData='\\?\'+[IO.Path]::GetFullPath($dataRoot)
 $env:NOVA_GAME_DATA_SNAPSHOT=@{schema_version=1;id=$package.id;game_version=$package.game_version;content_id=$package.content_id;root=$canonicalData;resource_jar=(Join-Path $canonicalData 'resources.jar');source='bundled-default'}|ConvertTo-Json -Compress
 $env:NOVA_USER_DATA_ROOT=$UserDataRoot;$env:NOVA_PROTECTED_TICKET_PATH=$null;$env:NOVA_PROTECTED_MODULE_PATH=$null;$env:LITEMATICA_NOVA_VIEWER_SESSION_DIRECTORY=$session;$env:LITEMATICA_NOVA_VIEWER_SESSION_ID=$id
 $contract=@{schema='lba.viewer.launch.v1';requestId=$id;scene=(Join-Path $assets $SourceName);content=@{fabricModsDir=$null;resourcePacks=@()};renderer=$null;task=@{kind='quick_preview';initialVisible=$false;graphicsBackend='auto';preload=$true;host=@{hwnd=$hwnd.ToInt64().ToString();processId=$PID}}}
 $launch=Join-Path $runtime 'capture.launch.json';[IO.File]::WriteAllText($launch,($contract|ConvertTo-Json -Depth 12 -Compress))
 $viewer=Start-Process -FilePath (Join-Path $backend 'litematica_native_viewer.exe') -ArgumentList ('"--launch-contract='+$launch+'"') -WorkingDirectory $candidate -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $runtime 'viewer.stdout.log') -RedirectStandardError (Join-Path $runtime 'viewer.stderr.log')
 Wait-CaptureFile (Join-Path $session 'status-00000000000000000000.json')
 $ready=Get-Content -LiteralPath (Join-Path $session 'status-00000000000000000000.json') -Raw|ConvertFrom-Json
 if($ready.sessionId -ne $id -or $ready.pid -ne $viewer.Id -or $ready.sequence -ne 0 -or $ready.state.kind -ne 'ready'){throw 'Ready receipt differs from this capture process'}
 $childHwnd=[intptr]([long]$ready.hwnd);[uint32]$childPid=0;[void][NovaCaptureHost]::GetWindowThreadProcessId($childHwnd,[ref]$childPid)
 if($childPid -ne $viewer.Id -or [NovaCaptureHost]::GetParent($childHwnd) -ne $hwnd){throw 'Capture resize must target only this script-owned child.'}
 Assert-Hidden
 # The normal presentation guard intentionally refuses to Show/Resize beneath
 # a hidden parent. Resize this script-owned child without showing either HWND.
 if(![NovaCaptureHost]::SetWindowPos($childHwnd,[intptr]::Zero,0,0,$CaptureWidth,$CaptureHeight,20)){throw 'Controlled capture child resize failed'}
 $attached=Write-Command 'attach_embedded' @{hostHwnd=$hwnd.ToInt64().ToString();width=$CaptureWidth;height=$CaptureHeight}
 Wait-CaptureFile (Join-Path $session ('status-{0:d20}.json' -f $attached))
 if($VerifyNativeOverlay){
  if($CaptureWidth -lt 96 -or $CaptureHeight -lt 96){throw 'Overlay probe needs at least 96 physical pixels per side'}
  foreach($blocked in @($true,$false)){
   $request="$id-overlay-$blocked";$feedbackPath=Join-Path $runtime "$request.json"
   $regions=@();if($blocked){$regions=@(@{x=16;y=16;width=32;height=32})}
   [void](Write-Command 'set_view' @{viewControls=@{uiRegions=$regions;uiBackdropOpacity=$(if($blocked){0.42}else{0.0});uiInputBlocked=$blocked;releaseCameraInput=$true;controlRequestId=$request;controlReceiptPath=$feedbackPath}})
   Wait-CaptureFile $feedbackPath
   $feedback=Get-Content -LiteralPath $feedbackPath -Raw|ConvertFrom-Json
   if($feedback.requestId -ne $request -or $null -ne $feedback.error -or $feedback.cameraLocked){throw 'Native overlay controls failed or pointer capture was retained'}
   [uint32]$actualPid=0;[void][NovaCaptureHost]::GetWindowThreadProcessId($childHwnd,[ref]$actualPid)
   if($actualPid -ne $viewer.Id -or [NovaCaptureHost]::GetParent($childHwnd) -ne $hwnd){throw 'Overlay controls changed the owned native child'}
   Assert-Hidden
   if([NovaCaptureHost]::IsWindowEnabled($childHwnd) -eq $blocked){throw 'Native modal input enable state differs from control'}
   if($blocked){
    $dimmer=[NovaCaptureHost]::GetWindow($childHwnd,5);[uint32]$colorKey=0;[byte]$alpha=0;[uint32]$flags=0
    if(!$dimmer -or ![NovaCaptureHost]::GetLayeredWindowAttributes($dimmer,[ref]$colorKey,[ref]$alpha,[ref]$flags) -or
       $alpha -ne [math]::Round(255*.42) -or ($flags -band 2) -eq 0 -or [NovaCaptureHost]::IsWindowEnabled($dimmer)){throw 'Expected disabled child dimmer with real 42% layer alpha'}
    # Posted messages exercise already-queued input; never inject into the OS or take focus.
    if(![NovaCaptureHost]::PostMessageW($childHwnd,0x100,[intptr]27,[intptr]0x00010001) -or
       ![NovaCaptureHost]::PostMessageW($childHwnd,0x101,[intptr]27,[intptr](-1073676287))){throw 'Posting Esc to the owned hidden child failed'}
    $fence="$request-fence";$fencePath=Join-Path $runtime "$fence.json"
    [void](Write-Command 'set_view' @{viewControls=@{controlRequestId=$fence;controlReceiptPath=$fencePath}})
    Wait-CaptureFile $fencePath
    if((Get-Content -Raw -LiteralPath $fencePath|ConvertFrom-Json).requestId -ne $fence){throw 'Wrong Viewer input fence receipt'}
    if(Get-ChildItem -LiteralPath $session -Filter 'event-*.json' | Where-Object {(Get-Content -Raw -LiteralPath $_.FullName|ConvertFrom-Json).state.kind -eq 'hidden'}){throw 'Queued Esc hid the Viewer while the modal owned input'}
   }
   $region=[NovaCaptureHost]::CreateRectRgn(0,0,0,0)
   try{
    if($region -eq [intptr]::Zero -or [NovaCaptureHost]::GetWindowRgn($childHwnd,$region) -eq 0){throw 'Native child clipping region missing'}
    if([NovaCaptureHost]::PtInRegion($region,24,24) -eq $blocked -or ![NovaCaptureHost]::PtInRegion($region,64,64)){throw 'Native overlay hole/restoration differs from requested region'}
   }finally{if($region -ne [intptr]::Zero){[void][NovaCaptureHost]::DeleteObject($region)}}
  }
  $overlayVerified=$true
 }
 $captured=Write-Command 'capture_frame';$image=Join-Path $session ('capture-{0:d20}.png' -f $captured);Wait-CaptureFile $image
 $captureStatus=Join-Path $session ('status-{0:d20}.json' -f $captured);Wait-CaptureFile $captureStatus
 $completed=Get-Content -LiteralPath $captureStatus -Raw|ConvertFrom-Json
 if($completed.sessionId -ne $id -or $completed.pid -ne $viewer.Id -or $completed.sequence -ne $captured -or $completed.state.kind -ne 'captured'){throw 'Actual frame did not complete for this capture request'}
 Assert-Hidden
 Copy-Item -LiteralPath $image -Destination $destination -Force
 $capturedImage=$true
} finally {
 try {
  if($viewer -and !$viewer.HasExited){
   try{[void](Write-Command 'release');$releaseRequested=$true}catch{Write-Warning "Release publication failed: $_"}
   $stop=[DateTime]::UtcNow;while(!$viewer.HasExited -and ([DateTime]::UtcNow-$stop).TotalSeconds -lt 5){[NovaCaptureHost]::Pump();Start-Sleep -Milliseconds 20}
   if(!$viewer.HasExited){$forced=$true;Stop-Process -InputObject $viewer -Force;[void]$viewer.WaitForExit(2000)}
  }
 } finally {
  [void][NovaCaptureHost]::DestroyWindow($hwnd)
  foreach($key in $envKeys){[Environment]::SetEnvironmentVariable($key,$before[$key],'Process')}
  $receipt=[pscustomobject]@{output=$destination;candidate=$candidate;sessionId=$id;sessionDirectory=$session;userDataRoot=$UserDataRoot;hostHwnd=$hwnd.ToInt64().ToString();viewerHwnd=$childHwnd.ToInt64().ToString();viewerPid=$(if($viewer){$viewer.Id}else{$null});hiddenChecks=$hiddenChecks;mode='Fast';capture='real GPU CaptureFrame';width=$CaptureWidth;height=$CaptureHeight;captured=$capturedImage;nativeOverlayVerified=$overlayVerified;finalWebViewCompositionVerified=$false;releaseRequested=$releaseRequested;forcedTermination=$forced;processExited=($viewer -and $viewer.HasExited);bytes=$(if($capturedImage){(Get-Item -LiteralPath $destination).Length}else{0})}
  $receipt|ConvertTo-Json -Depth 4|Set-Content (Join-Path $runtime 'capture-result.json') -Encoding utf8
 }
}
if(!$releaseRequested -or $forced -or !$viewer.HasExited -or $viewer.ExitCode -ne 0){throw 'Capture did not finish through its own clean Release'}
$released=Get-Content -LiteralPath (Join-Path $session ('status-{0:d20}.json' -f $sequence)) -Raw|ConvertFrom-Json
if($released.sessionId -ne $id -or $released.pid -ne $viewer.Id -or $released.sequence -ne $sequence -or $released.state.kind -ne 'released'){throw 'Wrong Release acknowledgement'}
$receipt|ConvertTo-Json -Compress
