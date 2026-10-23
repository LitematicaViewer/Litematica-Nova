//! Window-facing presentation state without a second rendering implementation.
//! A shared macOS viewport has no renderer-owned NSWindow or native handle.
use std::sync::Arc;

use anyhow::{Context, Result};
use winit::{
    dpi::{PhysicalSize, Size},
    raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle},
    window::{CursorGrabMode, Window, WindowId},
};

#[cfg(target_os = "macos")]
use crate::mac_shared_viewport::MacSharedViewport;

pub(crate) struct NativeViewportWindow {
    native: Option<Arc<Window>>,
    pointer_restore: std::sync::Mutex<Option<[f64; 2]>>,
    #[cfg(target_os = "macos")]
    shared: Option<Arc<MacSharedViewport>>,
}

impl From<Window> for NativeViewportWindow {
    fn from(window: Window) -> Self { Self::from(Arc::new(window)) }
}

impl From<Arc<Window>> for NativeViewportWindow {
    fn from(window: Arc<Window>) -> Self {
        Self { native: Some(window), pointer_restore: std::sync::Mutex::new(None), #[cfg(target_os = "macos")] shared: None }
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
        Self { native: None, pointer_restore: std::sync::Mutex::new(None), shared: Some(shared) }
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
        if let Some(window) = &self.native {
            // Winit's top-level activation helper synthesizes Alt when the
            // child HWND is not the foreground top-level HWND. A genuine child
            // only needs keyboard focus inside the already active Host.
            #[cfg(target_os = "windows")]
            if pointer_os::focus_child(window) { return; }
            window.focus_window();
        }
        // Shared-view focus is applied by the Host's native presentation/input
        // lease; no cross-process NSResponder operation is performed here.
    }

    pub(crate) fn set_pointer_capture(&self, captured: bool) -> Result<()> {
        if let Some(window) = &self.native {
            if captured {
                #[cfg(target_os = "windows")]
                let restore = pointer_os::cursor_in(window);
                #[cfg(not(target_os = "windows"))]
                let restore = None;
                window.set_cursor_grab(CursorGrabMode::Locked)
                    .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
                    .context("capture viewport pointer")?;
                *self.pointer_restore.lock().expect("pointer state") = restore;
                window.set_cursor_visible(false);
            } else {
                let released = window.set_cursor_grab(CursorGrabMode::None);
                window.set_cursor_visible(true);
                #[cfg(target_os = "windows")]
                pointer_os::release_drag(window);
                if let Some(position) = self.pointer_restore.lock().expect("pointer state").take() {
                    // Restore relative to the current viewport, not a stale monitor coordinate.
                    let size = window.inner_size();
                    let position = winit::dpi::PhysicalPosition::new(
                        position[0].clamp(0.0, f64::from(size.width.saturating_sub(1))),
                        position[1].clamp(0.0, f64::from(size.height.saturating_sub(1))));
                    if self.camera_has_focus() { let _ = window.set_cursor_position(position); }
                }
                released.context("release viewport pointer")?;
            }
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        if let Some(shared) = &self.shared {
            return shared.set_pointer_capture(captured).map_err(anyhow::Error::msg);
        }
        anyhow::bail!("native viewport pointer owner is unavailable")
    }

    pub(crate) fn camera_has_focus(&self) -> bool {
        if let Some(window) = &self.native {
            #[cfg(target_os = "windows")]
            { return pointer_os::has_focus(window); }
            #[cfg(not(target_os = "windows"))]
            { return window.has_focus(); }
        }
        // Shared macOS focus/capture is acknowledged by native Host events.
        true
    }

    pub(crate) fn camera_capture_confirmed(&self) -> bool {
        if let Some(window) = &self.native {
            #[cfg(target_os = "windows")]
            { return pointer_os::is_confined(window); }
            #[cfg(not(target_os = "windows"))]
            { return window.has_focus(); }
        }
        false
    }

    pub(crate) fn camera_drag_cancelled(&self) -> bool {
        #[cfg(target_os = "windows")]
        if let Some(window) = &self.native { return pointer_os::right_drag_cancelled(window); }
        false
    }

    pub(crate) fn camera_drag_active(&self) -> bool {
        #[cfg(target_os = "windows")]
        if let Some(window) = &self.native { return pointer_os::right_drag_active(window); }
        true
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

// Native event state, not a second input hook. Winit already captures button
// drags; querying the owner closes its missing WM_CAPTURECHANGED notification.
#[cfg(target_os = "windows")]
mod pointer_os {
    use super::*;
    use windows::Win32::Foundation::{POINT, RECT};
    use winit::raw_window_handle::RawWindowHandle;
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetFocus() -> isize;
        fn SetFocus(hwnd: isize) -> isize;
        fn GetWindowLongPtrW(hwnd: isize, index: i32) -> isize;
        fn GetCapture() -> isize;
        fn ReleaseCapture() -> i32;
        fn GetAsyncKeyState(key: i32) -> i16;
        fn GetCursorPos(point: *mut POINT) -> i32;
        fn ScreenToClient(hwnd: isize, point: *mut POINT) -> i32;
        fn ClientToScreen(hwnd: isize, point: *mut POINT) -> i32;
        fn GetClientRect(hwnd: isize, rect: *mut RECT) -> i32;
        fn GetClipCursor(rect: *mut RECT) -> i32;
    }
    fn hwnd(window: &Window) -> isize {
        match window.window_handle().map(|handle| handle.as_raw()) {
            Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get(), _ => 0,
        }
    }
    pub(super) fn focus_child(window: &Window) -> bool {
        let handle = hwnd(window);
        if handle != 0 && unsafe { GetWindowLongPtrW(handle, -16) } & 0x4000_0000 != 0 {
            unsafe { SetFocus(handle); } return true;
        }
        false
    }
    pub(super) fn has_focus(window: &Window) -> bool { let handle = hwnd(window); handle != 0 && unsafe { GetFocus() == handle } }
    pub(super) fn cursor_in(window: &Window) -> Option<[f64; 2]> {
        let mut point = POINT::default();
        (unsafe { GetCursorPos(&mut point) != 0 && ScreenToClient(hwnd(window), &mut point) != 0 })
            .then_some([f64::from(point.x), f64::from(point.y)])
    }
    pub(super) fn is_confined(window: &Window) -> bool {
        if !has_focus(window) { return false; }
        let mut clip = RECT::default(); let mut client = RECT::default(); let mut origin = POINT::default();
        if unsafe { GetClipCursor(&mut clip) == 0 || GetClientRect(hwnd(window), &mut client) == 0 || ClientToScreen(hwnd(window), &mut origin) == 0 } { return false; }
        clip.left >= origin.x && clip.top >= origin.y && clip.right <= origin.x + client.right && clip.bottom <= origin.y + client.bottom
    }
    pub(super) fn right_drag_cancelled(window: &Window) -> bool {
        unsafe { GetAsyncKeyState(0x02) < 0 && GetCapture() != hwnd(window) }
    }
    pub(super) fn right_drag_active(window: &Window) -> bool {
        unsafe { GetCapture() == hwnd(window) && GetAsyncKeyState(0x02) < 0 }
    }
    pub(super) fn release_drag(window: &Window) {
        if unsafe { GetCapture() == hwnd(window) } { unsafe { ReleaseCapture(); } }
    }
}
