# Contributing to Nova

The public repository builds a usable **Community** application without organization membership, private source, protected modules, download credentials, production signing keys, or a dedicated NovaCI machine.

## Prerequisites

Install Node.js 22, the native Rust toolchain, and the [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/). Windows additionally needs PowerShell 7, MSVC C++ tools and the Windows SDK; run PowerShell with the C++ environment configured. Linux needs GTK 3, WebKitGTK 4.1 and the platform libraries described in the Tauri guide. macOS needs Xcode Command Line Tools. Community Fast does not require MoltenVK.

## Develop

From the public repository root:

```powershell
npm ci --prefix desktop-nova
pwsh -NoProfile -File ./bin/dev-win.ps1
```

The PowerShell entry is preferred on Windows. On Linux/macOS use:

```powershell
npm run tauri:dev --prefix desktop-nova
```

Build the native Community installer/distribution:

```powershell
npm run tauri:build --prefix desktop-nova
```

Compile without packaging or opening a GUI:

```powershell
npm run tauri:build --prefix desktop-nova -- --compile-only
```

Community includes real public Core/Fast rendering and public application workflows: schematic parsing, properties, library, materials, layers, and the existing public editing features. Standard/Full are optional private capabilities and are not bundled. Selecting them reports that the build does not include the feature, rather than preventing startup or pretending to load a renderer. Community packages do not require official updater signing credentials and are not official releases.

Feature branches, forks, and legitimate new files/crates/features are welcome. Run checks relevant to your change and submit a pull request. Public/fork PR CI builds the real Community application on a GitHub-hosted runner without private secrets or privileged local runners. Do not commit generated binaries, personal data, private runtime modules, or credentials.

## Optional full-runtime development

Organization members should follow the private repository's full development and trusted signing instructions. The public entry retains explicit `--runtime-source prebuilt` for compatible signed components and `--runtime-source local --private-source <private-checkout>` for members building their own local development runtime. Neither option is required to contribute to or build the public application. AppVersion changes alone do not rebuild a compatible runtime; actual ABI/capability mismatches or missing requested modules fail explicitly.

[中文贡献说明](../CONTRIBUTING.md) · [Development reference](DEVELOPMENT.md)
