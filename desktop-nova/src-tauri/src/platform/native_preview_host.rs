//! Native Host identities. Window-system resource IDs are not process pointers.
use viewer_launch_contract::QuickPreviewHostWindow;

#[cfg(target_os = "macos")]
pub(crate) use crate::native_macos::NativePresentationMode;

#[cfg(any(target_os="macos", test))]
#[derive(Default)]
struct NativeOwnerLease {
    owner: Option<String>,
    pending: Option<u64>,
    epoch: u64,
}

#[cfg(any(target_os="macos", test))]
impl NativeOwnerLease {
    fn begin(&mut self, epoch: u64) {
        self.owner = None;
        self.pending = Some(epoch);
        self.epoch = epoch;
    }
    fn complete(&mut self, pending: u64, owner: String) -> bool {
        if self.pending != Some(pending) { return false; }
        self.pending = None;
        self.owner = Some(owner);
        true
    }
    fn advance(&mut self, owner: &str, epoch: u64) -> Result<u64, String> {
        if self.owner.as_deref() != Some(owner) { return Err("预览已切换，旧页面显示请求已忽略".into()); }
        if epoch <= self.epoch { return Err("原生显示请求已过期".into()); }
        self.epoch = epoch;
        Ok(epoch)
    }
}

#[cfg(target_os="macos")]
static NEXT_NATIVE_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
#[cfg(target_os="macos")]
static NATIVE_OWNER: std::sync::Mutex<NativeOwnerLease> = std::sync::Mutex::new(NativeOwnerLease { owner: None, pending: None, epoch: 0 });
#[cfg(target_os="macos")]
thread_local! { static MAC_VIEW: std::cell::RefCell<Option<crate::native_macos::MacNativeView>> = const { std::cell::RefCell::new(None) }; }

#[cfg(target_os="macos")]
fn next_epoch() -> u64 { NEXT_NATIVE_EPOCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst) }

#[cfg(target_os="macos")]
pub(crate) fn advance_owner(owner: &str) -> Result<u64,String> {
    NATIVE_OWNER.lock().map_err(|_| "原生预览状态锁异常")?.advance(owner, next_epoch())
}

#[cfg(target_os="macos")]
fn mac_on_main<T: Send + 'static>(window: &tauri::WebviewWindow,
    operation: impl FnOnce(&mut crate::native_macos::MacNativeView) -> Result<T,String> + Send + 'static,
) -> Result<T,String> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let host = window.clone();
    window.run_on_main_thread(move || {
        let result = MAC_VIEW.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_none() {
                let parent = host.ns_view().map_err(|error| error.to_string())?;
                // NSView stays on this Host's UI thread; only a Mach service
                // capability is serialized into the Viewer launch contract.
                *slot = Some(unsafe { crate::native_macos::create(parent, None)? });
            }
            operation(slot.as_mut().unwrap())
        });
        let _ = sender.send(result);
    }).map_err(|error| error.to_string())?;
    receiver.recv_timeout(std::time::Duration::from_secs(5)).map_err(|_| "原生视口主线程响应超时".to_owned())?
}

#[cfg(target_os="macos")]
pub(crate) fn begin_owner(window: &tauri::WebviewWindow) -> Result<u64,String> {
    let epoch = {
        let mut lease = NATIVE_OWNER.lock().map_err(|_| "原生预览状态锁异常")?;
        let epoch = next_epoch(); lease.begin(epoch); epoch
    };
    mac_on_main(window, move |view| view.hide(epoch))?;
    Ok(epoch)
}

#[cfg(target_os="macos")]
pub(crate) fn complete_owner(pending: u64, owner: String) -> bool {
    NATIVE_OWNER.lock().is_ok_and(|mut lease| lease.complete(pending, owner))
}

