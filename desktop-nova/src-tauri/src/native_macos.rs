//! Host-owned NSView/CAMetalLayer presentation for a separate GPU renderer.
//! Only IOSurface Mach capabilities and bounded metadata cross the process
//! boundary. The shader below copies final colour, not scene/rendering rules.
#![cfg(target_os = "macos")]

#[path = "native_macos_wire.rs"]
pub mod wire;

use core_graphics_types::geometry::CGSize;
use foreign_types::{ForeignType, ForeignTypeRef};
use metal::{
    CommandQueue, Device, MTLClearColor, MTLCommandBufferStatus, MTLLoadAction, MTLPixelFormat,
    MTLPrimitiveType, MTLStorageMode, MTLStoreAction, MTLTextureType, MTLTextureUsage, MetalLayer,
    RenderPassDescriptor, RenderPipelineDescriptor, RenderPipelineState, Texture,
    TextureDescriptor,
};
use objc::{
    class,
    declare::ClassDecl,
    msg_send,
    runtime::{Class, Object, Sel, BOOL, NO, YES},
    sel, sel_impl, Encode, Encoding,
};
use serde::Serialize;
use std::{
    ffi::{c_char, c_void, CStr, CString},
    marker::PhantomData,
    mem::{size_of, zeroed},
    ptr,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
};
use viewer_launch_contract::NativeCompositionEndpoint;
use wire::{AckStatus, FrameGate, FrameHeader, InputPacket};

// TASK_BOOTSTRAP_PORT is a selector, not a Mach port name.
const TASK_BOOTSTRAP_PORT: i32 = 4;
const MACH_SEND_MSG: u32 = 1;
const MACH_RCV_MSG: u32 = 2;
const MACH_SEND_TIMEOUT: u32 = 0x10;
const MACH_RCV_TIMEOUT: u32 = 0x100;
const MACH_RCV_TIMED_OUT: i32 = 0x1000_4003;
const MACH_MSGH_BITS_COMPLEX: u32 = 0x8000_0000;
const COPY_SEND: u8 = 19;
const MOVE_SEND_ONCE: u8 = 18;
const FRAME_WIRE_SIZE: u32 = 24 + 4 + 2 * 12 + wire::FRAME_BYTES as u32;
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[repr(C)]
#[derive(Clone, Copy)]
struct MachHeader {
    bits: u32,
    size: u32,
    remote: u32,
    local: u32,
    voucher: u32,
    id: i32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct PortDescriptor {
    name: u32,
    pad1: u32,
    pad2: u16,
    disposition: u8,
    kind: u8,
}
#[repr(C)]
struct FrameMessage {
    header: MachHeader,
    count: u32,
    ports: [PortDescriptor; 2],
    payload: [u8; wire::FRAME_BYTES],
    trailer: [u8; 512],
}
#[repr(C)]
struct DataMessage<const N: usize> {
    header: MachHeader,
    payload: [u8; N],
}

#[link(name = "System")]
extern "C" {
    static mach_task_self_: u32;
    fn task_get_special_port(task: u32, which_port: i32, port: *mut u32) -> i32;
    fn mach_port_allocate(task: u32, right: i32, name: *mut u32) -> i32;
    fn mach_port_insert_right(task: u32, name: u32, right: u32, disposition: u32) -> i32;
    fn mach_port_deallocate(task: u32, name: u32) -> i32;
    fn mach_port_destroy(task: u32, name: u32) -> i32;
    fn bootstrap_register(bootstrap: u32, name: *const c_char, service: u32) -> i32;
    fn mach_msg(
        message: *mut MachHeader,
        options: u32,
        send_size: u32,
        receive_size: u32,
        receive_name: u32,
        timeout_ms: u32,
        notify: u32,
    ) -> i32;
    fn mach_msg_destroy(message: *mut MachHeader);
    static _dispatch_main_q: u8;
    fn dispatch_async_f(
        queue: *const c_void,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
}
#[link(name = "IOSurface", kind = "framework")]
extern "C" {
    fn IOSurfaceLookupFromMachPort(port: u32) -> *mut c_void;
    fn IOSurfaceGetWidth(surface: *const c_void) -> usize;
    fn IOSurfaceGetHeight(surface: *const c_void) -> usize;
    fn IOSurfaceGetBytesPerRow(surface: *const c_void) -> usize;
    fn IOSurfaceGetAllocSize(surface: *const c_void) -> usize;
    fn IOSurfaceGetPixelFormat(surface: *const c_void) -> u32;
    fn IOSurfaceGetPlaneCount(surface: *const c_void) -> usize;
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(value: *const c_void);
}
#[link(name = "Security", kind = "framework")]
extern "C" {
    static kSecRandomDefault: *const c_void;
    fn SecRandomCopyBytes(random: *const c_void, count: usize, bytes: *mut u8) -> i32;
}
#[link(name = "AppKit", kind = "framework")]
extern "C" {}

#[derive(Clone, Copy)]
#[repr(C)]
struct Point {
    x: f64,
    y: f64,
}
#[derive(Clone, Copy)]
#[repr(C)]
struct Size {
    width: f64,
    height: f64,
}
#[derive(Clone, Copy)]
#[repr(C)]
struct Rect {
    origin: Point,
    size: Size,
}
unsafe impl Encode for Point {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{CGPoint=dd}") }
    }
}
unsafe impl Encode for Size {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{CGSize=dd}") }
    }
}
unsafe impl Encode for Rect {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{CGRect={CGPoint=dd}{CGSize=dd}}") }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresentationPlacement {
    /// Parent-content-relative top-left physical pixels, not screen coordinates.
    pub x: f64,
    pub y: f64,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeInput {
    pub generation: u64,
    pub epoch: u64,
    pub sequence: u64,
    pub kind: u32,
    pub flags: u32,
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
    pub code: u32,
    pub modifiers: u32,
    pub text: String,
}
pub type InputSink = Arc<dyn Fn(NativeInput) + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativePresentationMode { Embedded, Popup }

struct SendRight(u32);
impl Drop for SendRight {
    fn drop(&mut self) {
        unsafe {
            mach_port_deallocate(mach_task_self_, self.0);
        }
    }
}
struct Shared {
    gate: Mutex<FrameGate>,
    input: Mutex<Option<SendRight>>,
    stopped: AtomicBool,
    // This pointer is used only by Host main-thread callbacks, never in IPC.
    view: AtomicUsize,
    scale: AtomicU64,
    sequence: AtomicU64,
    presented_epoch: AtomicU64,
    reported_epoch: AtomicU64,
    request_focus: AtomicBool,
    popup: AtomicUsize,
    popup_active: AtomicBool,
    popup_minimized: AtomicBool,
    popup_requested_visible: AtomicBool,
    popup_closed_epoch: AtomicU64,
    sink: Option<InputSink>,
}

/// !Send/!Sync: create/apply/hide/dispose belong in the Host main-thread registry.
pub struct MacNativeView {
    view: *mut Object,
    parent: *mut Object,
    popup: *mut Object,
    popup_delegate: *mut Object,
    mode: NativePresentationMode,
    layer: MetalLayer,
    shared: Arc<Shared>,
    _input_context: Box<Arc<Shared>>,
    endpoint: NativeCompositionEndpoint,
    placement: Option<PresentationPlacement>,
    focused: bool,
    _main_thread: PhantomData<Rc<()>>,
}

fn main_thread() -> Result<(), String> {
    let yes: BOOL = unsafe { msg_send![class!(NSThread), isMainThread] };
    if yes == YES {
        Ok(())
    } else {
        Err("native macOS view mutation requires the Host main thread".into())
    }
}

impl MacNativeView {
    pub fn endpoint(&self) -> NativeCompositionEndpoint {
        self.endpoint.clone()
    }

