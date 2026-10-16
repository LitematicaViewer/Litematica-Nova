//! Cross-process final-colour presentation; no scene or lighting implementation.
//! The caller supplies its ACTUAL selected MTLDevice. No default-device lookup,
//! window creation, CPU pixel access, or extra rendering pipeline lives here.
#![cfg(target_os = "macos")]

use foreign_types::{ForeignType, ForeignTypeRef};
use metal::{CommandBufferRef, DeviceRef, MTLCommandBufferStatus, MTLPixelFormat,
    MTLStorageMode, MTLTextureType, MTLTextureUsage, Texture, TextureDescriptor, TextureRef};
use objc::{msg_send, sel, sel_impl};
use std::{collections::{HashSet, VecDeque}, ffi::{c_char, c_void, CString},
    mem::{size_of, zeroed}, ptr,
    sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};
use viewer_launch_contract::{NativeCompositionEndpoint, native_macos_wire as wire};

// TASK_BOOTSTRAP_PORT is a selector, not a Mach port name.
const TASK_BOOTSTRAP_PORT: i32 = 4;
const RING_SLOTS: usize = 3;
const EVENT_LIMIT: usize = 256;
const INPUT_QUEUE_LIMIT: u32 = 128;
const ACK_TIMEOUT: Duration = Duration::from_secs(10);
const SEND: u32 = 1;
const RECEIVE: u32 = 2;
const SEND_TIMEOUT: u32 = 0x10;
const RECEIVE_TIMEOUT: u32 = 0x100;
const RECEIVE_TIMED_OUT: i32 = 0x1000_4003;
const COPY_SEND: u8 = 19;
const MAKE_SEND_ONCE: u32 = 21;
const COMPLEX: u32 = 0x8000_0000;

#[repr(C)]
#[derive(Clone, Copy)]
struct Header { bits: u32, size: u32, remote: u32, local: u32, voucher: u32, id: i32 }
#[repr(C)]
#[derive(Clone, Copy)]
struct PortDescriptor { name: u32, pad1: u32, pad2: u16, disposition: u8, kind: u8 }
#[repr(C)]
struct FrameMessage { header: Header, count: u32, ports: [PortDescriptor; 2], payload: [u8; wire::FRAME_BYTES] }
#[repr(C)]
struct EventMessage { header: Header, payload: [u8; wire::INPUT_BYTES], trailer: [u8; 512] }
#[repr(C)]
struct PortLimits { queue_limit: u32 }

#[link(name = "System")]
unsafe extern "C" {
    static mach_task_self_: u32;
    fn task_get_special_port(task: u32, which_port: i32, port: *mut u32) -> i32;
    fn bootstrap_look_up(bootstrap: u32, name: *const c_char, service: *mut u32) -> i32;
    fn mach_port_allocate(task: u32, right: i32, port: *mut u32) -> i32;
    fn mach_port_insert_right(task: u32, name: u32, right: u32, disposition: u32) -> i32;
    fn mach_port_set_attributes(task: u32, name: u32, flavor: i32, info: *const PortLimits, count: u32) -> i32;
    fn mach_port_deallocate(task: u32, port: u32) -> i32;
    fn mach_port_destroy(task: u32, port: u32) -> i32;
    fn mach_msg(message: *mut Header, options: u32, send_size: u32, receive_size: u32,
        receive_name: u32, timeout: u32, notify: u32) -> i32;
    fn mach_msg_destroy(message: *mut Header);
}
#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    static kIOSurfaceWidth: *const c_void;
    static kIOSurfaceHeight: *const c_void;
    static kIOSurfaceBytesPerElement: *const c_void;
    static kIOSurfaceBytesPerRow: *const c_void;
    static kIOSurfaceAllocSize: *const c_void;
    static kIOSurfacePixelFormat: *const c_void;
    fn IOSurfaceAlignProperty(property: *const c_void, value: usize) -> usize;
    fn IOSurfaceCreate(properties: *const c_void) -> *mut c_void;
    fn IOSurfaceCreateMachPort(surface: *const c_void) -> u32;
    fn IOSurfaceGetAllocSize(surface: *const c_void) -> usize;
    fn IOSurfaceGetBytesPerRow(surface: *const c_void) -> usize;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    fn CFNumberCreate(allocator: *const c_void, kind: isize, value: *const c_void) -> *mut c_void;
    fn CFDictionaryCreate(allocator: *const c_void, keys: *const *const c_void, values: *const *const c_void,
        count: isize, key_callbacks: *const c_void, value_callbacks: *const c_void) -> *mut c_void;
    fn CFRelease(value: *const c_void);
}