#[cfg(target_os="macos")]
pub(crate) fn begin_current_owner(window: &tauri::WebviewWindow, owner: &str) -> Result<u64,String> {
    let epoch = {
        let mut lease = NATIVE_OWNER.lock().map_err(|_| "原生预览状态锁异常")?;
        if lease.owner.as_deref() != Some(owner) { return Err("预览已切换，请重新加载".into()); }
        let epoch = next_epoch(); lease.begin(epoch); epoch
    };
    mac_on_main(window, move |view| view.hide(epoch))?;
    Ok(epoch)
}

#[cfg(target_os="macos")]
pub(crate) fn hide_owner(window: &tauri::WebviewWindow, owner: &str) -> Result<(),String> {
    let epoch = advance_owner(owner)?;
    mac_on_main(window, move |view| view.hide(epoch))
}

#[cfg(target_os="macos")]
pub(crate) fn suspend_current(window: &tauri::WebviewWindow) -> Result<(),String> {
    let epoch = {
        let mut lease = NATIVE_OWNER.lock().map_err(|_| "原生预览状态锁异常")?;
        let epoch = next_epoch(); lease.epoch = epoch; epoch
    };
    mac_on_main(window, move |view| view.hide(epoch))
}

#[cfg(target_os="macos")]
pub(crate) fn apply(window: &tauri::WebviewWindow, owner: String,
    placement: Option<crate::application::model::PresentationPlacement>, epoch: Option<u64>, visible: bool,
    mode: NativePresentationMode,
) -> Result<(),String> {
    let Some(epoch) = epoch else { return Ok(()); };
    let origin = window.inner_position().map_err(|error| error.to_string())?;
    mac_on_main(window, move |view| {
        let current = NATIVE_OWNER.lock().map_err(|_| "原生预览状态锁异常")?;
        if current.owner.as_deref() != Some(owner.as_str()) || current.epoch != epoch { return Ok(()); }
        drop(current);
        view.set_mode(mode)?;
        if mode == NativePresentationMode::Popup {
            let rect = view.popup_placement()?;
            return view.apply(rect, epoch, visible, true);
        }
        if let Some(rect) = placement {
            let placement = crate::native_macos::PresentationPlacement {
                x: f64::from(rect.x) - f64::from(origin.x), y: f64::from(rect.y) - f64::from(origin.y),
                width: rect.width, height: rect.height, scale: rect.dpi_scale,
            };
            view.apply(placement, epoch, visible, rect.focused)
        } else { view.hide(epoch) }
    })
}

#[cfg(target_os="macos")]
pub(crate) fn dispose_current() {
    let _ = MAC_VIEW.try_with(|slot| {
        if let Some(mut view) = slot.borrow_mut().take() { let _ = view.dispose(); }
    });
}

#[cfg(any(target_os = "linux", test))]
fn linux_windowing_plan(display: Option<&std::ffi::OsStr>, wayland: Option<&std::ffi::OsStr>) -> Result<&'static str, String> {
    if !display.is_some_and(|value| !value.is_empty()) {
        return Err("Linux 原生预览需要 X11 或 XWayland（DISPLAY 缺失）；请启用桌面的 XWayland 兼容组件后启动 Nova。".into());
    }
    Ok(if wayland.is_some_and(|value| !value.is_empty()) { "xwayland" } else { "x11" })
}

/// Run before GTK/Tauri initialization; only this process and its supervised
/// children use X11. No user/system environment or desktop setting is changed.
pub(crate) fn configure_process_graphics() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let display = std::env::var_os("DISPLAY");
        let wayland = std::env::var_os("WAYLAND_DISPLAY");
        let mode = linux_windowing_plan(display.as_deref(), wayland.as_deref())?;
        std::env::set_var("GDK_BACKEND", "x11");
        eprintln!("[NOVA_NATIVE_HOST] platform=linux presentation={mode} gtk_backend=x11 viewport=child native_wayland=false");
    }
    Ok(())
}

