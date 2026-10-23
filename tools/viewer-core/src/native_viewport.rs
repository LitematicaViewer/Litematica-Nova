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
    #[cfg(target_os = "windows")]
    child_pointer_clip: std::sync::Mutex<Option<[i32; 4]>>,
    #[cfg(target_os = "macos")]
    shared: Option<Arc<MacSharedViewport>>,
}

#[cfg(target_os = "windows")]
impl Drop for NativeViewportWindow {
    fn drop(&mut self) {
        // Ordinary error/exit unwinding must not strand a desktop restriction.
        if let Ok(owned) = self.child_pointer_clip.get_mut() {
            if let Some(clip) = owned.take() { let _ = pointer_os::release_clip(clip); }
        }
    }
}

impl From<Window> for NativeViewportWindow {
    fn from(window: Window) -> Self { Self::from(Arc::new(window)) }
}

impl From<Arc<Window>> for NativeViewportWindow {
    fn from(window: Arc<Window>) -> Self {
        Self { native: Some(window), pointer_restore: std::sync::Mutex::new(None),
            #[cfg(target_os = "windows")] child_pointer_clip: std::sync::Mutex::new(None),
            #[cfg(target_os = "macos")] shared: None }
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
            if pointer_os::focus_child(window).is_some() { return; }
            window.focus_window();
        }
        // Shared-view focus is applied by the Host's native presentation/input
        // lease; no cross-process NSResponder operation is performed here.
    }