    /// Only an explicit Host presentation command changes the window mode.
    /// The same NSView/layer/IOSurface receiver survives both directions.
    pub fn set_mode(&mut self, mode: NativePresentationMode) -> Result<(), String> {
        main_thread()?;
        if self.mode == mode { return Ok(()); }
        if self.view.is_null() { return Err("native macOS view is disposed".into()); }
        self.shared.popup_active.store(false, Ordering::Release);
        self.shared.popup_requested_visible.store(false, Ordering::Release);
        if let Ok(mut gate) = self.shared.gate.lock() { gate.visible = false; }
        self.shared.presented_epoch.store(u64::MAX, Ordering::Release);
        unsafe {
            let _: () = msg_send![self.view, setHidden: YES];
            if mode == NativePresentationMode::Popup {
                if self.popup.is_null() {
                    let size = self.placement.unwrap_or(PresentationPlacement { x: 0.0, y: 0.0, width: 960, height: 640, scale: 1.0 });
                    let parent_window: *mut Object = msg_send![self.parent, window];
                    let screen: *mut Object = msg_send![parent_window, screen];
                    let scale: f64 = msg_send![parent_window, backingScaleFactor];
                    let scale = if scale.is_finite() && scale > 0.0 { scale } else { size.scale };
                    let rect = Rect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: f64::from(size.width) / scale, height: f64::from(size.height) / scale } };
                    let popup: *mut Object = msg_send![class!(NSWindow), alloc];
                    self.popup = msg_send![popup, initWithContentRect: rect styleMask: 15_u64 backing: 2_u64 defer: NO screen: screen];
                    if self.popup.is_null() { return Err("native macOS preview window allocation failed".into()); }
                    let delegate: *mut Object = msg_send![popup_delegate_class(), new];
                    (*delegate).set_ivar::<usize>("novaInput", (&*self._input_context as *const Arc<Shared>) as usize);
                    self.popup_delegate = delegate;
                    let _: () = msg_send![self.popup, setDelegate: delegate];
                    let _: () = msg_send![self.popup, setReleasedWhenClosed: NO];
                    let _: () = msg_send![self.popup, setAcceptsMouseMovedEvents: YES];
                    let _: () = msg_send![self.popup, setOpaque: NO];
                    let clear: *mut Object = msg_send![class!(NSColor), clearColor];
                    let _: () = msg_send![self.popup, setBackgroundColor: clear];
                    let title: *mut Object = msg_send![class!(NSString), stringWithUTF8String: b"Nova Preview\0".as_ptr()];
                    let _: () = msg_send![self.popup, setTitle: title];
                    let _: () = msg_send![self.popup, center];
                    self.shared.popup.store(self.popup as usize, Ordering::Release);
                }
                let content: *mut Object = msg_send![self.popup, contentView];
                let _: () = msg_send![self.view, removeFromSuperview];
                let _: () = msg_send![content, addSubview: self.view];
                self.shared.popup_active.store(true, Ordering::Release);
            } else {
                if !self.popup.is_null() { let _: () = msg_send![self.popup, orderOut: ptr::null_mut::<Object>()]; }
                let _: () = msg_send![self.view, removeFromSuperview];
                let _: () = msg_send![self.parent, addSubview: self.view positioned: 1_isize relativeTo: ptr::null_mut::<Object>()];
            }
        }
        self.mode = mode;
        self.placement = None;
        self.focused = false;
        Ok(())
    }

    pub fn popup_placement(&self) -> Result<PresentationPlacement, String> {
        main_thread()?;
        if self.mode != NativePresentationMode::Popup || self.popup.is_null() { return Err("native popup is not attached".into()); }
        unsafe { popup_placement(self.popup) }
    }

    pub fn apply(
        &mut self,
        mut placement: PresentationPlacement,
        epoch: u64,
        mut visible: bool,
        focused: bool,
    ) -> Result<(), String> {
        main_thread()?;
        if self.view.is_null() {
            return Err("native macOS view is disposed".into());
        }
        if self.mode == NativePresentationMode::Popup {
            placement = self.popup_placement()?;
            if self.shared.popup_closed_epoch.load(Ordering::Acquire) == epoch { visible = false; }
        }
        if !placement.scale.is_finite()
            || placement.scale <= 0.0
            || !placement.x.is_finite()
            || !placement.y.is_finite()
            || placement.width == 0
            || placement.height == 0
            || placement.width > 16_384
            || placement.height > 16_384
            || u64::from(placement.width) * u64::from(placement.height) * 4
                > wire::MAX_SURFACE_BYTES
        {
            return Err("invalid native macOS viewport bounds".into());
        }
        let (previous_epoch, previous_visible, previous_extent) = {
            let gate = self
                .shared
                .gate
                .lock()
                .map_err(|_| "native frame state lock failed")?;
            (gate.epoch, gate.visible, [gate.width, gate.height])
        };
        if epoch < previous_epoch {
            return Ok(());
        }
        if self.mode == NativePresentationMode::Popup {
            self.shared.popup_requested_visible.store(visible, Ordering::Release);
        }
        if self.mode == NativePresentationMode::Popup && self.shared.popup_minimized.load(Ordering::Acquire) {
            if visible && epoch > previous_epoch {
                // A new explicit ShowPopup restores the existing window even
                // after Hide/AttachEmbedded left it miniaturized. Polling the
                // same intent must not undo the user's minimize operation.
                self.shared.popup_minimized.store(false, Ordering::Release);
                unsafe { let _: () = msg_send![self.popup, deminiaturize: ptr::null_mut::<Object>()]; }
            } else {
                visible = false;
            }
        }
        if epoch == previous_epoch
            && previous_visible == visible
            && self.placement == Some(placement)
            && self.focused == focused
        {
            return Ok(());
        }
        // A popup delegate has already updated gate/layer on an OS resize.
        // Comparing the stale DOM placement here would hide its valid one-shot
        // frame a second time when the ordinary status poll arrives.
        let new_surface = epoch != previous_epoch
            || previous_extent != [placement.width, placement.height]
            || f64::from_bits(self.shared.scale.load(Ordering::Acquire)) != placement.scale;
        if new_surface {
            self.shared
                .presented_epoch
                .store(u64::MAX, Ordering::Release);
            self.shared.reported_epoch.store(u64::MAX, Ordering::Release);
        }
        if new_surface || !visible {
            unsafe {
                let _: () = msg_send![self.view, setHidden: YES];
            }
        }
        let mut gate = self
            .shared
            .gate
            .lock()
            .map_err(|_| "native frame state lock failed")?;
        gate.set_presentation(epoch, visible, placement.width, placement.height);
        self.shared.scale.store(placement.scale.to_bits(), Ordering::Release);
        drop(gate);
        self.shared.request_focus.store(
            visible && focused && (!self.focused || !previous_visible || new_surface),
            Ordering::Release,
        );
        unsafe {
            let parent: *mut Object = msg_send![self.view, superview];
            if parent.is_null() {
                return Err("native macOS parent view has closed".into());
            }
            let flipped: BOOL = msg_send![parent, isFlipped];
            let bounds: Rect = msg_send![parent, bounds];
            let width = f64::from(placement.width) / placement.scale;
            let height = f64::from(placement.height) / placement.scale;
            let y = if flipped == YES {
                placement.y / placement.scale
            } else {
                bounds.size.height - placement.y / placement.scale - height
            };
            let rect = Rect {
                origin: Point {
                    x: placement.x / placement.scale,
                    y,
                },
                size: Size { width, height },
            };
            let _: () = msg_send![self.view, setFrame: rect];
            self.layer.set_contents_scale(placement.scale);
            self.layer.set_drawable_size(CGSize::new(
                f64::from(placement.width),
                f64::from(placement.height),
            ));
            self.layer.remove_all_animations();
            if self.mode == NativePresentationMode::Popup {
                if visible {
                    let _: () = msg_send![self.popup, makeKeyAndOrderFront: ptr::null_mut::<Object>()];
                } else if !self.shared.popup_minimized.load(Ordering::Acquire)
                    || !self.shared.popup_requested_visible.load(Ordering::Acquire) {
                    let _: () = msg_send![self.popup, orderOut: ptr::null_mut::<Object>()];
                }
            }
            if visible && self.shared.presented_epoch.load(Ordering::Acquire) == epoch {
                let _: () = msg_send![self.view, setHidden: NO];
            }
            if visible && focused && (!self.focused || !previous_visible) {
                let window: *mut Object = msg_send![self.view, window];
                let did_focus: BOOL = msg_send![window, makeFirstResponder: self.view];
                if did_focus == YES {
                    self.shared.request_focus.store(false, Ordering::Release);
                }
                let _: () = msg_send![window, setAcceptsMouseMovedEvents: YES];
            }
        }
        self.placement = Some(placement);
        self.focused = focused;
        if self.mode == NativePresentationMode::Popup && visible { send_viewport_size(&self.shared); }
        Ok(())
    }

    pub fn hide(&mut self, epoch: u64) -> Result<(), String> {
        main_thread()?;
        let mut gate = self
            .shared
            .gate
            .lock()
            .map_err(|_| "native frame state lock failed")?;
        if epoch < gate.epoch {
            return Ok(());
        }
        let (width, height) = (gate.width, gate.height);
        gate.set_presentation(epoch, false, width, height);
        drop(gate);
        self.shared.request_focus.store(false, Ordering::Release);
        self.shared.popup_requested_visible.store(false, Ordering::Release);
        self.focused = false;
        if !self.view.is_null() {
            unsafe {
                let _: () = msg_send![self.view, setHidden: YES];
                if self.mode == NativePresentationMode::Popup && !self.popup.is_null() {
                    let _: () = msg_send![self.popup, orderOut: ptr::null_mut::<Object>()];
                }
            }
        }
        forward_input(&self.shared, 7, 0, 0.0, 0.0, 0.0, 0.0, 0, 0, String::new());
        Ok(())
    }

    pub fn dispose(&mut self) -> Result<(), String> {
        main_thread()?;
        if self.view.is_null() {
            return Ok(());
        }
        self.shared.stopped.store(true, Ordering::Release);
        self.shared.popup_active.store(false, Ordering::Release);
        self.shared.popup.store(0, Ordering::Release);
        self.shared.view.store(0, Ordering::Release);
        if let Ok(mut gate) = self.shared.gate.lock() {
            gate.visible = false;
            gate.epoch = gate.epoch.saturating_add(1);
        }
        unsafe {
            let _: () = msg_send![self.view, setHidden: YES];
            (*self.view).set_ivar::<usize>("novaInput", 0);
            let _: () = msg_send![self.view, removeFromSuperview];
            let _: () = msg_send![self.view, release];
            if !self.popup.is_null() {
                let _: () = msg_send![self.popup, setDelegate: ptr::null_mut::<Object>()];
                let _: () = msg_send![self.popup, orderOut: ptr::null_mut::<Object>()];
                let _: () = msg_send![self.popup, close];
                let _: () = msg_send![self.popup, release];
                (*self.popup_delegate).set_ivar::<usize>("novaInput", 0);
                let _: () = msg_send![self.popup_delegate, release];
            }
            let _: () = msg_send![self.parent, release];
        }
        self.view = ptr::null_mut();
        self.parent = ptr::null_mut();
        self.popup = ptr::null_mut();
        self.popup_delegate = ptr::null_mut();
        // No join/GPU wait on the main thread. The receiver wakes within 100ms,
        // owns its Mach receive right, and cleans it after its current GPU work.
        Ok(())
    }
}
impl Drop for MacNativeView {
    fn drop(&mut self) {
        let _ = self.dispose();
    }
}

