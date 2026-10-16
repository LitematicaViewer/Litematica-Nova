//! DirectComposition presentation for a viewer-owned native child HWND.
//! The existing wgpu DX12 composition surface owns the GPU swapchain; this module
//! only owns its target/visual and never captures a bitmap or reparents a window.

use std::ffi::c_void;
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionRectangleClip, IDCompositionTarget,
    IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GWL_STYLE, GetClientRect, GetWindowLongPtrW, GetWindowThreadProcessId, WS_CHILD,
    WS_EX_NOREDIRECTIONBITMAP,
};
use windows::core::{ComInterface, IUnknown, Interface};

/// Keep this alive until after the wgpu composition Surface has been dropped.
/// The HWND is borrowed and stays owned by the existing winit child window.
pub(crate) struct WindowsComposition {
    visual: IDCompositionVisual,
    clip: IDCompositionRectangleClip,
    target: IDCompositionTarget,
    device: IDCompositionDevice,
}

impl WindowsComposition {
    pub(crate) fn new(hwnd: isize) -> Result<Self> {
        ensure!(hwnd != 0, "DirectComposition needs a native child HWND");
        let hwnd = HWND(hwnd);
        let mut owner = 0;
        unsafe {
            ensure!(
                GetWindowThreadProcessId(hwnd, Some(&mut owner)) != 0,
                "DirectComposition target HWND is invalid"
            );
            ensure!(
                owner == GetCurrentProcessId(),
                "DirectComposition target must belong to the Viewer process"
            );
            ensure!(
                GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_CHILD.0 != 0,
                "DirectComposition embedded target must be WS_CHILD"
            );
            ensure!(
                GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_NOREDIRECTIONBITMAP.0 != 0,
                "DirectComposition child must disable the opaque HWND redirection bitmap"
            );

            // No D3D11 surface is created here. wgpu supplies the real DX12 DXGI
            // composition swapchain through IDCompositionVisual::SetContent.
            let device: IDCompositionDevice = DCompositionCreateDevice(None::<&IDXGIDevice>)
                .context("Create DirectComposition device")?;
            let target = device
                .CreateTargetForHwnd(hwnd, false)
                .context("Bind DirectComposition to native child HWND")?;
            let visual = device
                .CreateVisual()
                .context("Create DirectComposition visual")?;
            let clip = device
                .CreateRectangleClip()
                .context("Create child viewport clip")?;
            clip.SetLeft2(0.0).context("Set child clip left")?;
            clip.SetTop2(0.0).context("Set child clip top")?;
            visual
                .SetClip(&clip)
                .context("Attach child viewport clip")?;
            target
                .SetRoot(&visual)
                .context("Attach native child visual root")?;
            let composition = Self {
                visual,
                clip,
                target,
                device,
            };
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect).context("Read native child client extent")?;
            composition.resize(
                (rect.right - rect.left).max(0) as u32,
                (rect.bottom - rect.top).max(0) as u32,
                1.0,
            )?;
            Ok(composition)
        }
    }

    /// Borrowed COM pointer for wgpu SurfaceTargetUnsafe::CompositionVisual.
    /// wgpu-hal's DX12 ComPtr::from_raw adds its own COM reference. Do not forget
    /// or transfer this helper's reference, and do not pass the pointer to Vulkan.
    pub(crate) fn visual_ptr(&self) -> *mut c_void {
        self.visual.as_raw()
    }

    /// Commit after wgpu surface configure/resize attaches its DXGI content.
    pub(crate) fn commit(&self) -> Result<()> {
        unsafe {
            self.device
                .Commit()
                .context("Commit native child composition")
        }
    }

    /// Update only presentation clipping; retains the visual and GPU resources.
    /// Width/height are physical winit pixels, including DPI scaling already.
    /// A zero extent clips away content while minimized; callers must separately
    /// skip zero-sized wgpu Surface::configure. Scale is checked, not applied twice.
    pub(crate) fn resize(&self, width: u32, height: u32, dpi_scale: f64) -> Result<()> {
        validate_scale(dpi_scale)?;
        unsafe {
            self.clip
                .SetRight2(width as f32)
                .context("Resize native child clip width")?;
            self.clip
                .SetBottom2(height as f32)
                .context("Resize native child clip height")?;
        }
        self.commit()
    }
}