/// Publish only the presentation background, never a scene/render setting.
/// The GPU child composites over this color because its sibling-clipped WebView
/// cannot supply pixels behind it. No Host redirection styles are changed.
pub(crate) fn set_background(window: &tauri::WebviewWindow, color: [u8; 4]) -> Result<(), String> {
    if window.label() != "main" { return Ok(()); }
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindow, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, SetPropW,
            GW_CHILD, GW_HWNDNEXT, WM_PAINT,
        };
        let parent = window.hwnd().map_err(|error| error.to_string())?.0
            as windows_sys::Win32::Foundation::HWND;
        let name: Vec<u16> = "Litematica.Nova.PreviewBackground\0".encode_utf16().collect();
        let argb = 0xff00_0000_u32 | (u32::from(color[0]) << 16)
            | (u32::from(color[1]) << 8) | u32::from(color[2]);
        if unsafe { SetPropW(parent, name.as_ptr(), argb as usize as _) } == 0 {
            return Err(format!("同步原生预览背景失败：{}", std::io::Error::last_os_error()));
        }
        // A theme change must redraw an idle, already-prepared Viewer too.
        // WM_PAINT only asks the existing event loop to present its current
        // attachment; it does not rebuild or replace the projection.
        let mut child = unsafe { GetWindow(parent, GW_CHILD) };
        while !child.is_null() {
            let mut pid = 0;
            if unsafe { GetWindowThreadProcessId(child, &mut pid) } != 0
                && pid != std::process::id() && unsafe { IsWindowVisible(child) } != 0 {
                unsafe { PostMessageW(child, WM_PAINT, 0, 0); }
            }
            child = unsafe { GetWindow(child, GW_HWNDNEXT) };
        }
    }
    #[cfg(not(windows))]
    let _ = color;
    Ok(())
}

/// Keep native GPU siblings outside the Host/WebView repaint regions. Tao can
/// rewrite its parent styles on window-state changes, so callers may reapply
/// this idempotently without moving, raising or activating any window.
#[cfg(windows)]
pub(crate) fn ensure_windows_host_clipping(parent: isize) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetParent, GetWindow, GetWindowThreadProcessId,
        GW_CHILD, GW_HWNDNEXT, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
    };
    let parent = parent as windows_sys::Win32::Foundation::HWND;
    let own_pid = std::process::id();
    let mut parent_pid = 0;
    if unsafe { GetWindowThreadProcessId(parent, &mut parent_pid) } == 0 {
        return Err(format!("读取原生预览主窗口失败：{}", std::io::Error::last_os_error()));
    }
    if parent_pid != own_pid {
        return Err("原生预览主窗口不属于当前程序".into());
    }
    let mut webviews = Vec::new();
    let mut child = unsafe { GetWindow(parent, GW_CHILD) };
    while !child.is_null() {
        let mut pid = 0;
        if unsafe { GetParent(child) } == parent
            && unsafe { GetWindowThreadProcessId(child, &mut pid) } != 0
            && pid == own_pid
        {
            let mut class = [0_u16; 64];
            let length = unsafe { GetClassNameW(child, class.as_mut_ptr(), class.len() as i32) };
            if length == 0 {
                return Err(format!("读取原生预览同胞窗口类型失败：{}", std::io::Error::last_os_error()));
            }
            if class[..length as usize].iter().copied().eq("WRY_WEBVIEW".encode_utf16()) {
                webviews.push(child);
            }
        }
        child = unsafe { GetWindow(child, GW_HWNDNEXT) };
    }
    if webviews.is_empty() {
        return Err("原生预览主窗口的直属 WebView 尚未创建".into());
    }
    add_windows_clipping_style(parent, WS_CLIPCHILDREN)?;
    for webview in webviews {
        add_windows_clipping_style(webview, WS_CLIPSIBLINGS)?;
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn windows_clipping_needs_refresh(event: &tauri::WindowEvent) -> bool {
    matches!(event,
        tauri::WindowEvent::Resized(_)
        | tauri::WindowEvent::ScaleFactorChanged { .. }
        | tauri::WindowEvent::Focused(true))
}

#[cfg(windows)]
fn add_windows_clipping_style(
    hwnd: windows_sys::Win32::Foundation::HWND,
    clipping: u32,
) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{GetLastError, SetLastError};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    };
    unsafe {
        SetLastError(0);
        let before = GetWindowLongPtrW(hwnd, GWL_STYLE);
        if before == 0 && GetLastError() != 0 {
            return Err(format!("读取原生预览裁剪样式失败：{}", std::io::Error::last_os_error()));
        }
        let after = before | clipping as isize;
        if after == before { return Ok(()); }
        SetLastError(0);
        if SetWindowLongPtrW(hwnd, GWL_STYLE, after) == 0 && GetLastError() != 0 {
            return Err(format!("设置原生预览裁剪样式失败：{}", std::io::Error::last_os_error()));
        }
        if SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) == 0
        {
            return Err(format!("应用原生预览裁剪样式失败：{}", std::io::Error::last_os_error()));
        }
        eprintln!("[NOVA_NATIVE_HOST] hwnd={} clipping_style_before={before:#x} clipping_style_after={after:#x} move=false resize=false zorder_change=false activate=false", hwnd as isize);
    }
    Ok(())
}

