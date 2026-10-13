//! True WS_CHILD presentation. The existing wgpu surface draws directly into
//! this native control; no desktop overlay, CPU frame copy or reparenting.
use super::*;
use std::num::NonZeroIsize;
use viewer_launch_contract::QuickPreviewHostWindow;
use winit::raw_window_handle::Win32WindowHandle;

#[repr(C)]
struct Point { x: i32, y: i32 }
#[repr(C)]
struct GuiThreadInfo { size: u32, flags: u32, active: isize, focus: isize,
    capture: isize, menu_owner: isize, move_size: isize, caret: isize, caret_rect: [i32; 4] }
static HOST_FOCUS: AtomicIsize = AtomicIsize::new(0);

#[link(name = "user32")]
unsafe extern "system" {
    fn IsWindow(hwnd: isize) -> i32;
    fn IsIconic(hwnd: isize) -> i32;
    fn GetParent(hwnd: isize) -> isize;
    fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
    fn GetGUIThreadInfo(thread: u32, info: *mut GuiThreadInfo) -> i32;
    fn IsChild(parent: isize, child: isize) -> i32;
    fn ScreenToClient(hwnd: isize, point: *mut Point) -> i32;
    fn GetWindowDpiAwarenessContext(hwnd: isize) -> isize;
    fn SetThreadDpiAwarenessContext(context: isize) -> isize;
}

pub(super) struct DpiContext(isize);
impl Drop for DpiContext {
    fn drop(&mut self) { unsafe { SetThreadDpiAwarenessContext(self.0); } }
}

pub(super) fn validate_host(host: &QuickPreviewHostWindow) -> Result<isize> {
    let hwnd = host.hwnd.parse::<isize>().context("invalid native child host HWND")?;
    let mut pid = 0;
    if unsafe { IsWindow(hwnd) } == 0
        || unsafe { GetWindowThreadProcessId(hwnd, &mut pid) } == 0
        || pid != host.process_id {
        bail!("native child host is closed or no longer belongs to the launch process");
    }
    Ok(hwnd)
}

pub(super) fn builder(
    builder: WindowBuilder, host: &QuickPreviewHostWindow,
) -> Result<(WindowBuilder, DpiContext)> {
    let hwnd = validate_host(host)?;
    let mut info = GuiThreadInfo { size: std::mem::size_of::<GuiThreadInfo>() as u32,
        flags: 0, active: 0, focus: 0, capture: 0, menu_owner: 0, move_size: 0, caret: 0, caret_rect: [0; 4] };
    let thread = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
    if unsafe { GetGUIThreadInfo(thread, &mut info) } != 0 {
        HOST_FOCUS.store(info.focus, Ordering::Relaxed);
    }
    // All HWNDs in a parent/child tree use the parent's DPI awareness context.
    // Set this before CreateWindowEx, not after the child has been created.
    let context = unsafe { GetWindowDpiAwarenessContext(hwnd) };
    let previous = unsafe { SetThreadDpiAwarenessContext(context) };
    if context == 0 || previous == 0 { bail!("native child DPI context setup failed"); }
    let handle = Win32WindowHandle::new(NonZeroIsize::new(hwnd).context("zero native host HWND")?);
    let builder = unsafe { builder.with_parent_window(Some(RawWindowHandle::Win32(handle))) }
        .with_decorations(false).with_visible(false).with_active(false);
    Ok((builder, DpiContext(previous)))
}

pub(super) fn verify(window: &winit::window::Window, host: &QuickPreviewHostWindow) -> Result<()> {
    let hwnd = window_hwnd(window)?;
    let parent = validate_host(host)?;
    if unsafe { GetParent(hwnd) } != parent
        || unsafe { GetWindowLongPtrW(hwnd, -16) } & 0x4000_0000 == 0 {
        bail!("quick preview did not create a WS_CHILD window inside its host");
    }
    println!("[QUICK_NATIVE_CHILD] parent={} hwnd={} ws_child=true top_level=false reparent=false gpu_surface=existing-wgpu", parent, hwnd);
    Ok(())
}