struct SendRight(u32);
impl Drop for SendRight { fn drop(&mut self) { unsafe { mach_port_deallocate(mach_task_self_, self.0); } } }
struct ReceivePort(u32);
impl ReceivePort {
    fn new(send_right: bool, limit: u32) -> Result<Self, String> {
        let mut port = 0;
        unsafe {
            if mach_port_allocate(mach_task_self_, 1, &mut port) != 0 { return Err("allocate producer receive port failed".into()); }
            let owned = Self(port);
            if send_right && mach_port_insert_right(mach_task_self_, port, port, 20) != 0 { return Err("create producer input send right failed".into()); }
            let limits = PortLimits { queue_limit: limit };
            if mach_port_set_attributes(mach_task_self_, port, 1, &limits, 1) != 0 { return Err("bound producer receive queue failed".into()); }
            Ok(owned)
        }
    }
}
impl Drop for ReceivePort { fn drop(&mut self) { unsafe { mach_port_destroy(mach_task_self_, self.0); } } }
struct CfOwned(*mut c_void);
impl Drop for CfOwned { fn drop(&mut self) { unsafe { CFRelease(self.0); } } }

/// An IOSurface and its texture are reference-counted Apple GPU resources.
/// Immutable handle access is thread-safe; slot ownership below serializes all
/// writes. Both are retained through GPU completion and the Host's ACK.
struct SurfaceTexture { surface: CfOwned, texture: Texture, bytes: u64, bytes_per_row: usize, device_id: u64 }
unsafe impl Send for SurfaceTexture {}
unsafe impl Sync for SurfaceTexture {}

#[derive(Clone, Copy, Debug)]
pub struct RingLayout {
    pub width: u32, pub height: u32, pub extent: [u32; 2], pub format: MTLPixelFormat,
    /// Changes only when a different allocation replaces the ring.
    pub generation: u64,
    pub bytes_per_row: usize, pub total_bytes: u64, pub slot_count: usize,
}
struct Ring { slots: [Arc<SurfaceTexture>; RING_SLOTS], layout: RingLayout }