pub(crate) fn host_window(window: &tauri::WebviewWindow) -> Result<QuickPreviewHostWindow, String> {
    #[cfg(windows)]
    {
        let hwnd = window.hwnd().map_err(|error| error.to_string())?.0 as isize;
        ensure_windows_host_clipping(hwnd)?;
        return Ok(QuickPreviewHostWindow {
            hwnd: hwnd.to_string(),
            process_id: std::process::id(),
         native_composition: None });
    }
    #[cfg(target_os = "linux")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let xid = match window.window_handle().map_err(|error| error.to_string())?.as_raw() {
            RawWindowHandle::Xlib(handle) => u32::try_from(handle.window).map_err(|_| "X11 主窗口标识超出范围")?,
            RawWindowHandle::Xcb(handle) => handle.window.get(),
            _ => return Err("Nova 必须在 GTK 的 X11/XWayland 后端创建真正子窗口；不能把 Wayland 指针传给渲染进程。".into()),
        };
        if xid == 0 { return Err("X11 主窗口尚未创建".into()); }
        return Ok(QuickPreviewHostWindow { hwnd: xid.to_string(), process_id: std::process::id() , native_composition: None });
    }
    #[cfg(target_os="macos")]
    {
        let endpoint = mac_on_main(window, |view| Ok(view.endpoint()))?;
        return Ok(QuickPreviewHostWindow { hwnd: "0".into(), process_id: std::process::id(), native_composition: Some(endpoint) });
    }
    #[cfg(not(any(windows, target_os = "linux", target_os="macos")))]
    {
        let _ = window;
        Err("当前平台原生共享视口尚未初始化".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[cfg(windows)]
    #[test]
    fn windows_host_clipping_preserves_hidden_windows_and_is_idempotent() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        static POSITION_EVENTS: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "system" fn procedure(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
            if message == WM_WINDOWPOSCHANGED { POSITION_EVENTS.fetch_add(1, Ordering::SeqCst); }
            DefWindowProcW(hwnd, message, w, l)
        }
        struct HiddenParent(HWND);
        impl Drop for HiddenParent {
            fn drop(&mut self) { unsafe { DestroyWindow(self.0); } }
        }
        unsafe fn snapshot(hwnd: HWND) -> (isize, isize, [i32; 4], isize, isize, isize, i32) {
            let mut rect: RECT = std::mem::zeroed();
            assert_ne!(GetWindowRect(hwnd, &mut rect), 0);
            (GetWindowLongPtrW(hwnd, GWL_STYLE), GetWindowLongPtrW(hwnd, GWL_EXSTYLE),
                [rect.left, rect.top, rect.right, rect.bottom], GetParent(hwnd) as isize,
                GetWindow(hwnd, GW_HWNDPREV) as isize, GetWindow(hwnd, GW_HWNDNEXT) as isize,
                IsWindowVisible(hwnd))
        }
        unsafe {
            let class = "WRY_WEBVIEW\0".encode_utf16().collect::<Vec<_>>();
            let static_class = "STATIC\0".encode_utf16().collect::<Vec<_>>();
            let instance = GetModuleHandleW(std::ptr::null());
            let mut definition: WNDCLASSW = std::mem::zeroed();
            definition.lpfnWndProc = Some(procedure);
            definition.hInstance = instance;
            definition.lpszClassName = class.as_ptr();
            assert_ne!(RegisterClassW(&definition), 0, "{}", std::io::Error::last_os_error());
            let create = |name: *const u16, style, parent| CreateWindowExW(0, name, std::ptr::null(),
                style, 3, 5, 160, 120, parent, std::ptr::null_mut(), instance, std::ptr::null());
            // WS_VISIBLE is absent everywhere; no window is shown or rendered.
            let parent = HiddenParent(create(class.as_ptr(), WS_OVERLAPPEDWINDOW, std::ptr::null_mut()));
            assert!(!parent.0.is_null());
            let webview = create(class.as_ptr(), WS_CHILD | WS_CLIPCHILDREN | WS_TABSTOP, parent.0);
            let sibling = create(static_class.as_ptr(), WS_CHILD | WS_TABSTOP, parent.0);
            let nested = create(class.as_ptr(), WS_CHILD, sibling);
            assert!(!webview.is_null() && !sibling.is_null() && !nested.is_null());
            let before_parent = snapshot(parent.0);
            let before_webview = snapshot(webview);
            let before_sibling = snapshot(sibling);
            let before_nested = snapshot(nested);
            ensure_windows_host_clipping(parent.0 as isize).unwrap();
            let mut expected_parent = before_parent;
            expected_parent.0 |= WS_CLIPCHILDREN as isize;
            let mut expected_webview = before_webview;
            expected_webview.0 |= WS_CLIPSIBLINGS as isize;
            assert_eq!(snapshot(parent.0), expected_parent);
            assert_eq!(snapshot(webview), expected_webview);
            assert_eq!(snapshot(sibling), before_sibling);
            assert_eq!(snapshot(nested), before_nested, "only direct WRY siblings may change");
            POSITION_EVENTS.store(0, Ordering::SeqCst);
            ensure_windows_host_clipping(parent.0 as isize).unwrap();
            ensure_windows_host_clipping(parent.0 as isize).unwrap();
            assert_eq!(POSITION_EVENTS.load(Ordering::SeqCst), 0, "idempotent calls must not repeat SetWindowPos");
            assert_eq!(snapshot(parent.0), expected_parent);
            assert_eq!(snapshot(webview), expected_webview);
            // Reproduce Tao rewriting only the parent style during restore.
            SetWindowLongPtrW(parent.0, GWL_STYLE, before_parent.0);
            ensure_windows_host_clipping(parent.0 as isize).unwrap();
            assert_eq!(snapshot(parent.0), expected_parent);
            assert_eq!(snapshot(webview), expected_webview);
            assert_eq!(snapshot(sibling), before_sibling);
            assert_eq!(snapshot(nested), before_nested);
            // The WebView may independently lose its sibling-clipping bit;
            // restoring it must not rewrite the already-correct parent.
            SetWindowLongPtrW(webview, GWL_STYLE, before_webview.0);
            assert_eq!(snapshot(parent.0), expected_parent);
            ensure_windows_host_clipping(parent.0 as isize).unwrap();
            assert_eq!(snapshot(parent.0), expected_parent);
            assert_eq!(snapshot(webview), expected_webview);
            assert_eq!(snapshot(sibling), before_sibling);
            assert_eq!(snapshot(nested), before_nested);
            let size = |width, height| tauri::PhysicalSize::new(width, height);
            for (name, event, extent) in [
                ("route_restore", tauri::WindowEvent::Focused(true), [854, 480]),
                ("minimize_event", tauri::WindowEvent::Resized(size(0, 0)), [0, 0]),
                ("restore_event", tauri::WindowEvent::Resized(size(854, 480)), [854, 480]),
                ("maximize_event", tauri::WindowEvent::Resized(size(1920, 1080)), [1920, 1080]),
                ("resize_event", tauri::WindowEvent::Resized(size(1234, 789)), [1234, 789]),
                ("dpi_event", tauri::WindowEvent::from(tauri_runtime::window::WindowEvent::ScaleFactorChanged {
                    scale_factor: 1.5, new_inner_size: size(1281, 720),
                }), [1281, 720]),
            ] {
                // Exercise only owned, never-shown HWNDs. A zero-size event
                // represents minimization without invoking ShowWindow/min/max.
                assert_ne!(SetWindowPos(parent.0, std::ptr::null_mut(), 0, 0, extent[0], extent[1],
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE), 0);
                assert_ne!(SetWindowPos(webview, std::ptr::null_mut(), 7, 11, extent[0], extent[1],
                    SWP_NOZORDER | SWP_NOACTIVATE), 0);
                SetWindowLongPtrW(parent.0, GWL_STYLE,
                    GetWindowLongPtrW(parent.0, GWL_STYLE) & !(WS_CLIPCHILDREN as isize));
                SetWindowLongPtrW(webview, GWL_STYLE,
                    GetWindowLongPtrW(webview, GWL_STYLE) & !(WS_CLIPSIBLINGS as isize));
                let mut expected_parent = snapshot(parent.0);
                let mut expected_webview = snapshot(webview);
                let expected_sibling = snapshot(sibling);
                let expected_nested = snapshot(nested);
                expected_parent.0 |= WS_CLIPCHILDREN as isize;
                expected_webview.0 |= WS_CLIPSIBLINGS as isize;
                assert!(windows_clipping_needs_refresh(&event), "{name}");
                ensure_windows_host_clipping(parent.0 as isize).unwrap();
                assert_eq!(snapshot(parent.0), expected_parent, "{name}: parent geometry/style");
                assert_eq!(snapshot(webview), expected_webview, "{name}: WebView geometry/style");
                assert_eq!(snapshot(sibling), expected_sibling, "{name}: native sibling unchanged");
                assert_eq!(snapshot(nested), expected_nested, "{name}: descendants unchanged");
                POSITION_EVENTS.store(0, Ordering::SeqCst);
                ensure_windows_host_clipping(parent.0 as isize).unwrap();
                assert_eq!(POSITION_EVENTS.load(Ordering::SeqCst), 0, "{name}: no repeated position change");
            }
            for event in [tauri::WindowEvent::Focused(false), tauri::WindowEvent::Destroyed,
                tauri::WindowEvent::Moved(tauri::PhysicalPosition::new(100, 200))] {
                assert!(!windows_clipping_needs_refresh(&event));
            }
            assert!(ensure_windows_host_clipping(0).is_err());
            drop(parent);
            assert_ne!(UnregisterClassW(class.as_ptr(), instance), 0);
        }
    }

    #[test]
    fn x11_and_xwayland_use_the_same_real_child_protocol() {
        assert_eq!(linux_windowing_plan(Some(OsStr::new(":0")), None).unwrap(), "x11");
        assert_eq!(linux_windowing_plan(Some(OsStr::new(":1")), Some(OsStr::new("wayland-0"))).unwrap(), "xwayland");
    }
    #[test]
    fn missing_xwayland_does_not_fall_back_to_an_overlay_or_foreign_pointer() {
        assert!(linux_windowing_plan(None, Some(OsStr::new("wayland-0"))).is_err());
        assert!(linux_windowing_plan(Some(OsStr::new("")), None).is_err());
    }
    #[test]
    fn late_owner_completion_and_presentation_cannot_take_the_new_viewport() {
        let mut lease = NativeOwnerLease::default();
        lease.begin(1);
        lease.begin(2);
        assert!(!lease.complete(1, "old".into()));
        assert!(lease.complete(2, "current".into()));
        assert!(lease.advance("old", 3).is_err());
        assert_eq!(lease.epoch, 2);
        assert_eq!(lease.advance("current", 4).unwrap(), 4);
        lease.begin(5);
        assert!(lease.advance("current", 6).is_err());
    }
}
