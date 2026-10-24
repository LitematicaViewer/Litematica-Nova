[CmdletBinding()]
param([string]$Repository=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path,[string]$SourceName='nova-observatory.litematic',[string]$OutputName='hero-standard.png')
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public static class NovaStandardCaptureHost {
 [StructLayout(LayoutKind.Sequential)]public struct Msg{public IntPtr hwnd;public uint message;public UIntPtr wparam;public IntPtr lparam;public uint time;public int x;public int y;public uint priv;}
 [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)]public static extern IntPtr CreateWindowExW(uint ex,string cls,string name,uint style,int x,int y,int width,int height,IntPtr parent,IntPtr menu,IntPtr instance,IntPtr param);
 [DllImport("user32.dll")]public static extern bool DestroyWindow(IntPtr hwnd);
 [DllImport("user32.dll")]static extern bool PeekMessage(out Msg msg,IntPtr hwnd,uint min,uint max,uint remove);
 [DllImport("user32.dll")]static extern bool TranslateMessage(ref Msg msg);
 [DllImport("user32.dll")]static extern IntPtr DispatchMessage(ref Msg msg);
 [DllImport("user32.dll")]public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
 public static void Pump(){Msg m;while(PeekMessage(out m,IntPtr.Zero,0,0,1)){TranslateMessage(ref m);DispatchMessage(ref m);}}
}
'@
[void][NovaStandardCaptureHost]::SetProcessDpiAwarenessContext([intptr](-4))
$hwnd=[NovaStandardCaptureHost]::CreateWindowExW(0x08000000,'STATIC','Nova Standard controlled capture',2248146944,0,0,1920,1440,[intptr]::Zero,[intptr]::Zero,[intptr]::Zero,[intptr]::Zero)
if($hwnd -eq [intptr]::Zero){throw 'Hidden Standard capture host creation failed'}
$candidate=Join-Path $Repository '.tmp/release-beta040/windows-candidate';$protected=Join-Path $candidate 'bin/viewer-backend/protected';$assets=Join-Path $Repository 'tools/promo-video/assets/original';$id='promo-standard-'+[guid]::NewGuid().ToString('N');$runtime=Join-Path $Repository ('.tmp/promo-capture/'+$id);New-Item -ItemType Directory -Path $runtime -Force|Out-Null
$config=Get-Content -LiteralPath (Join-Path $protected 'protected-build.json') -Raw|ConvertFrom-Json;$publicKey=Join-Path $runtime 'public-key.txt';[IO.File]::WriteAllText($publicKey,$config.manifestPublicKey)
$userRoot=Join-Path $env:LOCALAPPDATA 'Litematica-Nova';$lease=$null;$coordinator=$null
$keys=@('NOVA_GAME_DATA_SNAPSHOT','NOVA_USER_DATA_ROOT');$before=@{};foreach($key in $keys){$before[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
try {
 $raw=& (Join-Path $Repository 'tools/nova-protection/target/release/nova-private-client.exe') --endpoint $config.endpoint --build-id $config.buildId --module-id $config.moduleId --public-key-file $publicKey --identity-dir (Join-Path $userRoot 'protection/devices') --ciphertext (Join-Path $protected $config.ciphertextFile) --runtime-dir $runtime --task-id $id 2> (Join-Path $runtime 'materialize.stderr.log')
 if($LASTEXITCODE -ne 0){throw 'Standard task materialization failed; private diagnostics retained.'};$lease=($raw -join "`n")|ConvertFrom-Json
 $dataRoot=Join-Path $candidate 'pack-in/game-data/default';$package=Get-Content -LiteralPath (Join-Path $dataRoot 'manifest.json') -Raw|ConvertFrom-Json;$canonicalData='\\?\'+[IO.Path]::GetFullPath($dataRoot)
 $env:NOVA_GAME_DATA_SNAPSHOT=@{schema_version=1;id=$package.id;game_version=$package.game_version;content_id=$package.content_id;root=$canonicalData;resource_jar=(Join-Path $canonicalData 'resources.jar');source='bundled-default'}|ConvertTo-Json -Compress
 $env:NOVA_USER_DATA_ROOT=Join-Path $runtime 'userdata'
 $output=Join-Path $assets $OutputName
 if(Test-Path -LiteralPath $output){throw 'Standard output already exists; preserve the frozen media asset.'}
 $arguments=@(('"'+(Join-Path $PSScriptRoot 'capture-standard.py')+'"'),'--candidate',('"'+$candidate+'"'),'--runtime',('"'+$runtime+'"'),'--source',('"'+(Join-Path $assets $SourceName)+'"'),'--output',('"'+$output+'"'),'--ticket',('"'+$lease.ticket_path+'"'),'--module',('"'+$lease.module_path+'"'),'--parent-hwnd',$hwnd.ToInt64().ToString(),'--parent-pid',"$PID")
 $coordinator=Start-Process -FilePath (Get-Command python).Source -ArgumentList $arguments -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $runtime 'coordinator.stdout.log') -RedirectStandardError (Join-Path $runtime 'coordinator.stderr.log')
 while(!$coordinator.HasExited){[NovaStandardCaptureHost]::Pump();Start-Sleep -Milliseconds 16}
 if($coordinator.ExitCode -ne 0){throw 'Standard production capture failed; private diagnostics retained.'}
 Get-Content -LiteralPath (Join-Path $runtime 'capture-result.json') -Raw
} finally {
 [void][NovaStandardCaptureHost]::DestroyWindow($hwnd)
 foreach($key in $keys){[Environment]::SetEnvironmentVariable($key,$before[$key],'Process')}
 if($lease){
  $base=[IO.Path]::GetFullPath($runtime)+[IO.Path]::DirectorySeparatorChar
  foreach($name in @('module_path','ticket_path')){$path=[string]$lease.$name;if($path -and [IO.Path]::GetFullPath($path).StartsWith($base,[StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $path)){try{Remove-Item -LiteralPath $path -Force}catch{Write-Warning 'Temporary task material remains outside public assets while in use.'}}}
 }
}