/// # Safety
/// parent_nsview is a live NSView in THIS Host process and this is the main
/// thread. No renderer process receives or dereferences this pointer.
pub unsafe fn create(
    parent_nsview: *mut c_void,
    input_sink: Option<InputSink>,
) -> Result<MacNativeView, String> {
    main_thread()?;
    if parent_nsview.is_null() {
        return Err("native macOS parent NSView is null".into());
    }
    let device = Device::system_default().ok_or("no Metal presentation device")?;
    let layer = MetalLayer::new();
    layer.set_device(&device);
    // Both producers publish final display-encoded colour. Fast's sRGB render
    // attachment already encoded it; Vulkan's existing final pass did likewise.
    // Sample/store those bytes as Unorm here, without a second gamma transform.
    layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    layer.set_presents_with_transaction(false);
    layer.set_display_sync_enabled(true);
    layer.set_maximum_drawable_count(3);
    layer.set_framebuffer_only(true);
    layer.set_opaque(false);
    layer.set_masks_to_bounds(true);
    let _: () = msg_send![layer.as_ptr(), setAllowsNextDrawableTimeout: YES];
    let mut nonce = [0_u8; 16];
    if SecRandomCopyBytes(kSecRandomDefault, nonce.len(), nonce.as_mut_ptr()) != 0 {
        return Err("system presentation nonce generation failed".into());
    }
    let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
    let nonce_hex: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
    let service_name = format!(
        "com.litematica.nova.viewport.{}.{}",
        std::process::id(),
        nonce_hex
    );
    let endpoint = NativeCompositionEndpoint {
        kind: "macos_iosurface_mach_v1".into(),
        service_name: service_name.clone(),
        nonce_hex,
        generation,
    };
    let shared = Arc::new(Shared {
        gate: Mutex::new(FrameGate {
            nonce,
            generation,
            epoch: 0,
            visible: false,
            width: 1,
            height: 1,
            last_frame: 0,
        }),
        input: Mutex::new(None),
        stopped: AtomicBool::new(false),
        view: AtomicUsize::new(0),
        scale: AtomicU64::new(1_f64.to_bits()),
        sequence: AtomicU64::new(1),
        presented_epoch: AtomicU64::new(u64::MAX),
        reported_epoch: AtomicU64::new(u64::MAX),
        request_focus: AtomicBool::new(false),
        popup: AtomicUsize::new(0),
        popup_active: AtomicBool::new(false),
        popup_minimized: AtomicBool::new(false),
        popup_requested_visible: AtomicBool::new(false),
        popup_closed_epoch: AtomicU64::new(u64::MAX),
        sink: input_sink,
    });
    let mut input_context = Box::new(Arc::clone(&shared));
    let view: *mut Object = msg_send![view_class(), alloc];
    let rect = Rect {
        origin: Point { x: 0.0, y: 0.0 },
        size: Size {
            width: 1.0,
            height: 1.0,
        },
    };
    let view: *mut Object = msg_send![view, initWithFrame: rect];
    if view.is_null() {
        return Err("native macOS child NSView allocation failed".into());
    }
    (*view).set_ivar(
        "novaInput",
        (&mut *input_context as *mut Arc<Shared>) as usize,
    );
    let _: () = msg_send![view, setWantsLayer: YES];
    let _: () = msg_send![view, setLayer: layer.as_ptr()];
    let _: () = msg_send![view, setHidden: YES];
    let parent = parent_nsview as *mut Object;
    let _: *mut Object = msg_send![parent, retain];
    let _: () = msg_send![parent, addSubview: view positioned: 1_isize relativeTo: ptr::null_mut::<Object>()];
    shared.view.store(view as usize, Ordering::Release);
    let mut native = MacNativeView {
        view,
        parent,
        popup: ptr::null_mut(),
        popup_delegate: ptr::null_mut(),
        mode: NativePresentationMode::Embedded,
        layer: layer.clone(),
        shared: Arc::clone(&shared),
        _input_context: input_context,
        endpoint,
        placement: None,
        focused: false,
        _main_thread: PhantomData,
    };
    let receiver = match Receiver::new(&service_name) {
        Ok(receiver) => receiver,
        Err(error) => {
            let _ = native.dispose();
            return Err(error);
        }
    };
    if let Err(error) = std::thread::Builder::new()
        .name("nova-macos-present".into())
        .spawn(move || {
            objc::rc::autoreleasepool(|| match Presenter::new(device, layer) {
                Ok(mut presenter) => receiver.run(&shared, &mut presenter),
                Err(error) => {
                    eprintln!("[NOVA_NATIVE_MACOS] stage=presentation_init_failed error={error}");
                    shared.stopped.store(true, Ordering::Release);
                }
            });
        })
    {
        let _ = native.dispose();
        return Err(format!("start macOS presentation receiver: {error}"));
    }
    Ok(native)
}