#[derive(Clone, Debug)]
pub struct ProducerInput {
    pub generation: u64, pub epoch: u64, pub sequence: u64,
    pub kind: u32, pub flags: u32, pub x: f64, pub y: f64,
    pub dx: f64, pub dy: f64, pub code: u32, pub modifiers: u32, pub text: String,
}
#[derive(Clone, Debug)]
pub enum ProducerEvent {
    Input(ProducerInput),
    /// The existing camera/input adapter must clear all held keys/buttons.
    ResetInput { epoch: u64 },
    /// Host NSWindow backing extent; this changes only presentation targets.
    ViewportResized { epoch: u64, extent: [u32; 2], scale: f64 },
    /// An explicit user close/Escape hides the Host view, not the Viewer.
    Hidden { epoch: u64 },
    /// This is the separate Host MTLDrawable onscreen receipt, never an ACK.
    Displayed { epoch: u64, frame_id: u64, presented_time_seconds: f64, received_at: Instant },
    FrameRejected { epoch: u64, frame_id: u64 },
    Failed(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishOutcome { Sent { frame_id: u64 }, Discarded }
#[derive(Clone, Copy, Debug)]
enum SlotState { Free, Rendering { epoch: u64, frame_id: u64 }, InFlight { epoch: u64, frame_id: u64, sent_at: Instant }, Quarantined }

// A timed-out GPU writer is not evidence of completion. Retain at most one
// failed ring per actual device until its owner proves device-idle. A new ring
// on that device is refused meanwhile; an ordinary renderer-process exit lets
// the OS reclaim these references without touching any external process/data.
static QUARANTINED: OnceLock<Mutex<Vec<Arc<SurfaceTexture>>>> = OnceLock::new();

struct State {
    epoch: u64, visible: bool, next_frame: u64, slots: [SlotState; RING_SLOTS],
    last_input_sequence: u64, pressed_keys: HashSet<u32>, pressed_buttons: HashSet<u32>,
    events: VecDeque<ProducerEvent>, failed: bool,
}
impl State {
    fn push(&mut self, event: ProducerEvent) {
        if let ProducerEvent::ViewportResized { epoch, .. } = &event {
            if let Some(ProducerEvent::ViewportResized { epoch: previous, .. }) = self.events.back() {
                if epoch == previous { self.events.pop_back(); self.events.push_back(event); return; }
            }
        }
        if let ProducerEvent::Input(next) = &event {
            if let Some(ProducerEvent::Input(previous)) = self.events.back_mut() {
                if next.epoch == previous.epoch && next.kind == 1 && previous.kind == 1 {
                    *previous = next.clone(); return;
                }
                if next.epoch == previous.epoch && next.kind == 4 && previous.kind == 4 && next.flags == previous.flags && next.modifiers == previous.modifiers {
                    previous.dx = (previous.dx + next.dx).clamp(-1e7, 1e7);
                    previous.dy = (previous.dy + next.dy).clamp(-1e7, 1e7);
                    previous.sequence = next.sequence; return;
                }
            }
        }
        if self.events.len() >= EVENT_LIMIT {
            if let Some(index) = self.events.iter().position(|item| matches!(item, ProducerEvent::Input(input) if input.kind == 1 || input.kind == 4)) {
                self.events.remove(index);
            } else {
                self.events.clear(); self.pressed_keys.clear(); self.pressed_buttons.clear();
                self.events.push_back(ProducerEvent::ResetInput { epoch: self.epoch });
            }
        }
        self.events.push_back(event);
    }
    fn fail(&mut self, message: String) {
        if self.failed { return; }
        self.failed = true; self.visible = false;
        self.pressed_keys.clear(); self.pressed_buttons.clear();
        self.push(ProducerEvent::ResetInput { epoch: self.epoch });
        self.push(ProducerEvent::Failed(message));
    }
    fn free_rendering(&mut self, index: usize, frame_id: u64) {
        if matches!(self.slots[index], SlotState::Rendering { frame_id: current, .. } if current == frame_id) { self.slots[index] = SlotState::Free; }
    }
}
struct Transport {
    destination: SendRight, ack: ReceivePort, input: ReceivePort, nonce: [u8; 16], generation: u64,
    state: Mutex<State>, stopped: AtomicBool,
}
impl Transport {
    fn fail(&self, message: String) { if let Ok(mut state) = self.state.lock() { state.fail(message); } }
}

/// Owned only by the existing Viewer/renderer. It creates no NSWindow or view.
pub struct MacPresentationProducer { transport: Arc<Transport>, ring: Option<Arc<Ring>>, next_ring_generation: u64 }
impl MacPresentationProducer {
    pub fn connect(endpoint: impl std::borrow::Borrow<NativeCompositionEndpoint>) -> Result<Self, String> {
        let endpoint = endpoint.borrow();
        if endpoint.kind != "macos_iosurface_mach_v1" || endpoint.generation == 0
            || !endpoint.service_name.starts_with("com.litematica.nova.viewport.") || endpoint.service_name.len() > 127
            || !endpoint.service_name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)) {
            return Err("invalid native macOS presentation endpoint".into());
        }
        let nonce = parse_nonce(&endpoint.nonce_hex)?;
        let name = CString::new(endpoint.service_name.as_str()).map_err(|_| "invalid Mach service name")?;
        let mut bootstrap = 0; let mut destination = 0;
        unsafe {
            if task_get_special_port(mach_task_self_, TASK_BOOTSTRAP_PORT, &mut bootstrap) != 0 { return Err("read renderer bootstrap port failed".into()); }
            let bootstrap = SendRight(bootstrap);
            if bootstrap_look_up(bootstrap.0, name.as_ptr(), &mut destination) != 0 || destination == 0 {
                return Err("connect Host presentation Mach service failed".into());
            }
        }
        let transport = Arc::new(Transport {
            destination: SendRight(destination), ack: ReceivePort::new(false, 8)?, input: ReceivePort::new(true, INPUT_QUEUE_LIMIT)?,
            nonce, generation: endpoint.generation, stopped: AtomicBool::new(false),
            state: Mutex::new(State { epoch: 0, visible: false, next_frame: 1, slots: [SlotState::Free; RING_SLOTS], last_input_sequence: 0,
                pressed_keys: HashSet::new(), pressed_buttons: HashSet::new(), events: VecDeque::new(), failed: false }),
        });
        for (name, ack) in [("nova-mac-frame-ack", true), ("nova-mac-input", false)] {
            let shared = Arc::clone(&transport);
            if let Err(error) = std::thread::Builder::new().name(name.into()).spawn(move || receive_events(shared, ack)) {
                transport.stopped.store(true, Ordering::Release);
                return Err(format!("start macOS presentation transport: {error}"));
            }
        }
        Ok(Self { transport, ring: None, next_ring_generation: 1 })
    }

