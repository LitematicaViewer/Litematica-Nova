# Nova DirectComposition surface capability patch

Upstream: gfx-rs `wgpu-hal` 0.19.5, crates.io package, MIT OR Apache-2.0.
Original license files remain beside this note. The renderer and dependency
versions are unchanged.

Original `src/dx12/adapter.rs` SHA256:
`5143D009AB75950DF6F6E311CA07108DEDD373B99029D0EAC9B882E4880893EA`.

The existing DX12 surface backend already maps `PreMultiplied` to the DXGI alpha
mode and creates composition swapchains for composition targets. The upstream
capability list nevertheless advertises only `Opaque` for every target.

Only DirectComposition Visual, composition SurfaceHandle and SwapChainPanel
targets now advertise `PreMultiplied` plus `Opaque`. Ordinary HWND targets remain
opaque-only. The application must create and retain a real composition target;
alpha-zero clearing on an ordinary HWND is not a transparent implementation.

The two unit tests exercise the capability policy without creating a window,
initializing a GPU or claiming visual acceptance. Native composition remains a
separate Presentation integration and manual acceptance item.

Primary API: [Microsoft CreateSwapChainForComposition](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgifactory2-createswapchainforcomposition).

DirectComposition swapchains use `FlipSequential` per the official CreateSwapChainForComposition contract; ordinary HWNDs retain FlipDiscard. Scaling remains Stretch. Pure null-pointer enum policy tests do not create windows or invoke DXGI.
Source: https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgifactory2-createswapchainforcomposition
