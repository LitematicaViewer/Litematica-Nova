//! Window-facing presentation state without a second rendering implementation.
//! A shared macOS viewport has no renderer-owned NSWindow or native handle.
use std::sync::Arc;

use anyhow::{Context, Result};
use winit::{
    dpi::{PhysicalSize, Size},
    raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle},
    window::{Window, WindowId},
};

#[cfg(target_os = "macos")]
use crate::mac_shared_viewport::MacSharedViewport;

pub(crate) struct NativeViewportWindow {
    native: Option<Arc<Window>>,
    #[cfg(target_os = "macos")]
    shared: Option<Arc<MacSharedViewport>>,
}

impl From<Window> for NativeViewportWindow {
    fn from(window: Window) -> Self { Self::from(Arc::new(window)) }
}

impl From<Arc<Window>> for NativeViewportWindow {
    fn from(window: Arc<Window>) -> Self {
        Self { native: Some(window), #[cfg(target_os = "macos")] shared: None }
    }
}

impl NativeViewportWindow {
    pub(crate) fn build<T: 'static>(
        builder: winit::window::WindowBuilder,
        target: &winit::event_loop::EventLoopWindowTarget<T>,
        host: Option<&viewer_launch_contract::QuickPreviewHostWindow>,
    ) -> Result<Arc<Self>> {
        #[cfg(target_os = "macos")]
        if let Some(endpoint) = host.and_then(|host| host.native_composition.as_ref()) {
            let shared = MacSharedViewport::connect(endpoint.clone(), [1280, 800], 1.0)?;
            return Ok(Arc::new(Self::from_shared(shared)));
        }
        #[cfg(not(target_os = "macos"))]
        let _ = host;
        Ok(Arc::new(builder.build(target).context("create native viewport window")?.into()))
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn from_shared(shared: Arc<MacSharedViewport>) -> Self {
        Self { native: None, shared: Some(shared) }
    }

    pub(crate) fn native_window_arc(&self) -> Result<Arc<Window>> {
        self.native.as_ref().map(Arc::clone)
            .context("the shared native viewport has no renderer-owned OS window")
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn shared(&self) -> Option<&Arc<MacSharedViewport>> { self.shared.as_ref() }

    pub(crate) fn inner_size(&self) -> PhysicalSize<u32> {
        if let Some(window) = &self.native { return window.inner_size(); }
        #[cfg(target_os = "macos")]
        if let Some(shared) = &self.shared {
            let [width, height] = shared.extent();
            return PhysicalSize::new(width, height);
        }
        unreachable!("viewport must own either a native window or a shared presentation")
    }

    pub(crate) fn scale_factor(&self) -> f64 {
        if let Some(window) = &self.native { return window.scale_factor(); }
        #[cfg(target_os = "macos")]
        if let Some(shared) = &self.shared { return shared.scale_factor(); }
        unreachable!("viewport must own either a native window or a shared presentation")
    }

    pub(crate) fn request_redraw(&self) {
        if let Some(window) = &self.native { window.request_redraw(); }
        #[cfg(target_os = "macos")]
        if let Some(shared) = &self.shared { shared.request_redraw(); }
    }

    pub(crate) fn id(&self) -> WindowId {
        if let Some(window) = &self.native { return window.id(); }
        // This identity is only for the Viewer's internal event routing. It
        // never becomes an NSWindow, surface handle or IPC Host identity.
        unsafe { WindowId::dummy() }
    }

    pub(crate) fn set_visible(&self, visible: bool) {
        if let Some(window) = &self.native { window.set_visible(visible); }
        #[cfg(target_os = "macos")]
        if !visible && let Some(shared) = &self.shared { shared.suspend(); }
        // A producer cannot show the Host view. Only its accepted Host lease
        // plus a completed frame for that epoch can make the NSView visible.
    }

    pub(crate) fn set_title(&self, title: &str) {
        if let Some(window) = &self.native { window.set_title(title); }
        // The Host owns the application title for a shared child view.
    }

    pub(crate) fn is_maximized(&self) -> bool {
        self.native.as_ref().is_some_and(|window| window.is_maximized())
    }

    pub(crate) fn set_maximized(&self, maximized: bool) {
        if let Some(window) = &self.native { window.set_maximized(maximized); }
        // A renderer cannot maximize its Host or invent a separate window.
    }

    pub(crate) fn focus_window(&self) {
        if let Some(window) = &self.native { window.focus_window(); }
        // Shared-view focus is applied by the Host's native presentation/input
        // lease; no cross-process NSResponder operation is performed here.
    }

    pub(crate) fn request_inner_size<S: Into<Size>>(&self, size: S) -> Option<PhysicalSize<u32>> {
        if let Some(window) = &self.native { return window.request_inner_size(size); }
        #[cfg(target_os = "macos")]
        if let Some(shared) = &self.shared {
            let size = size.into().to_physical::<u32>(shared.scale_factor());
            shared.set_extent([size.width, size.height]);
            return Some(self.inner_size());
        }
        None
    }

    pub(crate) fn pre_present_notify(&self) {
        if let Some(window) = &self.native { window.pre_present_notify(); }
    }
}

pub(crate) fn build<T: 'static>(
    builder: winit::window::WindowBuilder,
    target: &winit::event_loop::EventLoopWindowTarget<T>,
    host: Option<&viewer_launch_contract::QuickPreviewHostWindow>,
) -> Result<Arc<NativeViewportWindow>> {
    NativeViewportWindow::build(builder, target, host)
}

#[cfg(any(windows, target_os = "linux"))]
impl std::ops::Deref for NativeViewportWindow {
    type Target = Window;
    fn deref(&self) -> &Window {
        self.native.as_deref().expect("this platform only constructs real native windows")
    }
}

impl HasWindowHandle for NativeViewportWindow {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        self.native.as_ref().ok_or(HandleError::Unavailable)?.window_handle()
    }
}

impl HasDisplayHandle for NativeViewportWindow {
    fn display_handle(&self) -> std::result::Result<DisplayHandle<'_>, HandleError> {
        self.native.as_ref().ok_or(HandleError::Unavailable)?.display_handle()
    }
}