struct Receiver {
    port: u32,
    bootstrap: u32,
    name: CString,
}
impl Receiver {
    fn new(name: &str) -> Result<Self, String> {
        let name = CString::new(name).map_err(|_| "invalid Mach service name")?;
        let (mut port, mut bootstrap) = (0, 0);
        unsafe {
            let task = mach_task_self_;
            if task_get_special_port(task, TASK_BOOTSTRAP_PORT, &mut bootstrap) != 0 {
                return Err("get Host bootstrap port failed".into());
            }
            if mach_port_allocate(task, 1, &mut port) != 0 {
                mach_port_deallocate(task, bootstrap);
                return Err("allocate Host receive right failed".into());
            }
            if mach_port_insert_right(task, port, port, 20) != 0
                || bootstrap_register(bootstrap, name.as_ptr(), port) != 0
            {
                mach_port_destroy(task, port);
                mach_port_deallocate(task, bootstrap);
                return Err("register Host presentation Mach service failed".into());
            }
        }
        Ok(Self {
            port,
            bootstrap,
            name,
        })
    }

    fn run(self, shared: &Arc<Shared>, presenter: &mut Presenter) {
        while !shared.stopped.load(Ordering::Acquire) {
            let mut message: FrameMessage = unsafe { zeroed() };
            let code = unsafe {
                mach_msg(
                    &mut message.header,
                    MACH_RCV_MSG | MACH_RCV_TIMEOUT,
                    0,
                    size_of::<FrameMessage>() as u32,
                    self.port,
                    100,
                    0,
                )
            };
            if code == MACH_RCV_TIMED_OUT {
                continue;
            }
            if code != 0 {
                eprintln!("[NOVA_NATIVE_MACOS] stage=receive_failed code={code}");
                break;
            }
            let valid = message.header.id == wire::FRAME_MESSAGE_ID
                && message.header.size == FRAME_WIRE_SIZE
                && message.header.bits & MACH_MSGH_BITS_COMPLEX != 0
                && message.count == 2
                && message
                    .ports
                    .iter()
                    .all(|port| port.kind == 0 && port.name != 0);
            if valid {
                if let Ok(frame) = FrameHeader::decode(&message.payload) {
                    let (accepted, resize, hidden) = shared
                        .gate
                        .lock()
                        .map(|mut gate| {
                            // A current producer must be able to learn an OS
                            // resize before its first correctly sized frame.
                            // This handshake does not present/accept that frame.
                            let current = frame.nonce == gate.nonce && frame.generation == gate.generation && frame.epoch == gate.epoch;
                            if current {
                                let input = SendRight(message.ports[1].name);
                                message.ports[1].name = 0;
                                if let Ok(mut destination) = shared.input.lock() { *destination = Some(input); }
                            }
                            let accepted = gate.accepts(frame);
                            if accepted { gate.last_frame = frame.frame_id; }
                            (accepted, current && (frame.width != gate.width || frame.height != gate.height), current && !gate.visible)
                        })
                        .unwrap_or((false, false, false));
                    if resize { forward_input_at_epoch(shared, Some(frame.epoch), 8, 0, 0.0, 0.0, 0.0, 0.0, 0, 0, String::new()); }
                    if hidden && shared.popup_closed_epoch.load(Ordering::Acquire) == frame.epoch {
                        forward_input_at_epoch(shared, Some(frame.epoch), 9, 0, 0.0, 0.0, 0.0, 0.0, 0, 0, String::new());
                    }
                    let status = if accepted {
                        match objc::rc::autoreleasepool(|| {
                            presenter.present(message.ports[0].name, frame, shared)
                        }) {
                            Ok(status) => status,
                            Err(error) => {
                                eprintln!("[NOVA_NATIVE_MACOS] stage=frame_failed generation={} frame={} error={error}", frame.generation, frame.frame_id);
                                AckStatus::Failed
                            }
                        }
                    } else {
                        AckStatus::Discarded
                    };
                    send_ack(message.header.remote, frame, status);
                    message.header.remote = 0;
                }
            }
            // Releases every received descriptor and any unused reply right,
            // including invalid/stale messages. It never reads surface pixels.
            unsafe {
                mach_msg_destroy(&mut message.header);
            }
        }
        if let Ok(mut input) = shared.input.lock() {
            *input = None;
        }
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        unsafe {
            bootstrap_register(self.bootstrap, self.name.as_ptr(), 0);
            mach_port_destroy(mach_task_self_, self.port);
            mach_port_deallocate(mach_task_self_, self.bootstrap);
        }
    }
}

