//! Process-local viewport state for the Host-owned macOS native view.
//! No NSWindow or cross-process NSView pointer is created here.
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::{Context, Result, ensure};
use viewer_launch_contract::{NativeCompositionEndpoint, ViewerSessionCommand, ViewerSessionCommandKind};
use crate::macos_presentation::MacPresentationProducer;
use crate::macos_presentation::ProducerEvent;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewportState {
    pub extent: [u32; 2],
    pub scale: f64,
    pub epoch: u64,
    pub visible: bool,
    pub closed: bool,
}

pub(crate) struct MacSharedViewport {
    state: Mutex<ViewportState>,
    producer: Arc<Mutex<MacPresentationProducer>>,
    redraw: AtomicBool,
    presentation_budget: AtomicU64,
    displayed: Mutex<Option<(u64, u64, f64)>>,
}

impl MacSharedViewport {
    pub(crate) fn connect(endpoint: NativeCompositionEndpoint, extent: [u32; 2], scale: f64) -> Result<Arc<Self>> {
        endpoint.validate().map_err(anyhow::Error::msg)?;
        ensure!(extent[0] > 0 && extent[1] > 0 && extent[0] <= 16384 && extent[1] <= 16384, "invalid native viewport extent");
        ensure!(scale.is_finite() && scale > 0.0, "invalid native viewport scale");
        let producer = MacPresentationProducer::connect(endpoint).map_err(anyhow::Error::msg)?;
        Ok(Arc::new(Self {
            state: Mutex::new(ViewportState { extent, scale, epoch: 0, visible: false, closed: false }),
            producer: Arc::new(Mutex::new(producer)),
            redraw: AtomicBool::new(true),
            presentation_budget: AtomicU64::new(0),
            displayed: Mutex::new(None),
        }))
    }
    pub(crate) fn snapshot(&self) -> ViewportState { *self.state.lock().unwrap_or_else(|e| e.into_inner()) }
    pub(crate) fn extent(&self) -> [u32; 2] { self.snapshot().extent }
    pub(crate) fn scale_factor(&self) -> f64 { self.snapshot().scale }
    pub(crate) fn visible(&self) -> bool { self.snapshot().visible }
    pub(crate) fn epoch(&self) -> u64 { self.snapshot().epoch }
    pub(crate) fn closed(&self) -> bool { self.snapshot().closed }
    pub(crate) fn producer(&self) -> Arc<Mutex<MacPresentationProducer>> { Arc::clone(&self.producer) }
    /// The event loop is the only transport consumer. Renderers read the
    /// cached display receipt instead of stealing camera/input events.
    pub(crate) fn poll_events(&self) -> Vec<ProducerEvent> {
        let events = self.producer.lock().unwrap_or_else(|e| e.into_inner()).poll_events();
        for event in &events {
            if let ProducerEvent::ViewportResized { epoch, extent, scale } = event {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if state.epoch == *epoch && !state.closed && extent.iter().all(|n| *n > 0 && *n <= 16384)
                    && scale.is_finite() && *scale > 0.0 && *scale <= 16.0 {
                    state.extent = *extent;
                    state.scale = *scale;
                    *self.displayed.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    self.request_redraw();
                }
            }
            if let ProducerEvent::Hidden { epoch } = event {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if state.epoch == *epoch {
                    state.visible = false;
                    *self.displayed.lock().unwrap_or_else(|e| e.into_inner()) = None;
                }
            }
            if let ProducerEvent::Displayed { epoch, frame_id, presented_time_seconds, .. } = event {
                let state = self.snapshot();
                if state.visible && *epoch == state.epoch && presented_time_seconds.is_finite() && *presented_time_seconds > 0.0 {
                    let mut displayed = self.displayed.lock().unwrap_or_else(|e| e.into_inner());
                    if displayed.is_none_or(|last| last.0 != *epoch || last.1 < *frame_id) {
                        *displayed = Some((*epoch, *frame_id, *presented_time_seconds));
                    }
                }
            }
        }
        events
    }
    pub(crate) fn last_display_receipt(&self) -> Option<(u64, f64)> {
        let state = self.snapshot();
        self.displayed.lock().unwrap_or_else(|e| e.into_inner()).filter(|receipt|
            state.visible && receipt.0 == state.epoch).map(|receipt| (receipt.1, receipt.2))
    }
    pub(crate) fn presentation_budget(&self) -> u64 { self.presentation_budget.load(Ordering::Acquire) }
    pub(crate) fn set_presentation_budget(&self, bytes: u64) { self.presentation_budget.store(bytes, Ordering::Release); }
    pub(crate) fn request_redraw(&self) { self.redraw.store(true, Ordering::Release); }
    pub(crate) fn take_redraw(&self) -> bool { self.redraw.swap(false, Ordering::AcqRel) }
    pub(crate) fn set_extent(&self, extent: [u32; 2]) {
        if extent[0] == 0 || extent[1] == 0 || extent[0] > 16384 || extent[1] > 16384 { return; }
        self.state.lock().unwrap_or_else(|e| e.into_inner()).extent = extent;
        self.request_redraw();
    }
    pub(crate) fn suspend(&self) {
        let epoch = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.visible = false;
            *self.displayed.lock().unwrap_or_else(|e| e.into_inner()) = None;
            state.epoch
        };
        let _ = self.producer.lock().unwrap_or_else(|e| e.into_inner()).set_presentation(epoch, false);
    }

    /// UI epochs, not transport sequence numbers, govern native visibility.
    pub(crate) fn apply(&self, command: &ViewerSessionCommand) -> Result<bool> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let epoch = match command.presentation_epoch {
            Some(epoch) if epoch != 0 => epoch,
            None if matches!(command.kind, ViewerSessionCommandKind::Hide | ViewerSessionCommandKind::Release) => state.epoch,
            _ => anyhow::bail!("native presentation epoch is missing"),
        };
        if epoch < state.epoch || state.closed { return Ok(false); }
        let mut next = *state;
        next.epoch = epoch;
        match command.kind {
            ViewerSessionCommandKind::AttachEmbedded | ViewerSessionCommandKind::Resize => {
                let width = command.width.context("native viewport width is missing")?;
                let height = command.height.context("native viewport height is missing")?;
                ensure!(width > 0 && height > 0 && width <= 16384 && height <= 16384, "invalid native viewport size");
                next.extent = [width, height];
                if let Some(scale) = command.dpi_scale {
                    ensure!(scale.is_finite() && scale > 0.0 && scale <= 16.0, "invalid native viewport DPI scale");
                    next.scale = scale;
                }
                if command.kind == ViewerSessionCommandKind::AttachEmbedded { next.visible = true; }
            }
            ViewerSessionCommandKind::Hide => { next.visible = false; }
            ViewerSessionCommandKind::Release => { next.visible = false; next.closed = true; }
            ViewerSessionCommandKind::ShowPopup => { next.visible = true; }
            _ => return Ok(false),
        }
        if epoch != 0 {
            self.producer.lock().unwrap_or_else(|e| e.into_inner()).set_presentation(epoch, next.visible).map_err(anyhow::Error::msg)?;
        }
        if epoch != state.epoch || !next.visible {
            *self.displayed.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        *state = next;
        drop(state);
        self.request_redraw();
        Ok(true)
    }
}
