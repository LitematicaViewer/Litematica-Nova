param(
    [string]$VenvPath = (Join-Path $HOME ".venvs\litematica-ba-tools"),
    [string]$PythonExe = ""
)

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$requirementsPath = Join-Path $repoRoot "requirements.txt"

function Resolve-Python311 {
    param([string]$Preferred)

    if ($Preferred) {
        return (Resolve-Path -LiteralPath $Preferred).Path
    }

    $candidates = @(
        (Join-Path $HOME "AppData\Local\Programs\Python\Python311\python.exe"),
        (Join-Path $HOME "AppData\Local\Programs\Python\Python310\python.exe")
    )

    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }

    $command = Get-Command python -ErrorAction SilentlyContinue
    if ($command -and $command.Source -notlike "*WindowsApps*") {
        return $command.Source
    }

    throw "Python 3.11/3.10 not found. Install Python 3.11 first or pass -PythonExe."
}

$resolvedPython = Resolve-Python311 -Preferred $PythonExe

if (-not (Test-Path -LiteralPath $VenvPath)) {
    & $resolvedPython -m venv $VenvPath
}

$venvPython = Join-Path $VenvPath "Scripts\python.exe"

& $venvPython -m pip install --upgrade pip setuptools wheel
& $venvPython -m pip install -r $requirementsPath

Write-Output "[python-env] ready python=$venvPython requirements=$requirementsPath"