fn send_ack(port: u32, frame: FrameHeader, status: AckStatus) {
    if port == 0 {
        return;
    }
    let mut message = DataMessage {
        header: MachHeader {
            bits: u32::from(MOVE_SEND_ONCE),
            size: (24 + wire::ACK_BYTES) as u32,
            remote: port,
            local: 0,
            voucher: 0,
            id: wire::ACK_MESSAGE_ID,
        },
        payload: wire::encode_ack(frame, status),
    };
    let size = message.header.size;
    if unsafe {
        mach_msg(
            &mut message.header,
            MACH_SEND_MSG | MACH_SEND_TIMEOUT,
            size,
            0,
            0,
            0,
            0,
        )
    } != 0
    {
        unsafe {
            mach_port_deallocate(mach_task_self_, port);
        }
    }
}

struct Surface(*mut c_void);
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.0);
        }
    }
}
struct Presenter {
    device: Device,
    layer: MetalLayer,
    queue: CommandQueue,
    pipeline: RenderPipelineState,
}
impl Presenter {
    fn new(device: Device, layer: MetalLayer) -> Result<Self, String> {
        let library =
            device.new_library_with_source(PRESENT_SHADER, &metal::CompileOptions::new())?;
        let vertex = library.get_function("nova_present_vertex", None)?;
        let fragment = library.get_function("nova_present_fragment", None)?;
        let descriptor = RenderPipelineDescriptor::new();
        descriptor.set_vertex_function(Some(&vertex));
        descriptor.set_fragment_function(Some(&fragment));
        descriptor
            .color_attachments()
            .object_at(0)
            .ok_or("missing presentation attachment")?
            .set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        let pipeline = device.new_render_pipeline_state(&descriptor)?;
        let queue = device.new_command_queue_with_max_command_buffer_count(3);
        Ok(Self {
            device,
            layer,
            queue,
            pipeline,
        })
    }

    fn present(
        &mut self,
        surface_port: u32,
        frame: FrameHeader,
        shared: &Arc<Shared>,
    ) -> Result<AckStatus, String> {
        let raw_surface = unsafe { IOSurfaceLookupFromMachPort(surface_port) };
        if raw_surface.is_null() {
            return Err("IOSurface Mach lookup failed".into());
        }
        let surface = Surface(raw_surface);
        unsafe {
            let stride = IOSurfaceGetBytesPerRow(surface.0);
            if IOSurfaceGetWidth(surface.0) != frame.width as usize
                || IOSurfaceGetHeight(surface.0) != frame.height as usize
                || IOSurfaceGetPlaneCount(surface.0) != 0
                || IOSurfaceGetPixelFormat(surface.0) != u32::from_be_bytes(*b"BGRA")
                || stride < frame.width as usize * 4
                || IOSurfaceGetAllocSize(surface.0) as u64 > wire::MAX_SURFACE_BYTES
                || (stride as u64).saturating_mul(u64::from(frame.height)) > wire::MAX_SURFACE_BYTES
            {
                return Err("shared surface layout/byte budget mismatch".into());
            }
        }
        let descriptor = TextureDescriptor::new();
        descriptor.set_texture_type(MTLTextureType::D2);
        descriptor.set_width(u64::from(frame.width));
        descriptor.set_height(u64::from(frame.height));
        descriptor.set_mipmap_level_count(1);
        descriptor.set_usage(MTLTextureUsage::ShaderRead);
        descriptor.set_storage_mode(MTLStorageMode::Shared);
        // Wire format describes the producer attachment, not a request to
        // decode the finished display image again in this presentation pass.
        descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        let raw_texture: *mut metal::MTLTexture = unsafe {
            msg_send![self.device.as_ptr(), newTextureWithDescriptor: descriptor.as_ptr() iosurface: surface.0 plane: 0_u64]
        };
        if raw_texture.is_null() {
            return Err("Metal cannot import this IOSurface layout".into());
        }
        let texture = unsafe { Texture::from_ptr(raw_texture) };
        if !current_frame(shared, frame) {
            return Ok(AckStatus::Discarded);
        }
        let Some(drawable) = self.layer.next_drawable() else {
            return Ok(AckStatus::Discarded);
        };
        if frame.flags & wire::REAL_GEOMETRY != 0 {
            let observed = Arc::clone(shared);
            let handler = block::ConcreteBlock::new(move |displayed: *mut Object| {
                if displayed.is_null() {
                    return;
                }
                let time: f64 = unsafe { msg_send![displayed, presentedTime] };
                if time.is_finite() && time > 0.0 {
                    schedule_display_receipt(Arc::clone(&observed), frame, time);
                }
            })
            .copy();
            unsafe {
                let _: () = msg_send![drawable.as_ptr(), addPresentedHandler: &*handler];
            }
        }
        let pass = RenderPassDescriptor::new();
        let colour = pass
            .color_attachments()
            .object_at(0)
            .ok_or("missing native layer colour attachment")?;
        colour.set_texture(Some(drawable.texture()));
        colour.set_load_action(MTLLoadAction::Clear);
        colour.set_store_action(MTLStoreAction::Store);
        colour.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
        let command = self.queue.new_command_buffer();
        let encoder = command.new_render_command_encoder(pass);
        encoder.set_render_pipeline_state(&self.pipeline);
        encoder.set_fragment_texture(0, Some(&texture));
        encoder.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
        encoder.end_encoding();
        if !current_frame(shared, frame) {
            return Ok(AckStatus::Discarded);
        }
        command.present_drawable(drawable);
        command.commit();
        command.wait_until_completed();
        // Waiting happens only on this bounded presentation worker. The ACK is
        // the producer's permission to reuse a ring slot after BOTH GPU owners.
        if command.status() != MTLCommandBufferStatus::Completed {
            return Err("Metal presentation command failed".into());
        }
        if !current_frame(shared, frame) {
            return Ok(AckStatus::Discarded);
        }
        shared.presented_epoch.store(frame.epoch, Ordering::Release);
        schedule_visible(Arc::clone(shared), frame.epoch);
        Ok(AckStatus::GpuCopyComplete)
    }
}