pub(super) fn apply(
    window: &winit::window::Window, host: &QuickPreviewHostWindow,
    command: &ViewerSessionCommand, mode: &mut PersistentPresentationMode,
) -> Result<ViewerPresentationCommandOutcome> {
    let parent = validate_host(host)?;
    let hwnd = window_hwnd(window)?;
    verify_relationship(hwnd, parent)?;
    match command.kind {
        ViewerSessionCommandKind::AttachEmbedded | ViewerSessionCommandKind::Resize => {
            if let Some(requested) = command.host_hwnd.as_deref()
                && requested != host.hwnd { bail!("native child cannot change its host"); }
            let width = command.width.context("native child width missing")?;
            let height = command.height.context("native child height missing")?;
            if !host_allows_presentation(
                unsafe { IsWindowVisible(parent) } != 0,
                unsafe { IsIconic(parent) } != 0,
            ) {
                hide(window, host)?;
                *mode = PersistentPresentationMode::Hidden;
                return Ok(ViewerPresentationCommandOutcome::Applied(ViewerSessionState::Hidden));
            }
            let mut origin = Point { x: command.screen_x.unwrap_or(0), y: command.screen_y.unwrap_or(0) };
            if command.screen_x.is_some() && unsafe { ScreenToClient(parent, &mut origin) } == 0 {
                bail!("native child screen-to-client conversion failed");
            }
            let flags = 0x0010 | if command.kind == ViewerSessionCommandKind::AttachEmbedded { 0x0040 } else { 0 };
            if unsafe { SetWindowPos(hwnd, 0, origin.x, origin.y, width as i32, height as i32, flags) } == 0 {
                bail!("native child placement failed");
            }
            if command.kind == ViewerSessionCommandKind::AttachEmbedded {
                *mode = PersistentPresentationMode::Embedded;
            }
            window.request_redraw();
            Ok(ViewerPresentationCommandOutcome::Applied(ViewerSessionState::Attached {
                host_hwnd: host.hwnd.clone(), width, height,
            }))
        }
        ViewerSessionCommandKind::Hide | ViewerSessionCommandKind::Release => {
            hide(window, host)?;
            *mode = PersistentPresentationMode::Hidden;
            if command.kind == ViewerSessionCommandKind::Release {
                Ok(ViewerPresentationCommandOutcome::Release(ViewerSessionState::Released))
            } else { Ok(ViewerPresentationCommandOutcome::Applied(ViewerSessionState::Hidden)) }
        }
        // Explicitly reject all desktop presentation for a native child session.
        ViewerSessionCommandKind::ShowPopup | ViewerSessionCommandKind::ShowOverlay =>
            bail!("native child preview supports embedded presentation only"),
        ViewerSessionCommandKind::ReplaceScene | ViewerSessionCommandKind::CaptureFrame => Ok(ViewerPresentationCommandOutcome::NotPresentation),
    }
}

fn host_allows_presentation(visible: bool, minimized: bool) -> bool {
    visible && !minimized
}

pub(super) fn host_needs_hide(parent: isize, mode: PersistentPresentationMode) -> bool {
    mode == PersistentPresentationMode::Embedded
        && !host_allows_presentation(
            unsafe { IsWindowVisible(parent) } != 0,
            unsafe { IsIconic(parent) } != 0,
        )
}

pub(super) fn hide(
    window: &winit::window::Window, host: &QuickPreviewHostWindow,
) -> Result<()> {
    let parent = validate_host(host)?;
    let hwnd = window_hwnd(window)?;
    verify_relationship(hwnd, parent)?;
    if unsafe { GetFocus() } == hwnd { focus_host(host); }
    // AttachEmbedded uses SetWindowPos(SWP_SHOWWINDOW), which does not update
    // winit's initial cached visible=false. Always dispatch the real Win32 hide.
    // ShowWindow returns the previous visibility, not a success flag.
    const SW_HIDE: i32 = 0;
    unsafe { ShowWindow(hwnd, SW_HIDE); }
    Ok(())
}

fn verify_relationship(hwnd: isize, parent: isize) -> Result<()> {
    if unsafe { GetParent(hwnd) } != parent
        || unsafe { GetWindowLongPtrW(hwnd, -16) } & 0x4000_0000 == 0 {
        bail!("native preview parent relationship changed");
    }
    Ok(())
}