    /// device MUST be the renderer's actual selected Metal device, including
    /// the one behind wgpu/Metal or MoltenVK's selected VkPhysicalDevice.
    pub fn allocate_ring(&mut self, device: &DeviceRef, width: u32, height: u32, format: MTLPixelFormat, budget_bytes: u64) -> Result<RingLayout, String> {
        if width == 0 || height == 0 || width > 16_384 || height > 16_384 || !matches!(format, MTLPixelFormat::BGRA8Unorm | MTLPixelFormat::BGRA8Unorm_sRGB) {
            return Err("invalid IOSurface presentation extent/format".into());
        }
        let device_id: u64 = unsafe { msg_send![device.as_ptr(), registryID] };
        if QUARANTINED.get_or_init(|| Mutex::new(Vec::new())).lock().map_err(|_| "GPU quarantine lock failed")?.iter().any(|surface| surface.device_id == device_id) {
            return Err("a previous GPU writer needs confirmed device-idle cleanup before a new ring".into());
        }
        if self.ring.as_ref().is_some_and(|ring| ring.layout.width == width && ring.layout.height == height && ring.layout.format == format) {
            return Ok(self.ring.as_ref().unwrap().layout);
        }
        let state = self.transport.state.lock().map_err(|_| "presentation state lock failed")?;
        if state.failed || self.transport.stopped.load(Ordering::Acquire) { return Err("presentation transport has stopped".into()); }
        if state.slots.iter().any(|slot| !matches!(slot, SlotState::Free)) { return Err("presentation ring resize waits for outstanding GPU/Host ownership".into()); }
        // Exclusive &mut self prevents any new acquisition during allocation.
        drop(state);
        let bytes_per_row = unsafe { IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, width as usize * 4) };
        let bytes = bytes_per_row.checked_mul(height as usize).ok_or("IOSurface byte size overflow")?;
        let expected = (bytes as u64).checked_mul(RING_SLOTS as u64).ok_or("presentation ring byte size overflow")?;
        if bytes_per_row < width as usize * 4 || bytes as u64 > wire::MAX_SURFACE_BYTES || expected > budget_bytes {
            return Err("IOSurface ring exceeds its granted GPU/memory budget".into());
        }
        // The caller releases its old imported VkImages/wgpu textures before
        // this resize. ACK/free slots prove neither GPU still owns this ring;
        // release our old ring before allocating another, not at twice the peak.
        self.ring = None;
        let mut surfaces = Vec::with_capacity(RING_SLOTS);
        for _ in 0..RING_SLOTS { surfaces.push(Arc::new(allocate_surface(device, width, height, format, bytes_per_row, bytes)?)); }
        let actual = surfaces.iter().map(|surface| surface.bytes).sum::<u64>();
        if actual > budget_bytes || surfaces.iter().any(|surface| surface.bytes > wire::MAX_SURFACE_BYTES) { return Err("actual IOSurface allocation exceeds the granted memory budget".into()); }
        let layout = RingLayout { width, height, extent: [width, height], format, generation: self.next_ring_generation,
            bytes_per_row: surfaces[0].bytes_per_row, total_bytes: actual, slot_count: RING_SLOTS };
        let slots: [Arc<SurfaceTexture>; RING_SLOTS] = surfaces.try_into().map_err(|_| "invalid presentation ring slot count")?;
        self.ring = Some(Arc::new(Ring { slots, layout }));
        self.next_ring_generation = self.next_ring_generation.checked_add(1).ok_or("presentation ring generation exhausted")?;
        Ok(layout)
    }

    pub fn set_presentation(&mut self, epoch: u64, visible: bool) -> Result<(), String> {
        if epoch == 0 { return Err("native presentation epoch must be non-zero".into()); }
        let mut state = self.transport.state.lock().map_err(|_| "presentation state lock failed")?;
        if epoch < state.epoch { return Ok(()); }
        if epoch != state.epoch || !visible {
            state.events.retain(|event| matches!(event, ProducerEvent::Failed(_)));
            state.pressed_keys.clear(); state.pressed_buttons.clear(); state.last_input_sequence = 0;
            state.push(ProducerEvent::ResetInput { epoch });
        }
        state.epoch = epoch; state.visible = visible && !state.failed;
        Ok(())
    }

    /// None is normal bounded backpressure or a hidden view: do not block the
    /// application, recreate geometry, or allocate an extra ring as a shortcut.
    pub fn acquire_slot(&mut self, epoch: u64) -> Result<Option<SlotLease>, String> {
        let ring = self.ring.as_ref().ok_or("presentation ring has not been allocated")?;
        let mut state = self.transport.state.lock().map_err(|_| "presentation state lock failed")?;
        if state.failed || self.transport.stopped.load(Ordering::Acquire) { return Err("presentation transport has stopped".into()); }
        if !state.visible || epoch != state.epoch { return Ok(None); }
        let Some(index) = state.slots.iter().position(|slot| matches!(slot, SlotState::Free)) else { return Ok(None); };
        let frame_id = state.next_frame; state.next_frame = state.next_frame.checked_add(1).ok_or("native frame counter exhausted")?;
        state.slots[index] = SlotState::Rendering { epoch, frame_id };
        Ok(Some(SlotLease { transport: Arc::clone(&self.transport), surface: Arc::clone(&ring.slots[index]), layout: ring.layout,
            index, epoch, frame_id, active: true, gpu_complete: false }))
    }

    /// Feed Input into the existing camera adapter, ResetInput into its normal
    /// focus-lost path, and Displayed into real first-frame diagnostics.
    pub fn drain_events(&mut self, max: usize) -> Vec<ProducerEvent> {
        let Ok(mut state) = self.transport.state.lock() else { return vec![ProducerEvent::Failed("presentation state lock failed".into())]; };
        let count = max.min(EVENT_LIMIT).min(state.events.len());
        state.events.drain(..count).collect()
    }
    pub fn layout(&self) -> Option<RingLayout> { self.ring.as_ref().map(|ring| ring.layout) }
    pub fn poll_events(&mut self) -> Vec<ProducerEvent> { self.drain_events(EVENT_LIMIT) }
    pub fn ring_is_idle(&self) -> bool { self.transport.state.lock().map(|state| state.slots.iter().all(|slot| matches!(slot, SlotState::Free))).unwrap_or(false) }
    pub fn can_replace_ring(&self) -> bool { self.ring_is_idle() }

    /// # Safety
    /// The selected producer device is genuinely idle, including ALL queues
    /// that could have touched abandoned leases (e.g. successful vkDeviceWaitIdle).
    /// A timeout, one queue submission, or a lost UI lease is not sufficient.
    pub unsafe fn release_quarantined_after_device_idle(&mut self, device: &DeviceRef) -> Result<(), String> {
        let device_id: u64 = unsafe { msg_send![device.as_ptr(), registryID] };
        QUARANTINED.get_or_init(|| Mutex::new(Vec::new())).lock().map_err(|_| "GPU quarantine lock failed")?
            .retain(|surface| surface.device_id != device_id);
        Ok(())
    }
}
impl Drop for MacPresentationProducer { fn drop(&mut self) { self.transport.stopped.store(true, Ordering::Release); } }

