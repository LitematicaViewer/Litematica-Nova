//! An X11 child created inside the Host from its first XCreateWindow.
//! The XID is a server resource ID, never a pointer into the Host process.
use super::*;
use std::sync::{OnceLock, atomic::AtomicU32};
use viewer_launch_contract::QuickPreviewHostWindow;
use winit::{platform::x11::WindowBuilderExtX11, raw_window_handle::{HasWindowHandle, RawWindowHandle}};
use x11rb::{connection::Connection, protocol::xproto::{AtomEnum, ConfigureWindowAux, ConnectionExt, InputFocus, MapState}, rust_connection::RustConnection};

struct HostConnection {
    connection: RustConnection,
    pid_atom: u32,
}
static HOST_FOCUS: AtomicU32 = AtomicU32::new(0);

fn is_host_descendant(display: &HostConnection, mut child: u32, parent: u32) -> bool {
    for _ in 0..64 {
        if child == parent { return true; }
        let Ok(cookie) = display.connection.query_tree(child) else { return false; };
        let Ok(tree) = cookie.reply() else { return false; };
        if child == tree.root || tree.parent == 0 || tree.parent == child { return false; }
        child = tree.parent;
    }
    false
}

fn with_connection<T>(operation: impl FnOnce(&HostConnection) -> Result<T>) -> Result<T> {
    static CONNECTION: OnceLock<Mutex<Option<HostConnection>>> = OnceLock::new();
    let mut connection = CONNECTION.get_or_init(|| Mutex::new(None)).lock()
        .map_err(|_| anyhow!("native X11 Host connection lock failed"))?;
    if connection.is_none() {
        let (display, _) = x11rb::connect(None).context("connect native X11 Host display")?;
        let pid_atom = display.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom;
        *connection = Some(HostConnection { connection: display, pid_atom });
    }
    operation(connection.as_ref().unwrap())
}

fn parent_id(host: &QuickPreviewHostWindow) -> Result<u32> {
    let parent = host.hwnd.parse::<u32>().context("invalid native X11 Host XID")?;
    ensure!(parent != 0 && host.process_id != 0, "native X11 Host identity is zero");
    Ok(parent)
}

pub(super) fn validate_host(host: &QuickPreviewHostWindow) -> Result<u32> {
    let parent = parent_id(host)?;
    with_connection(|display| {
        display.connection.get_window_attributes(parent)?.reply()
            .context("native X11 Host window has closed")?;
        let pid = display.connection.get_property(false, parent, display.pid_atom, AtomEnum::CARDINAL, 0, 1)?.reply()?;
        ensure!(pid.value32().and_then(|mut values| values.next()) == Some(host.process_id),
            "native X11 Host no longer belongs to the launch process");
        Ok(parent)
    })
}

pub(super) fn builder(builder: WindowBuilder, host: &QuickPreviewHostWindow) -> Result<WindowBuilder> {
    let parent = validate_host(host)?;
    with_connection(|display| {
        let focused = display.connection.get_input_focus()?.reply()?.focus;
        HOST_FOCUS.store(if is_host_descendant(display, focused, parent) { focused } else { parent }, Ordering::Relaxed);
        Ok(())
    })?;
    Ok(builder.with_embed_parent_window(parent as _).with_decorations(false)
        .with_visible(false).with_active(false))
}

fn window_id(window: &winit::window::Window) -> Result<u32> {
    match window.window_handle()?.as_raw() {
        RawWindowHandle::Xlib(handle) => u32::try_from(handle.window).context("X11 window ID overflow"),
        RawWindowHandle::Xcb(handle) => Ok(handle.window.get()),
        _ => bail!("native X11 child requires the X11 event loop, not a Wayland surface"),
    }
}

fn verify_relationship(child: u32, parent: u32) -> Result<()> {
    with_connection(|display| {
        ensure!(display.connection.query_tree(child)?.reply()?.parent == parent,
            "native X11 preview parent relationship changed");
        Ok(())
    })
}

pub(super) fn verify(window: &winit::window::Window, host: &QuickPreviewHostWindow) -> Result<()> {
    let parent = validate_host(host)?;
    let child = window_id(window)?;
    verify_relationship(child, parent)?;
    println!("[QUICK_NATIVE_CHILD] platform=x11 parent={parent} window={child} child=true top_level=false reparent=false gpu_surface=existing-wgpu");
    Ok(())
}

pub(super) fn host_needs_hide(parent: u32, mode: PersistentPresentationMode) -> bool {
    mode == PersistentPresentationMode::Embedded && !with_connection(|display| {
        Ok(display.connection.get_window_attributes(parent)?.reply()?.map_state == MapState::VIEWABLE)
    }).unwrap_or(false)
}