pub(super) fn focus_host(host: &QuickPreviewHostWindow) {
    if let Ok(parent) = validate_host(host) {
        let previous = HOST_FOCUS.load(Ordering::Relaxed);
        let target = if previous != 0 && unsafe { IsWindow(previous) } != 0
            && (previous == parent || unsafe { IsChild(parent, previous) } != 0) { previous } else { parent };
        unsafe { SetFocus(target); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_child_does_not_accept_closed_or_zero_host() {
        assert!(validate_host(&QuickPreviewHostWindow { hwnd: "0".into(), process_id: 1 }).is_err());
        assert!(validate_host(&QuickPreviewHostWindow { hwnd: "invalid".into(), process_id: 1 }).is_err());
    }
    #[test]
    fn native_child_uses_creation_parent_not_reparenting() {
        let source = include_str!("native_child.rs");
        let implementation = source.split("#[cfg(test)]").next().unwrap();
        assert!(implementation.contains("with_parent_window"));
        assert!(!implementation.contains("SetParent"));
        assert!(!implementation.contains("position_top_level_overlay"));
    }
    #[test]
    fn native_child_hide_bypasses_stale_winit_visibility_without_reparenting() {
        let source = include_str!("native_child.rs");
        let implementation = source.split("#[cfg(test)]").next().unwrap();
        assert!(implementation.contains("ShowWindow(hwnd, SW_HIDE)"),
            "AttachEmbedded uses SWP_SHOWWINDOW; hide must dispatch directly to Win32");
        assert!(!implementation.contains("window.set_visible(false)"),
            "the builder's cached false state does not track SetWindowPos visibility");
        assert!(!implementation.contains("DestroyWindow"));
        assert!(!implementation.contains("SetParent"));
        let hide = implementation.split("pub(super) fn hide(").nth(1).unwrap();
        assert!(hide.contains("verify_relationship(hwnd, parent)?"));
        assert!(hide.contains("focus_host(host)"));
    }
    #[test]
    fn native_child_window_close_and_escape_use_same_uncached_hide() {
        let source = include_str!("native_viewer.rs");
        let quick_run = source.split("pub fn run(").nth(1).unwrap();
        let close = quick_run.split("WindowEvent::CloseRequested =>").nth(1).unwrap()
            .split("WindowEvent::Resized").next().unwrap();
        let escape = quick_run.split("PhysicalKey::Code(KeyCode::Escape) =>").nth(1).unwrap()
            .split("PhysicalKey::Code(KeyCode::KeyR)").next().unwrap();
        for route in [close, escape] {
            assert!(route.contains("hide_quick_preview_window("));
            assert!(!route.contains("window.set_visible(false)"));
            assert!(route.contains("presentation_mode = PersistentPresentationMode::Hidden"));
            assert!(route.contains("publish_viewer_hidden_event"));
        }
    }
    #[test]
    fn native_child_hidden_or_minimized_host_blocks_show() {
        assert!(host_allows_presentation(true, false));
        assert!(!host_allows_presentation(false, false));
        assert!(!host_allows_presentation(true, true));
        assert!(!host_allows_presentation(false, true));
        let source = include_str!("native_child.rs");
        let implementation = source.split("#[cfg(test)]").next().unwrap();
        let attach = implementation.split("ViewerSessionCommandKind::AttachEmbedded | ViewerSessionCommandKind::Resize =>")
            .nth(1).unwrap().split("ViewerSessionCommandKind::Hide |").next().unwrap();
        let guard = attach.find("if !host_allows_presentation(").unwrap();
        let show = attach.find("SetWindowPos(hwnd").unwrap();
        assert!(guard < show);
        let blocked_branch = &attach[guard..show];
        assert!(blocked_branch.contains("IsWindowVisible(parent)"));
        assert!(blocked_branch.contains("IsIconic(parent)"));
        assert!(blocked_branch.contains("hide(window, host)?"));
        assert!(blocked_branch.contains("return Ok(ViewerPresentationCommandOutcome::Applied(ViewerSessionState::Hidden))"));
    }
    #[test]
    fn native_child_poll_clears_visibility_without_restore_or_unsolicited_ack() {
        let source = include_str!("native_child.rs");
        let guard = source.split("pub(super) fn host_needs_hide(").nth(1).unwrap()
            .split("pub(super) fn hide(").next().unwrap();
        assert!(guard.contains("mode == PersistentPresentationMode::Embedded"));
        assert!(guard.contains("!host_allows_presentation("));
        assert!(guard.contains("IsWindowVisible(parent)"));
        assert!(guard.contains("IsIconic(parent)"));
        let viewer = include_str!("native_viewer.rs");
        let run = viewer.split("pub fn run(").nth(1).unwrap();
        let poll = run.split("Event::AboutToWait =>").nth(1).unwrap()
            .split("if let Some(watcher) = session_watcher.as_mut()").next().unwrap();
        assert!(poll.contains("native_child::host_needs_hide(parent, presentation_mode)"));
        assert!(poll.contains("native_child::hide(window.as_ref(), host)"));
        assert!(poll.contains("presentation_mode = PersistentPresentationMode::Hidden"));
        assert!(!poll.contains("publish_viewer_hidden_event"));
        assert!(!poll.contains("complete_viewer_session_command"));
        assert!(!poll.contains("set_visible(true)"));
        assert!(!poll.contains("ViewerState::new"));
        assert!(!poll.contains("SetParent"));
    }
}