pub struct SlotLease {
    transport: Arc<Transport>, surface: Arc<SurfaceTexture>, layout: RingLayout,
    index: usize, epoch: u64, frame_id: u64, active: bool, gpu_complete: bool,
}
impl SlotLease {
    pub fn texture(&self) -> &TextureRef { &self.surface.texture }
    /// Same-process integration only. Send the IOSurface by Mach right, never
    /// serialize this pointer or use a global IOSurface ID lookup.
    pub fn iosurface(&self) -> *mut c_void { self.surface.surface.0 }
    pub fn slot_index(&self) -> usize { self.index }
    pub fn frame_id(&self) -> u64 { self.frame_id }
    pub fn layout(&self) -> RingLayout { self.layout }

    /// # Safety
    /// All writes to this slot on the actual producer GPU have completed and
    /// are visible to an IOSurface texture reader. A queue submission, scene
    /// preparation, or CPU-side encoder completion is NOT sufficient evidence.
    /// Do not touch this texture again after handing ownership to this call.
    pub unsafe fn publish_after_gpu_complete(mut self, real_geometry: bool) -> Result<PublishOutcome, String> {
        self.gpu_complete = true;
        {
            let mut state = self.transport.state.lock().map_err(|_| "presentation state lock failed")?;
            if state.failed || self.transport.stopped.load(Ordering::Acquire) || !state.visible || state.epoch != self.epoch {
                state.free_rendering(self.index, self.frame_id); self.active = false;
                return Ok(PublishOutcome::Discarded);
            }
            if !matches!(state.slots[self.index], SlotState::Rendering { epoch, frame_id } if epoch == self.epoch && frame_id == self.frame_id) {
                return Err("native presentation slot lease changed".into());
            }
        }
        let surface_port = unsafe { IOSurfaceCreateMachPort(self.surface.surface.0) };
        if surface_port == 0 { return Err("export final-colour IOSurface Mach right failed".into()); }
        let surface_port = SendRight(surface_port);
        let frame = wire::FrameHeader { nonce: self.transport.nonce, generation: self.transport.generation, epoch: self.epoch,
            frame_id: self.frame_id, width: self.layout.width, height: self.layout.height,
            format: if self.layout.format == MTLPixelFormat::BGRA8Unorm_sRGB { wire::BGRA8_SRGB } else { wire::BGRA8_UNORM },
            flags: wire::GPU_COMPLETE | if real_geometry { wire::REAL_GEOMETRY } else { 0 } };
        let mut message = FrameMessage {
            header: Header { bits: COMPLEX | u32::from(COPY_SEND) | (MAKE_SEND_ONCE << 8), size: size_of::<FrameMessage>() as u32,
                remote: self.transport.destination.0, local: self.transport.ack.0, voucher: 0, id: wire::FRAME_MESSAGE_ID }, count: 2,
            ports: [PortDescriptor { name: surface_port.0, pad1: 0, pad2: 0, disposition: COPY_SEND, kind: 0 },
                PortDescriptor { name: self.transport.input.0, pad1: 0, pad2: 0, disposition: COPY_SEND, kind: 0 }], payload: frame.encode(),
        };
        // Publish state before sending, so a very fast Host ACK cannot race the
        // InFlight transition. COPY_SEND keeps local RAII ownership unambiguous.
        {
            let mut state = self.transport.state.lock().map_err(|_| "presentation state lock failed")?;
            if state.failed || self.transport.stopped.load(Ordering::Acquire) || !state.visible || state.epoch != self.epoch {
                state.free_rendering(self.index, self.frame_id); self.active = false; return Ok(PublishOutcome::Discarded);
            }
            state.slots[self.index] = SlotState::InFlight { epoch: self.epoch, frame_id: self.frame_id, sent_at: Instant::now() };
        }
        self.active = false;
        let send_size = message.header.size;
        let code = unsafe { mach_msg(&mut message.header, SEND | SEND_TIMEOUT, send_size, 0, 0, 0, 0) };
        if code != 0 {
            // A failed submission never grants permission to keep rendering
            // into a possibly shared slot. Stop this transport, do not retry
            // with a new ring or silently fall back to a window/CPU copy.
            self.transport.fail(format!("native frame Mach send failed: {code}"));
            return Err(format!("native frame Mach send failed: {code}"));
        }
        Ok(PublishOutcome::Sent { frame_id: self.frame_id })
    }