impl Drop for WindowsComposition {
    fn drop(&mut self) {
        // Surface must have gone first. Detach the tree/content before releasing
        // COM references, without destroying the borrowed HWND or any renderer.
        let detached = unsafe {
            self.target
                .SetRoot(None::<&IDCompositionVisual>)
                .and_then(|_| self.visual.SetContent(None::<&IUnknown>))
                .and_then(|_| self.device.Commit())
        };
        if let Err(error) = detached {
            eprintln!("[NOVA_COMPOSITION] detach failed: {error}");
        }
    }
}

/// Fast keeps its own wgpu renderer. Only its finished color attachment is
/// presented through a same-device imported texture; no geometry/core sharing.
pub(crate) struct VulkanComposition {
    // GPU aliases go before COM backing resources in destruction order.
    imported: wgpu::Texture,
    imported_view: wgpu::TextureView,
    target: Arc<wgpu::hal::vulkan::NativeCompositionImage>,
    source: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    shared: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    mutex: windows::Win32::Graphics::Dxgi::IDXGIKeyedMutex,
    context: windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    swapchain: windows::Win32::Graphics::Dxgi::IDXGISwapChain1,
    composition: WindowsComposition,
}

impl VulkanComposition {
    pub(crate) fn new(hwnd: isize, adapter: &wgpu::Adapter, device: &wgpu::Device,
        width: u32, height: u32, format: wgpu::TextureFormat) -> Result<Self> {
        use windows::Win32::Graphics::{Direct3D::D3D_DRIVER_TYPE_UNKNOWN, Direct3D11::*, Dxgi::*, Dxgi::Common::*};
        let luid = unsafe { adapter.as_hal::<wgpu::hal::api::Vulkan, _, _>(|selected|
            selected.and_then(|selected| selected.adapter_luid())) }.context("Vulkan composition adapter has no valid LUID")?;
        let composition = WindowsComposition::new(hwnd)?;
        unsafe {
            let factory: IDXGIFactory2 = CreateDXGIFactory1().context("Create Vulkan composition DXGI factory")?;
            let mut index = 0;
            let dxgi = loop {
                let candidate = factory.EnumAdapters1(index).context("Find Vulkan composition adapter by LUID")?;
                let mut desc = DXGI_ADAPTER_DESC1::default();
                candidate.GetDesc1(&mut desc).context("Read Vulkan composition adapter")?;
                let candidate_luid = (u64::from(desc.AdapterLuid.LowPart) | ((desc.AdapterLuid.HighPart as u32 as u64) << 32)).to_le_bytes();
                if candidate_luid == luid { break candidate; }
                index += 1;
            };
            let adapter3: IDXGIAdapter3 = dxgi.cast()?;
            let mut budget = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            adapter3.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut budget)?;
            let one = u64::from(width).checked_mul(u64::from(height)).and_then(|n| n.checked_mul(4)).context("Vulkan composition extent overflow")?;
            let physical = one.checked_add(65535).and_then(|n| (n / 65536 * 65536).checked_mul(4)).context("Vulkan composition memory overflow")?;
            ensure!(budget.Budget != 0 && (u128::from(budget.CurrentUsage) + u128::from(physical)) * 100 < u128::from(budget.Budget) * 95,
                "Vulkan native presentation exceeds the current GPU memory budget");
            let mut d3d = None; let mut context = None;
            D3D11CreateDevice(&dxgi, D3D_DRIVER_TYPE_UNKNOWN, windows::Win32::Foundation::HMODULE(0),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut d3d), None, Some(&mut context))?;
            let d3d = d3d.context("Missing same-adapter D3D11 device")?;
            let context = context.context("Missing D3D11 presentation context")?;
            let swapchain = factory.CreateSwapChainForComposition(&d3d, &DXGI_SWAP_CHAIN_DESC1 {
                Width: width, Height: height, Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2, Scaling: DXGI_SCALING_STRETCH, SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED, ..Default::default()
            }, None::<&IDXGIOutput>)?;
            composition.visual.SetContent(&swapchain)?;
            let mut shared = None;
            d3d.CreateTexture2D(&D3D11_TEXTURE2D_DESC { Width: width, Height: height, MipLevels: 1, ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                MiscFlags: (D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0 | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX.0) as u32,
                ..Default::default() }, None, Some(&mut shared))?;
            let shared = shared.context("Missing shared native texture")?;
            let mutex: IDXGIKeyedMutex = shared.cast()?;
            let handle = shared.cast::<IDXGIResource1>()?.CreateSharedHandle(None, 0x8000_0000 | 1, windows::core::PCWSTR::null())?;
            let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
            let descriptor = wgpu::TextureDescriptor { label: Some("quick_vulkan_native_alias"), size,
                mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8Unorm, usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[] };
            let imported_result = device.as_hal::<wgpu::hal::api::Vulkan, _, _>(|selected| {
                selected.context("Vulkan presentation needs the selected wgpu Vulkan device")?
                    .import_native_composition_image(handle.0 as *mut c_void, &wgpu::hal::TextureDescriptor {
                        label: descriptor.label, size, mip_level_count: 1, sample_count: 1, dimension: descriptor.dimension,
                        format: descriptor.format, usage: wgpu::hal::TextureUses::COLOR_TARGET,
                        memory_flags: wgpu::hal::MemoryFlags::empty(), view_formats: vec![],
                    }).map_err(|error| anyhow::anyhow!("Import Vulkan native composition image: {error:?}"))
            });
            let _ = windows::Win32::Foundation::CloseHandle(handle);
            let (texture, target) = imported_result.context("Missing native Vulkan device")??;
            let imported = device.create_texture_from_hal::<wgpu::hal::api::Vulkan>(texture, &descriptor);
            let imported_view = imported.create_view(&Default::default());
            let source = device.create_texture(&wgpu::TextureDescriptor { label: Some("quick_vulkan_finished_color"),
                format, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                ..descriptor });
            let (bind_group, pipeline) = native_present_pipeline(device, &source);
            composition.resize(width, height, 1.0)?;
            println!("[NOVA_QUICK_NATIVE_PRESENT] backend=vulkan shared_device=true same_adapter_luid=true physical_color_images=4 cpu_readback=false renderer=existing_fast");
            Ok(Self { imported, imported_view, target, source, bind_group, pipeline, shared, mutex, context, swapchain, composition })
        }
    }

    pub(crate) fn source(&self) -> &wgpu::Texture { &self.source }

    pub(crate) fn resize_clip(&self, width: u32, height: u32, dpi_scale: f64) -> Result<()> {
        self.composition.resize(width, height, dpi_scale)
    }

    pub(crate) fn encode(&self, encoder: &mut wgpu::CommandEncoder) -> Result<()> {
        unsafe { self.target.arm_next_submit().map_err(|error| anyhow::anyhow!("Arm native Vulkan presentation: {error:?}"))?; }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("quick_native_gpu_present_only"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &self.imported_view, resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None });
        pass.set_pipeline(&self.pipeline); pass.set_bind_group(0, &self.bind_group, &[]); pass.draw(0..3, 0..1);
        Ok(())
    }

    pub(crate) fn present(&self) -> Result<()> { unsafe {
        let result = (self.mutex.vtable().AcquireSync)(self.mutex.as_raw(), 1, 5000);
        ensure!(result.0 == 0, "Vulkan native shared image ownership wait failed: {result:?}");
        let copied = (|| -> Result<()> {
            let destination: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D = self.swapchain.GetBuffer(0)?;
            self.context.CopyResource(&destination, &self.shared); self.context.Flush(); Ok(())
        })();
        self.mutex.ReleaseSync(0)?; copied?;
        self.swapchain.Present(1, 0).ok()?;
        self.composition.commit()
    } }

    pub(crate) fn present_count(&self) -> Option<u32> { unsafe { self.swapchain.GetLastPresentCount().ok() } }
    pub(crate) fn displayed(&self) -> Option<(u32, i64)> { unsafe {
        use windows::Win32::Graphics::Dxgi::{DXGI_FRAME_STATISTICS, DXGI_FRAME_STATISTICS_MEDIA, IDXGISwapChainMedia};
        let mut stats = DXGI_FRAME_STATISTICS::default();
        if self.swapchain.GetFrameStatistics(&mut stats).is_err() {
            let media: IDXGISwapChainMedia = self.swapchain.cast().ok()?;
            let mut alternative = DXGI_FRAME_STATISTICS_MEDIA::default();
            media.GetFrameStatisticsMedia(&mut alternative).ok()?;
            stats.PresentCount = alternative.PresentCount; stats.SyncQPCTime = alternative.SyncQPCTime;
        }
        (stats.PresentCount != 0 && stats.SyncQPCTime != 0).then_some((stats.PresentCount, stats.SyncQPCTime))
    } }
}