    pub(crate) fn set_pointer_capture(&self, captured: bool) -> Result<()> {
        if let Some(window) = &self.native {
            #[cfg(target_os = "windows")]
            if pointer_os::is_child(window) {
                return self.set_child_pointer_capture(window, captured);
            }
            if captured {
                anyhow::ensure!(self.camera_has_focus(), "viewport has no keyboard focus");
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

    #[cfg(target_os = "windows")]
    fn set_child_pointer_capture(&self, window: &Window, captured: bool) -> Result<()> {
        let mut owned = self.child_pointer_clip.lock().expect("child pointer clip");
        if captured {
            anyhow::ensure!(pointer_os::has_focus(window), "viewport has no keyboard focus");
            if owned.is_some() { return Ok(()); }
            let restore = pointer_os::cursor_in(window).context("read viewport cursor position")?;
            let clip = pointer_os::client_clip(window)?;
            // Winit 0.29 tests HWND == GetActiveWindow before ClipCursor. That
            // always excludes WS_CHILD; Confined returns Ok without clipping.
            // This lease uses the focused child's physical client rectangle.
            pointer_os::set_clip(Some(clip))?;
            *owned = Some(clip);
            *self.pointer_restore.lock().expect("pointer state") = Some(restore);
            window.set_cursor_visible(false);
            return Ok(());
        }
        if let Some(clip) = owned.take() {
            let released = pointer_os::release_clip(clip);
            window.set_cursor_visible(true);
            pointer_os::release_drag(window);
            if let Some(position) = self.pointer_restore.lock().expect("pointer state").take() {
                let size = window.inner_size();
                if pointer_os::has_focus(window) {
                    let _ = window.set_cursor_position(winit::dpi::PhysicalPosition::new(
                        position[0].clamp(0.0, f64::from(size.width.saturating_sub(1))),
                        position[1].clamp(0.0, f64::from(size.height.saturating_sub(1)))));
                }
            }
            released?;
        } else {
            // Drag mode has no long-lived clip, but can still own button capture.
            pointer_os::release_drag(window);
        }
        Ok(())
    }

    pub(crate) fn refresh_pointer_capture(&self) -> bool {
        #[cfg(target_os = "windows")]
        if let Some(window) = &self.native {
            if pointer_os::is_child(window) {
                let mut owned = self.child_pointer_clip.lock().expect("child pointer clip");
                let Some(previous) = *owned else { return false; };
                if !pointer_os::has_focus(window) { return false; }
                let refreshed = (|| -> Result<bool> {
                    // An external focus/capture owner wins; do not reclaim its clip.
                    if pointer_os::clip_rect()? != previous { return Ok(false); }
                    let next = pointer_os::client_clip(window)?;
                    if next != previous {
                        pointer_os::set_clip(Some(next))?;
                        *owned = Some(next);
                    }
                    Ok(pointer_os::clip_rect()? == next)
                })();
                return match refreshed {
                    Ok(value) => value,
                    Err(error) => { eprintln!("[NOVA_CAMERA_INPUT] capture_refresh_failed error={error:#}"); false }
                };
            }
        }
        self.camera_capture_confirmed()
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
            {
                if pointer_os::is_child(window) {
                    return pointer_os::has_focus(window)
                        && self.child_pointer_clip.lock().expect("child pointer clip").is_some_and(|owned|
                            pointer_os::clip_rect().is_ok_and(|actual| actual == owned));
                }
                return pointer_os::is_confined(window);
            }
            #[cfg(not(target_os = "windows"))]
            { return window.has_focus(); }
        }
        false
    }

    pub(crate) fn camera_drag_cancelled(&self) -> bool {
        #[cfg(target_os = "windows")]
        if let Some(window) = &self.native { return pointer_os::drag_cancelled(window); }
        false
    }

    pub(crate) fn camera_drag_active(&self) -> bool {
        #[cfg(target_os = "windows")]
        if let Some(window) = &self.native { return pointer_os::drag_active(window); }
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
        fn GetForegroundWindow() -> isize;
        fn GetAncestor(hwnd: isize, flags: u32) -> isize;
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
        fn ClipCursor(rect: *const RECT) -> i32;
        fn GetWindowDpiAwarenessContext(hwnd: isize) -> isize;
        fn SetThreadDpiAwarenessContext(context: isize) -> isize;
        fn GetSystemMetrics(index: i32) -> i32;
    }
    fn hwnd(window: &Window) -> isize {
        match window.window_handle().map(|handle| handle.as_raw()) {
            Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get(), _ => 0,
        }
    }
    pub(super) fn is_child(window: &Window) -> bool {
        let handle = hwnd(window);
        handle != 0 && unsafe { GetWindowLongPtrW(handle, -16) } & 0x4000_0000 != 0
    }
    pub(super) fn client_clip(window: &Window) -> Result<[i32; 4]> {
        let handle = hwnd(window);
        let context = unsafe { GetWindowDpiAwarenessContext(handle) };
        let previous = unsafe { SetThreadDpiAwarenessContext(context) };
        if context == 0 || previous == 0 {
            return Err(std::io::Error::last_os_error()).context("enter viewport DPI context");
        }
        let result = (|| {
            let mut client = RECT::default();
            if unsafe { GetClientRect(handle, &mut client) } == 0 {
                return Err(std::io::Error::last_os_error()).context("read viewport client rectangle");
            }
            let mut first = POINT { x: client.left, y: client.top };
            let mut last = POINT { x: client.right, y: client.bottom };
            if unsafe { ClientToScreen(handle, &mut first) == 0 || ClientToScreen(handle, &mut last) == 0 } {
                return Err(std::io::Error::last_os_error()).context("map viewport clip to physical screen");
            }
            let [desktop_x, desktop_y, desktop_width, desktop_height] = unsafe {
                [GetSystemMetrics(76), GetSystemMetrics(77), GetSystemMetrics(78), GetSystemMetrics(79)]
            };
            // A partly off-screen child has a smaller effective cursor region;
            // account for the desktop clamp before storing the owned rectangle.
            let clip = [first.x.max(desktop_x), first.y.max(desktop_y),
                last.x.min(desktop_x.saturating_add(desktop_width)),
                last.y.min(desktop_y.saturating_add(desktop_height))];
            anyhow::ensure!(clip[2] > clip[0] && clip[3] > clip[1], "viewport has an empty pointer clip");
            Ok(clip)
        })();
        if previous != 0 { unsafe { SetThreadDpiAwarenessContext(previous); } }
        result
    }
    pub(super) fn clip_rect() -> Result<[i32; 4]> {
        let mut rect = RECT::default();
        if unsafe { GetClipCursor(&mut rect) } == 0 {
            return Err(std::io::Error::last_os_error()).context("read pointer clip");
        }
        Ok([rect.left, rect.top, rect.right, rect.bottom])
    }
    pub(super) fn set_clip(rect: Option<[i32; 4]>) -> Result<()> {
        let rect = rect.map(|[left, top, right, bottom]| RECT { left, top, right, bottom });
        let pointer = rect.as_ref().map_or(std::ptr::null(), |value| value as *const RECT);
        if unsafe { ClipCursor(pointer) } == 0 {
            return Err(std::io::Error::last_os_error()).context("set viewport pointer clip");
        }
        Ok(())
    }
    pub(super) fn release_clip(owned: [i32; 4]) -> Result<()> {
        // A stale release must not remove a newer owner's restriction.
        if clip_rect()? == owned { set_clip(None)?; }
        Ok(())
    }
    // Some(false) still denotes a child: never fall back to top-level activation
    // (which winit implements by synthesizing Alt) when its SetFocus fails.
    pub(super) fn focus_child(window: &Window) -> Option<bool> {
        let handle = hwnd(window);
        if handle == 0 || unsafe { GetWindowLongPtrW(handle, -16) } & 0x4000_0000 == 0 { return None; }
        if unsafe { GetAncestor(handle, 2) != GetForegroundWindow() } { return Some(false); }
        unsafe { SetFocus(handle); }
        Some(has_focus(window))
    }
    pub(super) fn has_focus(window: &Window) -> bool {
        let handle = hwnd(window);
        // A child can retain thread-local focus after another application or
        // an owned dialog becomes foreground. Such focus is not an input lease.
        handle != 0 && unsafe { GetFocus() == handle && GetAncestor(handle, 2) == GetForegroundWindow() }
    }
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
    fn drag_button_down() -> bool {
        unsafe { GetAsyncKeyState(0x01) < 0 || GetAsyncKeyState(0x02) < 0 || GetAsyncKeyState(0x04) < 0 }
    }
    pub(super) fn drag_cancelled(window: &Window) -> bool {
        drag_button_down() && unsafe { GetCapture() != hwnd(window) }
    }
    pub(super) fn drag_active(window: &Window) -> bool {
        drag_button_down() && unsafe { GetCapture() == hwnd(window) }
    }
    pub(super) fn release_drag(window: &Window) {
        if unsafe { GetCapture() == hwnd(window) } { unsafe { ReleaseCapture(); } }
    }
}
