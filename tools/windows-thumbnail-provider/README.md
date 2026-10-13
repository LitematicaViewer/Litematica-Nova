# Windows cache-only thumbnail provider

Native x64 COM DLL for `.litematic` thumbnails. It implements `IThumbnailProvider`, `IInitializeWithStream` and optional `IInitializeWithFile`; it never renders or parses projection data, loads a renderer, starts a process, writes a thumbnail, or changes the registry.

- CLSID: `{95331DAE-48DC-4BBD-A4E0-E0880D365870}`.
- Installer-owned `InprocServer32` registration uses `ThreadingModel=Apartment`; the file type thumbnail handler identifier is `{E357FCCD-A995-4576-B01F-234630154E96}`. Do **not** set `DisableProcessIsolation`. The DLL has no self-registration exports.
- Microsoft specifies `IInitializeWithStream` as the initialization interface used for [default out-of-process thumbnail isolation](https://learn.microsoft.com/en-us/windows/win32/api/thumbcache/nn-thumbcache-ithumbnailprovider). The optional file initializer is for direct third-party callers.
- Cache path: `%LOCALAPPDATA%\Litematica-Nova\thumbnails\<lowercase-SHA256-of-original-litematic-bytes>.png`, resolved using the current user's known LocalAppData folder. The stream is hashed from offset zero in 64 KiB chunks; the original stream position is preserved. No metadata or file name supplies the digest.
- A missing cache returns `HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)` without an image. Corrupt, non-PNG, oversized or unreadable cache data returns the appropriate error, not an invented preview.
- WIC decodes the actual cached PNG. The returned top-down 32-bit PBGRA DIB preserves aspect ratio and alpha, downscales to fit `cx` and never upscales or pads. These follow the [official GetThumbnail bitmap contract](https://learn.microsoft.com/en-us/windows/win32/api/thumbcache/nf-thumbcache-ithumbnailprovider-getthumbnail).

## CLI build and isolated COM unit smoke

```powershell
& .\tools\windows-thumbnail-provider\build.ps1 -Smoke
```

Requires Visual Studio 2022 C++ BuildTools and the Windows SDK. The script discovers BuildTools with `vswhere -products *`, builds x64 Release with static CRT `/MT`, and leaves:

- `tools/windows-thumbnail-provider/target/release/litematica_thumbnail.dll`
- `tools/windows-thumbnail-provider/target/release/thumbnail_smoke.exe`

The smoke directly loads this DLL in its own console process; it does not register it or launch Explorer/GUI. It exercises real COM interfaces, a 131,077-byte raw stream and a clearly identified synthetic PNG unit fixture: cache miss, cache hit, aspect/alpha, no upscale, corrupt cache, preserved stream position, optional file initialization, parameter errors, canonical IUnknown identity, class factory locking and unload lifetime. Unit cache/source files are created only at previously absent known fixture paths and removed afterward. It does not claim a real projection screenshot or Explorer acceptance.

Actual installer association, Windows out-of-process activation and Explorer refresh/display remain manual integration acceptance. Source/render-cache production is owned by Nova's separately tested capture service.