// Presentation-only color transfer; shared by the actual HWND route and its
// no-window GPU ownership/readback contract test.
fn native_present_pipeline(device: &wgpu::Device, source: &wgpu::Texture) -> (wgpu::BindGroup, wgpu::RenderPipeline) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("quick_native_present_layout"),
        entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None }] });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("quick_native_present_source"), layout: &layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source.create_view(&Default::default())) }] });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("quick_native_present_only"), source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(QUICK_NATIVE_PRESENT_SHADER)) });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("quick_native_present_pipeline_layout"),
        bind_group_layouts: &[&layout], push_constant_ranges: &[] });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: Some("quick_native_present_pipeline"), layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState { module: &shader, entry_point: "vertex", buffers: &[] },
        fragment: Some(wgpu::FragmentState { module: &shader, entry_point: "fragment", targets: &[Some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::Bgra8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
        primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview: None });
    (bind_group, pipeline)
}

const QUICK_NATIVE_PRESENT_SHADER: &str = r#"
@group(0) @binding(0) var source: texture_2d<f32>;
@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
fn encode_srgb(v: f32) -> f32 { if v <= 0.0031308 { return v * 12.92; } return 1.055 * pow(v, 1.0/2.4) - 0.055; }
@fragment fn fragment(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(source, vec2<i32>(p.xy), 0);
    return vec4<f32>(encode_srgb(c.r),encode_srgb(c.g),encode_srgb(c.b),c.a);
}
"#;