pub(super) fn hide(window: &winit::window::Window, host: &QuickPreviewHostWindow) -> Result<()> {
    let child = window_id(window)?;
    verify_relationship(child, validate_host(host)?)?;
    let focused = with_connection(|display| {
        let focused = display.connection.get_input_focus()?.reply()?.focus == child;
        // Direct protocol dispatch is required: winit's initial visible=false
        // does not track the explicit map request used by AttachEmbedded.
        display.connection.unmap_window(child)?.check()?;
        display.connection.flush()?;
        Ok(focused)
    })?;
    if focused { focus_host(host); }
    Ok(())
}

pub(super) fn focus_host(host: &QuickPreviewHostWindow) {
    if let Ok(parent) = validate_host(host) {
        let _ = with_connection(|display| {
            let previous = HOST_FOCUS.load(Ordering::Relaxed);
            let target = if previous > 1 && is_host_descendant(display, previous, parent) { previous } else { parent };
            display.connection.set_input_focus(InputFocus::PARENT, target, x11rb::CURRENT_TIME)?.check()?;
            display.connection.flush()?;
            Ok(())
        });
    }
}

pub(super) fn apply(window: &winit::window::Window, host: &QuickPreviewHostWindow,
    command: &ViewerSessionCommand, mode: &mut PersistentPresentationMode,
) -> Result<ViewerPresentationCommandOutcome> {
    let parent = validate_host(host)?;
    let child = window_id(window)?;
    verify_relationship(child, parent)?;
    match command.kind {
        ViewerSessionCommandKind::AttachEmbedded | ViewerSessionCommandKind::Resize => {
            if let Some(requested) = command.host_hwnd.as_deref() {
                ensure!(requested == host.hwnd, "native X11 child cannot change its Host");
            }
            let width = command.width.context("native X11 child width missing")?;
            let height = command.height.context("native X11 child height missing")?;
            ensure!(width > 0 && width <= u16::MAX.into() && height > 0 && height <= u16::MAX.into(),
                "native X11 child dimensions exceed the protocol range");
            if host_needs_hide(parent, PersistentPresentationMode::Embedded) {
                hide(window, host)?;
                *mode = PersistentPresentationMode::Hidden;
                return Ok(ViewerPresentationCommandOutcome::Applied(ViewerSessionState::Hidden));
            }
            with_connection(|display| {
                let (mut x, mut y) = (command.screen_x.unwrap_or(0), command.screen_y.unwrap_or(0));
                if command.screen_x.is_some() {
                    let root = display.connection.query_tree(parent)?.reply()?.root;
                    let origin = display.connection.translate_coordinates(root, parent,
                        i16::try_from(x).context("X11 screen x exceeds protocol range")?,
                        i16::try_from(y).context("X11 screen y exceeds protocol range")?)?.reply()?;
                    (x, y) = (origin.dst_x.into(), origin.dst_y.into());
                }
                display.connection.configure_window(child, &ConfigureWindowAux::new().x(x).y(y).width(width).height(height))?.check()?;
                // Resize alone must never resurrect a viewport hidden by routing.
                if command.kind == ViewerSessionCommandKind::AttachEmbedded {
                    display.connection.map_window(child)?.check()?;
                    *mode = PersistentPresentationMode::Embedded;
                }
                if command.focused == Some(true) && *mode == PersistentPresentationMode::Embedded {
                    display.connection.set_input_focus(InputFocus::PARENT, child, x11rb::CURRENT_TIME)?.check()?;
                }
                display.connection.flush()?;
                Ok(())
            })?;
            if command.focused == Some(false) { focus_host(host); }
            window.request_redraw();
            Ok(ViewerPresentationCommandOutcome::Applied(ViewerSessionState::Attached { host_hwnd: host.hwnd.clone(), width, height }))
        }
        ViewerSessionCommandKind::Hide | ViewerSessionCommandKind::Release => {
            hide(window, host)?;
            *mode = PersistentPresentationMode::Hidden;
            Ok(if command.kind == ViewerSessionCommandKind::Release {
                ViewerPresentationCommandOutcome::Release(ViewerSessionState::Released)
            } else { ViewerPresentationCommandOutcome::Applied(ViewerSessionState::Hidden) })
        }
        ViewerSessionCommandKind::ShowPopup | ViewerSessionCommandKind::ShowOverlay => bail!("native X11 child supports embedded presentation only"),
        ViewerSessionCommandKind::ReplaceScene | ViewerSessionCommandKind::CaptureFrame => Ok(ViewerPresentationCommandOutcome::NotPresentation),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn x11_host_rejects_zero_and_process_local_pointers() {
        for id in ["0", "0x1234", "18446744073709551615", "wl_surface:42"] {
            assert!(parent_id(&QuickPreviewHostWindow { hwnd: id.into(), process_id: 1 }).is_err());
        }
    }
    #[test]
    fn x11_child_uses_creation_parent_and_never_reparents() {
        let source = include_str!("native_child_x11.rs").split("#[cfg(test)]").next().unwrap();
        assert!(source.contains("with_embed_parent_window"));
        assert!(!source.contains("reparent_window"));
        assert!(source.contains("unmap_window(child)"));
    }
}