fn current_frame(shared: &Shared, frame: FrameHeader) -> bool {
    !shared.stopped.load(Ordering::Acquire)
        && shared
            .gate
            .lock()
            .map(|gate| {
                gate.visible
                    && gate.generation == frame.generation
                    && gate.epoch == frame.epoch
                    && gate.width == frame.width
                    && gate.height == frame.height
            })
            .unwrap_or(false)
}

fn schedule_visible(shared: Arc<Shared>, epoch: u64) {
    extern "C" fn on_main(context: *mut c_void) {
        let (shared, epoch) = unsafe { *Box::from_raw(context as *mut (Arc<Shared>, u64)) };
        if shared.stopped.load(Ordering::Acquire) {
            return;
        }
        let visible = shared
            .gate
            .lock()
            .map(|gate| gate.visible && gate.epoch == epoch)
            .unwrap_or(false);
        let view = shared.view.load(Ordering::Acquire) as *mut Object;
        if visible && !view.is_null() {
            unsafe {
                let _: () = msg_send![view, setHidden: NO];
                if shared.request_focus.swap(false, Ordering::AcqRel) {
                    let window: *mut Object = msg_send![view, window];
                    let _: BOOL = msg_send![window, makeFirstResponder: view];
                    let _: () = msg_send![window, setAcceptsMouseMovedEvents: YES];
                }
            }
        }
    }
    let context = Box::into_raw(Box::new((shared, epoch))) as *mut c_void;
    unsafe {
        dispatch_async_f(ptr::addr_of!(_dispatch_main_q).cast(), context, on_main);
    }
}

fn schedule_display_receipt(shared: Arc<Shared>, frame: FrameHeader, presented_time: f64) {
    extern "C" fn on_main(context: *mut c_void) {
        let (shared, frame, time) =
            unsafe { *Box::from_raw(context as *mut (Arc<Shared>, FrameHeader, f64)) };
        if !current_frame(&shared, frame) {
            return;
        }
        let view = shared.view.load(Ordering::Acquire) as *mut Object;
        if view.is_null() {
            return;
        }
        unsafe {
            let hidden: BOOL = msg_send![view, isHiddenOrHasHiddenAncestor];
            let window: *mut Object = msg_send![view, window];
            if hidden == YES || window.is_null() {
                return;
            }
            let visible: BOOL = msg_send![window, isVisible];
            let minimized: BOOL = msg_send![window, isMiniaturized];
            let occlusion: u64 = msg_send![window, occlusionState];
            if visible != YES || minimized == YES || occlusion & 2 == 0 {
                return;
            }
        }
        if shared.reported_epoch.swap(frame.epoch, Ordering::AcqRel) == frame.epoch {
            return;
        }
        if let Ok(destination) = shared.input.lock() {
            if let Some(port) = destination.as_ref() {
                let mut message = DataMessage {
                    header: MachHeader {
                        bits: u32::from(COPY_SEND),
                        size: (24 + wire::DISPLAY_BYTES) as u32,
                        remote: port.0,
                        local: 0,
                        voucher: 0,
                        id: wire::DISPLAY_MESSAGE_ID,
                    },
                    payload: wire::encode_display(frame, time),
                };
                let size = message.header.size;
                let result = unsafe {
                    mach_msg(
                        &mut message.header,
                        MACH_SEND_MSG | MACH_SEND_TIMEOUT,
                        size,
                        0,
                        0,
                        0,
                        0,
                    )
                };
                eprintln!("[NOVA_NATIVE_MACOS] stage=drawable_presented generation={} epoch={} frame={} real_geometry=true presented_time_s={time:.9} receipt_send_code={result}", frame.generation, frame.epoch, frame.frame_id);
            }
        };
    }
    let context = Box::into_raw(Box::new((shared, frame, presented_time))) as *mut c_void;
    unsafe {
        dispatch_async_f(ptr::addr_of!(_dispatch_main_q).cast(), context, on_main);
    }
}

const PRESENT_SHADER: &str = r#"
#include <metal_stdlib>
using namespace metal;
struct NovaPresentationVertex { float4 position [[position]]; float2 uv; };
vertex NovaPresentationVertex nova_present_vertex(uint index [[vertex_id]]) {
    const float2 p[3] = {float2(-1,-1), float2(3,-1), float2(-1,3)};
    NovaPresentationVertex value;
    value.position = float4(p[index], 0, 1);
    value.uv = float2((p[index].x + 1) * 0.5, 1 - (p[index].y + 1) * 0.5);
    return value;
}
fragment float4 nova_present_fragment(NovaPresentationVertex value [[stage_in]], texture2d<float> image [[texture(0)]]) {
    constexpr sampler colour(coord::normalized, address::clamp_to_edge, filter::nearest);
    return image.sample(colour, value.uv);
}
"#;

fn forward_input(
    shared: &Shared,
    kind: u32,
    flags: u32,
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
    code: u32,
    modifiers: u32,
    text: String,
) {
    forward_input_at_epoch(shared, None, kind, flags, x, y, dx, dy, code, modifiers, text);
}

fn forward_input_at_epoch(
    shared: &Shared, expected_epoch: Option<u64>, kind: u32, flags: u32,
    mut x: f64, mut y: f64, mut dx: f64, dy: f64,
    code: u32, modifiers: u32, text: String,
) {
    if shared.stopped.load(Ordering::Acquire) {
        return;
    }
    let Ok(gate) = shared.gate.lock() else {
        return;
    };
    if expected_epoch.is_some_and(|epoch| epoch != gate.epoch) { return; }
    if !gate.visible && !matches!(kind, 7 | 8 | 9) {
        return;
    }
    if kind == 8 {
        x = f64::from(gate.width);
        y = f64::from(gate.height);
        dx = f64::from_bits(shared.scale.load(Ordering::Acquire));
    }
    let sequence = shared.sequence.fetch_add(1, Ordering::Relaxed);
    let packet = InputPacket {
        nonce: gate.nonce,
        generation: gate.generation,
        epoch: gate.epoch,
        sequence,
        kind,
        flags,
        x,
        y,
        dx,
        dy,
        code,
        modifiers,
        text: &text,
    };
    let payload = packet.encode();
    let generation = gate.generation;
    let epoch = gate.epoch;
    drop(gate);
    if let Ok(destination) = shared.input.lock() {
        if let Some(port) = destination.as_ref() {
            let mut message = DataMessage {
                header: MachHeader {
                    bits: u32::from(COPY_SEND),
                    size: (24 + wire::INPUT_BYTES) as u32,
                    remote: port.0,
                    local: 0,
                    voucher: 0,
                    id: wire::INPUT_MESSAGE_ID,
                },
                payload,
            };
            // Never wait for rendering or network on an AppKit input callback.
            let size = message.header.size;
            let result = unsafe {
                mach_msg(
                    &mut message.header,
                    MACH_SEND_MSG | MACH_SEND_TIMEOUT,
                    size,
                    0,
                    0,
                    0,
                    0,
                )
            };
            if result != 0 {
                eprintln!("[NOVA_NATIVE_MACOS] stage=input_not_delivered code={result} kind={kind} epoch={epoch}");
            }
        }
    }
    if let Some(sink) = shared.sink.as_ref() {
        sink(NativeInput {
            generation,
            epoch,
            sequence,
            kind,
            flags,
            x,
            y,
            dx,
            dy,
            code,
            modifiers,
            text,
        });
    }
}