    /// Native Metal callers can register the actual command completion instead
    /// of synchronously waiting or making an unsupported completion assertion.
    pub fn publish_after_metal_command(self, command: &CommandBufferRef, real_geometry: bool) {
        let pending = Mutex::new(Some(self));
        let handler = block::ConcreteBlock::new(move |completed: &CommandBufferRef| {
            let Some(lease) = pending.lock().ok().and_then(|mut pending| pending.take()) else { return; };
            if completed.status() == MTLCommandBufferStatus::Completed {
                let transport = Arc::clone(&lease.transport);
                if let Err(error) = unsafe { lease.publish_after_gpu_complete(real_geometry) } { transport.fail(error); }
            } else {
                let transport = Arc::clone(&lease.transport);
                // A terminal command error is still a completed ownership
                // interval. It is not an image to publish, but it can retire.
                unsafe { lease.discard_after_gpu_complete(); }
                transport.fail("producer Metal command failed before final-colour publication".into());
            }
        }).copy();
        command.add_completed_handler(&handler);
    }

    /// # Safety
    /// No GPU has been submitted with access to this lease's surface.
    pub unsafe fn discard_unsubmitted(mut self) { self.gpu_complete = true; }

    /// # Safety
    /// The actual GPU writer reached a terminal completion/fence or device-idle.
    pub unsafe fn discard_after_gpu_complete(mut self) { self.gpu_complete = true; }
}
impl Drop for SlotLease {
    fn drop(&mut self) {
        if !self.active { return; }
        if !self.gpu_complete {
            let mut surfaces = QUARANTINED.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            surfaces.push(Arc::clone(&self.surface));
        }
        if let Ok(mut state) = self.transport.state.lock() {
            if self.gpu_complete { state.free_rendering(self.index, self.frame_id); }
            else {
                state.slots[self.index] = SlotState::Quarantined;
                state.fail("GPU ownership ended without completion evidence; slot quarantined until device-idle".into());
            }
        }
    }
}

