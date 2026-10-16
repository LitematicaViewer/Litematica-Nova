import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

// This is a production beforeBuild step, not a developer-only DLL injection.
// Linux/macOS retain their existing package and thumbnail build paths.
if (process.platform === 'win32') {
  const script = fileURLToPath(new URL('../../tools/nsis-duilib/build.ps1', import.meta.url));
  const result = spawnSync('powershell.exe', ['-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', script], { stdio: 'inherit', windowsHide: true });
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
}