unsafe fn popup_placement(window: *mut Object) -> Result<PresentationPlacement, String> {
    let content: *mut Object = msg_send![window, contentView];
    if content.is_null() { return Err("native popup content view is missing".into()); }
    let bounds: Rect = msg_send![content, bounds];
    let scale: f64 = msg_send![window, backingScaleFactor];
    if !scale.is_finite() || scale <= 0.0 || !bounds.size.width.is_finite() || !bounds.size.height.is_finite() {
        return Err("invalid native popup backing bounds".into());
    }
    let width = (bounds.size.width * scale).round();
    let height = (bounds.size.height * scale).round();
    if width < 1.0 || height < 1.0 || width > 16_384.0 || height > 16_384.0 || width * height * 4.0 > wire::MAX_SURFACE_BYTES as f64 {
        return Err("native popup size exceeds presentation surface budget".into());
    }
    Ok(PresentationPlacement { x: 0.0, y: 0.0, width: width as u32, height: height as u32, scale })
}

fn send_viewport_size(shared: &Shared) {
    forward_input(shared, 8, 0, 0.0, 0.0, 0.0, 0.0, 0, 0, String::new());
}

/// AppKit callbacks call this only on the main thread. It never changes mode,
/// owns renderer resources, or waits for a renderer/GPU acknowledgement.
fn close_popup(shared: &Shared) {
    if !shared.popup_active.load(Ordering::Acquire) || shared.stopped.load(Ordering::Acquire) { return; }
    let Ok(mut gate) = shared.gate.lock() else { return; };
    shared.popup_closed_epoch.store(gate.epoch, Ordering::Release);
    shared.popup_requested_visible.store(false, Ordering::Release);
    gate.visible = false;
    drop(gate);
    shared.request_focus.store(false, Ordering::Release);
    unsafe {
        let view = shared.view.load(Ordering::Acquire) as *mut Object;
        if !view.is_null() { let _: () = msg_send![view, setHidden: YES]; }
        let popup = shared.popup.load(Ordering::Acquire) as *mut Object;
        if !popup.is_null() { let _: () = msg_send![popup, orderOut: ptr::null_mut::<Object>()]; }
    }
    forward_input(shared, 7, 0, 0.0, 0.0, 0.0, 0.0, 0, 0, String::new());
    forward_input(shared, 9, 0, 0.0, 0.0, 0.0, 0.0, 0, 0, String::new());
}

fn resize_popup(shared: &Shared) {
    if !shared.popup_active.load(Ordering::Acquire) || shared.stopped.load(Ordering::Acquire) { return; }
    let popup = shared.popup.load(Ordering::Acquire) as *mut Object;
    let view = shared.view.load(Ordering::Acquire) as *mut Object;
    if popup.is_null() || view.is_null() { return; }
    let Ok(placement) = (unsafe { popup_placement(popup) }) else { return; };
    let Ok(mut gate) = shared.gate.lock() else { return; };
    let changed = gate.width != placement.width || gate.height != placement.height || f64::from_bits(shared.scale.load(Ordering::Acquire)) != placement.scale;
    gate.width = placement.width;
    gate.height = placement.height;
    shared.scale.store(placement.scale.to_bits(), Ordering::Release);
    drop(gate);
    if changed {
        shared.presented_epoch.store(u64::MAX, Ordering::Release);
        shared.reported_epoch.store(u64::MAX, Ordering::Release);
        unsafe {
            let _: () = msg_send![view, setHidden: YES];
            let rect = Rect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: f64::from(placement.width) / placement.scale, height: f64::from(placement.height) / placement.scale } };
            let _: () = msg_send![view, setFrame: rect];
            let layer: *mut Object = msg_send![view, layer];
            let _: () = msg_send![layer, setContentsScale: placement.scale];
            let _: () = msg_send![layer, setDrawableSize: CGSize::new(f64::from(placement.width), f64::from(placement.height))];
            let _: () = msg_send![layer, removeAllAnimations];
        }
    }
    send_viewport_size(shared);
}

fn popup_delegate_class() -> &'static Class {
    static CLASS: OnceLock<&'static Class> = OnceLock::new();
    CLASS.get_or_init(|| unsafe {
        extern "C" fn should_close(this: &Object, _: Sel, _: *mut Object) -> BOOL {
            let context = unsafe { *this.get_ivar::<usize>("novaInput") } as *const Arc<Shared>;
            if !context.is_null() { close_popup(unsafe { &*context }); }
            NO // Keep the Host window/view and the independent Viewer alive.
        }
        extern "C" fn resized(this: &Object, _: Sel, _: *mut Object) {
            let context = unsafe { *this.get_ivar::<usize>("novaInput") } as *const Arc<Shared>;
            if !context.is_null() { resize_popup(unsafe { &*context }); }
        }
        extern "C" fn focus(this: &Object, selector: Sel, _: *mut Object) {
            let context = unsafe { *this.get_ivar::<usize>("novaInput") } as *const Arc<Shared>;
            if context.is_null() { return; }
            let shared = unsafe { &*context };
            if shared.popup_active.load(Ordering::Acquire) {
                forward_input(shared, 7, u32::from(selector == sel!(windowDidBecomeKey:)), 0.0, 0.0, 0.0, 0.0, 0, 0, String::new());
            }
        }
        extern "C" fn minimized(this: &Object, selector: Sel, _: *mut Object) {
            let context = unsafe { *this.get_ivar::<usize>("novaInput") } as *const Arc<Shared>;
            if context.is_null() { return; }
            let shared = unsafe { &*context };
            if !shared.popup_active.load(Ordering::Acquire) { return; }
            let minimized = selector == sel!(windowDidMiniaturize:);
            shared.popup_minimized.store(minimized, Ordering::Release);
            let Ok(mut gate) = shared.gate.lock() else { return; };
            gate.visible = !minimized && shared.popup_requested_visible.load(Ordering::Acquire)
                && shared.popup_closed_epoch.load(Ordering::Acquire) != gate.epoch;
            drop(gate);
            if minimized {
                let view = shared.view.load(Ordering::Acquire) as *mut Object;
                if !view.is_null() { unsafe { let _: () = msg_send![view, setHidden: YES]; } }
                forward_input(shared, 7, 0, 0.0, 0.0, 0.0, 0.0, 0, 0, String::new());
            } else { resize_popup(shared); }
        }
        let mut declaration = ClassDecl::new("NovaIOSurfacePopupDelegate", class!(NSObject)).expect("unique Nova popup delegate class");
        declaration.add_ivar::<usize>("novaInput");
        declaration.add_method(sel!(windowShouldClose:), should_close as extern "C" fn(&Object, Sel, *mut Object) -> BOOL);
        declaration.add_method(sel!(windowDidResize:), resized as extern "C" fn(&Object, Sel, *mut Object));
        declaration.add_method(sel!(windowDidChangeBackingProperties:), resized as extern "C" fn(&Object, Sel, *mut Object));
        declaration.add_method(sel!(windowDidBecomeKey:), focus as extern "C" fn(&Object, Sel, *mut Object));
        declaration.add_method(sel!(windowDidResignKey:), focus as extern "C" fn(&Object, Sel, *mut Object));
        declaration.add_method(sel!(windowDidMiniaturize:), minimized as extern "C" fn(&Object, Sel, *mut Object));
        declaration.add_method(sel!(windowDidDeminiaturize:), minimized as extern "C" fn(&Object, Sel, *mut Object));
        declaration.register()
    })
}

