# 在 third_party/vscode-nbt 中构建 editor.js 并复制到 resources/web/nbt-viewer/
# 需已安装 Node.js（npm、npx 可用）。
$ErrorActionPreference = "Stop"
$env:Path = "$env:ProgramFiles\nodejs;$env:Path"
$root = Split-Path -Parent $PSScriptRoot
Set-Location "$root\third_party\vscode-nbt"
$patch = Join-Path $root "tools\patches\vscode-nbt-lba-editor.patch"
if (Test-Path $patch) {
	git apply --check $patch 2>$null
	if ($LASTEXITCODE -eq 0) {
		git apply $patch
	} else {
		git apply --reverse --check $patch 2>$null
		if ($LASTEXITCODE -ne 0) {
			Write-Error "无法应用或校验 LitematicaBA 的 vscode-nbt 补丁：$patch"
		}
	}
}
npm ci
npx rollup --config
$pkg = "$root\src\litematicaba\resources\web\nbt-viewer\editor.js"
Copy-Item -Force "out\editor.js" $pkg
Write-Host "OK: editor.js -> $pkg"
$dataOut = "$root\data\minecraft-assets\nbt-viewer\editor.js"
if (Test-Path (Split-Path $dataOut -Parent)) {
    Copy-Item -Force "out\editor.js" $dataOut
    Write-Host "OK: editor.js -> $dataOut (外置热更新)"
}