fn validate_scale(scale: f64) -> Result<()> {
    ensure!(
        scale.is_finite() && scale > 0.0,
        "DirectComposition DPI scale must be finite and positive"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpi_scale_accepts_physical_pixel_scales_without_double_scaling() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            assert!(validate_scale(scale).is_ok());
        }
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(validate_scale(scale).is_err());
        }
    }

    #[test]
    fn null_hwnd_is_rejected_without_creating_a_window_or_gpu_device() {
        assert!(WindowsComposition::new(0).is_err());
    }

    /// Exercises the real same-device import + keyed-mutex submit hook without
    /// creating any HWND, swapchain, DirectComposition tree, or visible frame.
    #[test]
    #[ignore = "requires a real Windows Vulkan GPU and matching D3D11 adapter"]
    fn vulkan_shared_texture_gpu_handoff_without_a_window() -> Result<()> {
        use windows::Win32::Graphics::{Direct3D::D3D_DRIVER_TYPE_UNKNOWN, Direct3D11::*, Dxgi::*, Dxgi::Common::*};
        pollster::block_on(async {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::VULKAN, ..Default::default()
            });
            let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: None, power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
            }).await.context("No Vulkan adapter")?;
            let luid = unsafe { adapter.as_hal::<wgpu::hal::api::Vulkan, _, _>(|selected|
                selected.and_then(|selected| selected.adapter_luid())) }.context("No Vulkan LUID")?;
            let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("no_window_native_import_test"), required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
            }, None).await?;
            device.push_error_scope(wgpu::ErrorFilter::Validation);
            unsafe {
                let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
                let mut index = 0;
                let dxgi = loop {
                    let candidate = factory.EnumAdapters1(index)?;
                    let mut desc = DXGI_ADAPTER_DESC1::default();
                    candidate.GetDesc1(&mut desc)?;
                    if (u64::from(desc.AdapterLuid.LowPart) | ((desc.AdapterLuid.HighPart as u32 as u64) << 32)).to_le_bytes() == luid {
                        break candidate;
                    }
                    index += 1;
                };
                let mut d3d = None; let mut context = None;
                D3D11CreateDevice(&dxgi, D3D_DRIVER_TYPE_UNKNOWN, windows::Win32::Foundation::HMODULE(0),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut d3d), None, Some(&mut context))?;
                let d3d = d3d.context("No D3D11 device")?;
                let context = context.context("No D3D11 context")?;
                let desc = D3D11_TEXTURE2D_DESC { Width: 2, Height: 2, MipLevels: 1, ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                    MiscFlags: (D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0 | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX.0) as u32,
                    ..Default::default() };
                let mut shared = None;
                d3d.CreateTexture2D(&desc, None, Some(&mut shared))?;
                let shared = shared.context("No shared texture")?;
                let mutex: IDXGIKeyedMutex = shared.cast()?;
                let handle = shared.cast::<IDXGIResource1>()?.CreateSharedHandle(None, 0x8000_0001, windows::core::PCWSTR::null())?;
                let size = wgpu::Extent3d { width: 2, height: 2, depth_or_array_layers: 1 };
                let desc = wgpu::TextureDescriptor { label: Some("native_import_test"), size,
                    mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Bgra8Unorm, usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[] };
                let imported = device.as_hal::<wgpu::hal::api::Vulkan, _, _>(|native|
                    native.context("No native Vulkan device")?.import_native_composition_image(handle.0 as *mut c_void,
                        &wgpu::hal::TextureDescriptor { label: desc.label, size, mip_level_count: 1, sample_count: 1,
                            dimension: desc.dimension, format: desc.format, usage: wgpu::hal::TextureUses::COLOR_TARGET,
                            memory_flags: wgpu::hal::MemoryFlags::empty(), view_formats: vec![] })
                        .map_err(|error| anyhow::anyhow!("Actual shared-image import: {error:?}")));
                let _ = windows::Win32::Foundation::CloseHandle(handle);
                let (raw, target) = imported.context("Missing Vulkan device")??;
                let imported = device.create_texture_from_hal::<wgpu::hal::api::Vulkan>(raw, &desc);
                let view = imported.create_view(&Default::default());
                let source = device.create_texture(&wgpu::TextureDescriptor {
                    format: wgpu::TextureFormat::Bgra8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, ..desc
                });
                let (bind_group, pipeline) = native_present_pipeline(&device, &source);
                if let Some(error) = device.pop_error_scope().await { anyhow::bail!("Native import/pipeline validation: {error}"); }
                device.push_error_scope(wgpu::ErrorFilter::Validation);
                let mut staging = None;
                d3d.CreateTexture2D(&D3D11_TEXTURE2D_DESC { Width: 2, Height: 2, MipLevels: 1, ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_STAGING, CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    ..Default::default() }, None, Some(&mut staging))?;
                let staging = staging.context("No staging texture")?;
                for bytes in [[0u8, 32, 128, 255], [255, 128, 32, 128]] {
                    let pixels = bytes.repeat(4);
                    queue.write_texture(source.as_image_copy(), &pixels,
                        wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(8), rows_per_image: Some(2) }, size);
                    let mut encoder = device.create_command_encoder(&Default::default());
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("actual_native_present_shader_no_window"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, resolve_target: None,
                                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
                            depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None,
                        });
                        pass.set_pipeline(&pipeline); pass.set_bind_group(0, &bind_group, &[]); pass.draw(0..3, 0..1);
                    }
                    target.arm_next_submit().map_err(|error| anyhow::anyhow!("Arm real Vulkan submit: {error:?}"))?;
                    queue.submit(Some(encoder.finish()));
                    let acquired = (mutex.vtable().AcquireSync)(mutex.as_raw(), 1, 5000);
                    ensure!(acquired.0 == 0, "Actual D3D key-1 handoff failed: {acquired:?}");
                    context.CopyResource(&staging, &shared);
                    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                    let readback = context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped));
                    // Release the shared resource on the error path too.
                    mutex.ReleaseSync(0)?;
                    readback?;
                    let actual = std::slice::from_raw_parts(mapped.pData as *const u8, mapped.RowPitch as usize * 2);
                    let mut valid = true;
                    for y in 0..2 { for x in 0..2 { for c in 0..4 {
                        valid &= actual[y * mapped.RowPitch as usize + x * 4 + c].abs_diff(bytes[c]) <= 1;
                    } } }
                    let first = actual[..4].to_vec();
                    context.Unmap(&staging, 0);
                    ensure!(valid, "Native presentation changed color/alpha: expected={bytes:?} first={first:?}");
                    println!("NATIVE_IMPORT_FRAME bgra={first:?} keyed_mutex=0_to_1_to_0 same_device=true");
                }
                device.poll(wgpu::Maintain::Wait);
                if let Some(error) = device.pop_error_scope().await { anyhow::bail!("Native import validation: {error}"); }
                println!("NATIVE_IMPORT_PASS adapter={:?} backend=Vulkan frames=2 hwnd=none visible_first_frame=not_tested", adapter.get_info().name);
            }
            Ok(())
        })
    }
}
