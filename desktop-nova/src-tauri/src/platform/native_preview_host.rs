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

pub(crate) fn host_window(window: &tauri::WebviewWindow) -> Result<QuickPreviewHostWindow, String> {
    #[cfg(windows)]
    {
        return Ok(QuickPreviewHostWindow {
            hwnd: (window.hwnd().map_err(|error| error.to_string())?.0 as isize).to_string(),
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