fn view_class() -> &'static Class {
    static CLASS: OnceLock<&'static Class> = OnceLock::new();
    CLASS.get_or_init(|| unsafe {
        extern "C" fn accepts(_: &Object, _: Sel) -> BOOL {
            YES
        }
        extern "C" fn focus(this: &Object, selector: Sel) -> BOOL {
            let context = unsafe { *this.get_ivar::<usize>("novaInput") } as *const Arc<Shared>;
            if !context.is_null() {
                let focused = selector == sel!(becomeFirstResponder);
                forward_input(
                    unsafe { &*context },
                    7,
                    u32::from(focused),
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0,
                    0,
                    String::new(),
                );
            }
            YES
        }
        extern "C" fn event(this: &Object, selector: Sel, event: *mut Object) {
            if event.is_null() {
                return;
            }
            let context = unsafe { *this.get_ivar::<usize>("novaInput") } as *const Arc<Shared>;
            if context.is_null() {
                return;
            }
            let shared = unsafe { &*context };
            let scale = f64::from_bits(shared.scale.load(Ordering::Acquire));
            let kind = if selector == sel!(scrollWheel:) {
                4
            } else if selector == sel!(keyDown:) {
                5
            } else if selector == sel!(keyUp:) {
                6
            } else if [
                sel!(mouseDown:),
                sel!(rightMouseDown:),
                sel!(otherMouseDown:),
            ]
            .contains(&selector)
            {
                2
            } else if [sel!(mouseUp:), sel!(rightMouseUp:), sel!(otherMouseUp:)].contains(&selector)
            {
                3
            } else {
                1
            };
            unsafe {
                if kind == 2 {
                    let window: *mut Object = msg_send![this, window];
                    let _: BOOL = msg_send![window, makeFirstResponder: this];
                    let _: () = msg_send![window, setAcceptsMouseMovedEvents: YES];
                }
                let modifiers: u64 = msg_send![event, modifierFlags];
                let point: Point = msg_send![event, locationInWindow];
                let point: Point =
                    msg_send![this, convertPoint: point fromView: ptr::null_mut::<Object>()];
                let (mut flags, mut code, mut dx, mut dy, mut text) =
                    (0, 0, 0.0, 0.0, String::new());
                if kind == 4 {
                    dx = msg_send![event, scrollingDeltaX];
                    dy = msg_send![event, scrollingDeltaY];
                    let precise: BOOL = msg_send![event, hasPreciseScrollingDeltas];
                    if precise == YES {
                        flags |= 2;
                        dx *= scale;
                        dy *= scale;
                    }
                } else if kind == 5 || kind == 6 {
                    let key: u16 = msg_send![event, keyCode];
                    code = u32::from(key);
                    if kind == 5 && code == 53 && shared.popup_active.load(Ordering::Acquire) {
                        close_popup(shared);
                        return;
                    }
                    let repeat: BOOL = msg_send![event, isARepeat];
                    if repeat == YES {
                        flags |= 1;
                    }
                    let chars: *mut Object = msg_send![event, charactersIgnoringModifiers];
                    if !chars.is_null() {
                        let value: *const c_char = msg_send![chars, UTF8String];
                        if !value.is_null() {
                            text = CStr::from_ptr(value).to_string_lossy().into_owned();
                        }
                    }
                } else if kind == 2 || kind == 3 {
                    let button: i64 = msg_send![event, buttonNumber];
                    code = button as u32;
                }
                forward_input(
                    shared,
                    kind,
                    flags,
                    point.x * scale,
                    point.y * scale,
                    dx,
                    dy,
                    code,
                    modifiers as u32,
                    text,
                );
            }
        }
        let superclass = Class::get("NSView").expect("AppKit NSView class");
        let mut declaration = ClassDecl::new("NovaIOSurfacePresentationView", superclass)
            .expect("unique Nova NSView class");
        declaration.add_ivar::<usize>("novaInput");
        declaration.add_method(
            sel!(acceptsFirstResponder),
            accepts as extern "C" fn(&Object, Sel) -> BOOL,
        );
        declaration.add_method(
            sel!(isFlipped),
            accepts as extern "C" fn(&Object, Sel) -> BOOL,
        );
        declaration.add_method(
            sel!(acceptsFirstMouse:),
            accepts_mouse as extern "C" fn(&Object, Sel, *mut Object) -> BOOL,
        );
        declaration.add_method(
            sel!(becomeFirstResponder),
            focus as extern "C" fn(&Object, Sel) -> BOOL,
        );
        declaration.add_method(
            sel!(resignFirstResponder),
            focus as extern "C" fn(&Object, Sel) -> BOOL,
        );
        for selector in [
            sel!(mouseMoved:),
            sel!(mouseDragged:),
            sel!(rightMouseDragged:),
            sel!(otherMouseDragged:),
            sel!(mouseDown:),
            sel!(rightMouseDown:),
            sel!(otherMouseDown:),
            sel!(mouseUp:),
            sel!(rightMouseUp:),
            sel!(otherMouseUp:),
            sel!(scrollWheel:),
            sel!(keyDown:),
            sel!(keyUp:),
        ] {
            declaration.add_method(selector, event as extern "C" fn(&Object, Sel, *mut Object));
        }
        declaration.register()
    })
}
extern "C" fn accepts_mouse(_: &Object, _: Sel, _: *mut Object) -> BOOL {
    YES
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mach_wire_layout_is_identical_on_both_64bit_macos_targets() {
        assert_eq!(size_of::<MachHeader>(), 24);
        assert_eq!(size_of::<PortDescriptor>(), 12);
        assert_eq!(FRAME_WIRE_SIZE, 116);
        assert_eq!(size_of::<DataMessage<{ wire::ACK_BYTES }>>(), 56);
        assert_eq!(size_of::<DataMessage<{ wire::INPUT_BYTES }>>(), 152);
    }
}
