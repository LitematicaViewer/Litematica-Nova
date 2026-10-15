//! DirectComposition presentation for a viewer-owned native child HWND.
//! The existing wgpu DX12 composition surface owns the GPU swapchain; this module
//! only owns its target/visual and never captures a bitmap or reparents a window.

use std::ffi::c_void;

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
use windows::core::{IUnknown, Interface};

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
}