fn allocate_surface(device: &DeviceRef, width: u32, height: u32, format: MTLPixelFormat, stride: usize, bytes: usize) -> Result<SurfaceTexture, String> {
    unsafe {
        let keys = [kIOSurfaceWidth, kIOSurfaceHeight, kIOSurfaceBytesPerElement, kIOSurfaceBytesPerRow, kIOSurfaceAllocSize, kIOSurfacePixelFormat];
        let values = [i64::from(width), i64::from(height), 4, stride as i64, bytes as i64, i64::from(u32::from_be_bytes(*b"BGRA"))];
        let mut numbers = Vec::with_capacity(values.len());
        for value in values {
            let number = CFNumberCreate(ptr::null(), 4, (&value as *const i64).cast());
            if number.is_null() { return Err("allocate IOSurface property failed".into()); }
            numbers.push(CfOwned(number));
        }
        let pointers: Vec<*const c_void> = numbers.iter().map(|number| number.0 as *const c_void).collect();
        let dictionary = CFDictionaryCreate(ptr::null(), keys.as_ptr(), pointers.as_ptr(), keys.len() as isize,
            ptr::addr_of!(kCFTypeDictionaryKeyCallBacks).cast(), ptr::addr_of!(kCFTypeDictionaryValueCallBacks).cast());
        if dictionary.is_null() { return Err("allocate IOSurface properties failed".into()); }
        let dictionary = CfOwned(dictionary);
        let surface = IOSurfaceCreate(dictionary.0);
        if surface.is_null() { return Err("allocate final-colour IOSurface failed".into()); }
        let surface = CfOwned(surface);
        let actual_bytes = IOSurfaceGetAllocSize(surface.0) as u64;
        let actual_stride = IOSurfaceGetBytesPerRow(surface.0);
        if actual_bytes < bytes as u64 || actual_stride < width as usize * 4 || actual_bytes > wire::MAX_SURFACE_BYTES {
            return Err("IOSurface allocation layout exceeds the bounded request".into());
        }
        let descriptor = TextureDescriptor::new(); descriptor.set_texture_type(MTLTextureType::D2);
        descriptor.set_width(u64::from(width)); descriptor.set_height(u64::from(height)); descriptor.set_mipmap_level_count(1);
        descriptor.set_storage_mode(MTLStorageMode::Shared); descriptor.set_usage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        descriptor.set_pixel_format(format);
        let texture: *mut metal::MTLTexture = msg_send![device.as_ptr(), newTextureWithDescriptor: descriptor.as_ptr() iosurface: surface.0 plane: 0_u64];
        if texture.is_null() { return Err("selected renderer GPU cannot import its IOSurface".into()); }
        let device_id: u64 = msg_send![device.as_ptr(), registryID];
        Ok(SurfaceTexture { surface, texture: Texture::from_ptr(texture), bytes: actual_bytes, bytes_per_row: actual_stride, device_id })
    }
}

fn receive_events(transport: Arc<Transport>, ack_only: bool) {
    let port = if ack_only { transport.ack.0 } else { transport.input.0 };
    while !transport.stopped.load(Ordering::Acquire) {
        let mut message: EventMessage = unsafe { zeroed() };
        let result = unsafe { mach_msg(&mut message.header, RECEIVE | RECEIVE_TIMEOUT, 0, size_of::<EventMessage>() as u32, port, 100, 0) };
        if result == RECEIVE_TIMED_OUT {
            if ack_only {
                if let Ok(mut state) = transport.state.lock() {
                    if state.slots.iter().any(|slot| matches!(slot, SlotState::InFlight { sent_at, .. } if sent_at.elapsed() > ACK_TIMEOUT)) {
                        state.fail("Host presentation ACK timed out; shared slots are not reused".into());
                    }
                }
            }
            continue;
        }
        if result != 0 { transport.fail(format!("native presentation receive failed: {result}")); break; }
        let payload_bytes = message.header.size.saturating_sub(size_of::<Header>() as u32) as usize;
        if message.header.bits & COMPLEX == 0 && payload_bytes <= message.payload.len() {
            let bytes = &message.payload[..payload_bytes];
            if ack_only && message.header.id == wire::ACK_MESSAGE_ID { receive_ack(&transport, bytes); }
            else if !ack_only && message.header.id == wire::INPUT_MESSAGE_ID { receive_input(&transport, bytes); }
            else if !ack_only && message.header.id == wire::DISPLAY_MESSAGE_ID { receive_display(&transport, bytes); }
        }
        unsafe { mach_msg_destroy(&mut message.header); }
    }
}

fn receive_ack(transport: &Transport, bytes: &[u8]) {
    if bytes.len() != wire::ACK_BYTES || &bytes[..4] != b"NVMA" || u32_at(bytes, 4) != 1 || u64_at(bytes, 8) != transport.generation { return; }
    let (frame_id, status) = (u64_at(bytes, 16), u32_at(bytes, 24));
    if status > 2 { return; }
    let Ok(mut state) = transport.state.lock() else { return; };
    if let Some(index) = state.slots.iter().position(|slot| matches!(slot, SlotState::InFlight { frame_id: current, .. } if *current == frame_id)) {
        if let SlotState::InFlight { epoch, .. } = state.slots[index] {
            state.slots[index] = SlotState::Free;
            if status == 2 { state.push(ProducerEvent::FrameRejected { epoch, frame_id }); }
        }
    }
}

