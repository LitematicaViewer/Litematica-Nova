$targets = @(
    "git",
    "git-remote-http",
    "git-remote-https",
    "git-remote-ftp",
    "git-lfs"
)

$intervalMs = 700

Write-Host "Killing git-related processes every $intervalMs ms. Press Ctrl+C to stop."

while ($true) {
    foreach ($name in $targets) {
        Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    }

    Start-Sleep -Milliseconds $intervalMs
}
