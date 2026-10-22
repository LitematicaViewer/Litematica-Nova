import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const icons = path.join(root, 'desktop-nova/src-tauri/icons');
const staging = path.join(root, '.tmp/brand-icons-generated');
const cli = path.join(root, 'desktop-nova/node_modules/@tauri-apps/cli/tauri.js');
// Keep the source artwork intact; the official Tauri converter preserves its
// proportions and transparency for every supported desktop bundle format.
const result = spawnSync(process.execPath, [cli, 'icon', path.join(icons, 'icon.svg'), '-o', staging], { stdio: 'inherit', windowsHide: true });
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
for (const name of ['32x32.png', '64x64.png', '128x128.png', '128x128@2x.png', 'icon.png', 'icon.ico', 'icon.icns']) {
  fs.copyFileSync(path.join(staging, name), path.join(icons, name));
}