fn receive_input(transport: &Transport, bytes: &[u8]) {
    if bytes.len() != wire::INPUT_BYTES || &bytes[..4] != b"NVMI" || u32_at(bytes, 4) != 1
        || bytes[8..24] != transport.nonce || u64_at(bytes, 24) != transport.generation { return; }
    let (epoch, sequence, kind, flags) = (u64_at(bytes, 32), u64_at(bytes, 40), u32_at(bytes, 48), u32_at(bytes, 52));
    if !(1..=9).contains(&kind) || sequence == 0 { return; }
    let (x, y, dx, dy) = (f64_at(bytes, 56), f64_at(bytes, 64), f64_at(bytes, 72), f64_at(bytes, 80));
    if ![x, y, dx, dy].iter().all(|value| value.is_finite() && value.abs() <= 1e7) { return; }
    let length = u32_at(bytes, 96) as usize;
    if length > 28 { return; }
    let Ok(text) = std::str::from_utf8(&bytes[100..100 + length]) else { return; };
    let (code, modifiers) = (u32_at(bytes, 88), u32_at(bytes, 92));
    let Ok(mut state) = transport.state.lock() else { return; };
    if state.failed || epoch != state.epoch || sequence <= state.last_input_sequence || (!state.visible && !matches!(kind, 7 | 8 | 9)) { return; }
    if kind == 8 && (x < 1.0 || y < 1.0 || x > 16_384.0 || y > 16_384.0 || x.fract() != 0.0 || y.fract() != 0.0 || x * y * 4.0 > wire::MAX_SURFACE_BYTES as f64 || dx <= 0.0 || dx > 16.0) { return; }
    state.last_input_sequence = sequence;
    if kind == 8 {
        state.push(ProducerEvent::ViewportResized { epoch, extent: [x as u32, y as u32], scale: dx });
        return;
    }
    if kind == 9 {
        state.visible = false;
        state.pressed_keys.clear(); state.pressed_buttons.clear();
        state.push(ProducerEvent::ResetInput { epoch });
        state.push(ProducerEvent::Hidden { epoch });
        return;
    }
    match kind {
        2 if code < 32 => { state.pressed_buttons.insert(code); }
        3 => { state.pressed_buttons.remove(&code); }
        5 if code <= 255 => { state.pressed_keys.insert(code); }
        6 => { state.pressed_keys.remove(&code); }
        7 if flags & 1 == 0 => { state.pressed_keys.clear(); state.pressed_buttons.clear(); state.push(ProducerEvent::ResetInput { epoch }); }
        _ => {}
    }
    state.push(ProducerEvent::Input(ProducerInput { generation: transport.generation, epoch, sequence, kind, flags, x, y, dx, dy, code, modifiers, text: text.into() }));
}

fn receive_display(transport: &Transport, bytes: &[u8]) {
    if bytes.len() != wire::DISPLAY_BYTES || &bytes[..4] != b"NVMP" || u32_at(bytes, 4) != 1
        || bytes[8..24] != transport.nonce || u64_at(bytes, 24) != transport.generation { return; }
    let epoch = u64_at(bytes, 32); let frame_id = u64_at(bytes, 40); let time = f64_at(bytes, 48);
    if !time.is_finite() || time <= 0.0 || frame_id == 0 { return; }
    let Ok(mut state) = transport.state.lock() else { return; };
    if state.failed || !state.visible || epoch != state.epoch || frame_id >= state.next_frame { return; }
    state.push(ProducerEvent::Displayed { epoch, frame_id, presented_time_seconds: time, received_at: Instant::now() });
}

fn parse_nonce(value: &str) -> Result<[u8; 16], String> {
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) { return Err("invalid native presentation nonce".into()); }
    let mut nonce = [0; 16];
    for (index, value) in value.as_bytes().chunks_exact(2).enumerate() {
        let digit = |byte: u8| if byte.is_ascii_digit() { byte - b'0' } else { byte - b'a' + 10 };
        nonce[index] = (digit(value[0]) << 4) | digit(value[1]);
    }
    Ok(nonce)
}
fn u32_at(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) }
fn u64_at(bytes: &[u8], offset: usize) -> u64 { u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) }
fn f64_at(bytes: &[u8], offset: usize) -> f64 { f64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) }
