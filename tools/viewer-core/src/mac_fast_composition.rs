//! Import the shared native-presentation ring into Fast's existing wgpu device.
//! ViewerState still renders its real scene with its original pipelines. This
//! module only chooses the output texture and publishes completed GPU frames.
#![cfg(target_os = "macos")]

use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use crate::{mac_shared_viewport::MacSharedViewport, macos_presentation::SlotLease};

pub(crate) struct MacFastComposition {
    viewport: Arc<MacSharedViewport>,
    metal_device: metal::Device,
    format: wgpu::TextureFormat,
    extent: [u32; 2],
    pending_extent: Option<[u32; 2]>,
}

pub(crate) struct MacFastFrame {
    texture: wgpu::Texture,
    lease: Option<SlotLease>,
}

impl MacFastFrame {
    pub(crate) fn texture(&self) -> &wgpu::Texture { &self.texture }

    /// # Safety
    /// No command accessing this frame has been submitted to any GPU queue.
    /// After submission, use composition.submit or retain the lease until a
    /// genuine completion; dropping an unknown writer quarantines its slot.
    pub(crate) unsafe fn discard_unsubmitted(mut self) {
        if let Some(lease) = self.lease.take() { unsafe { lease.discard_unsubmitted(); } }
    }
}

impl MacFastComposition {
    pub(crate) fn new(
        viewport: Arc<MacSharedViewport>, adapter: &wgpu::Adapter, device: &wgpu::Device,
        width: u32, height: u32, format: wgpu::TextureFormat,
    ) -> Result<Self> {
        if adapter.get_info().backend != wgpu::Backend::Metal { bail!("macOS shared Fast presentation requires its actual Metal adapter"); }
        metal_format(format)?;
        // Extract an owning clone and leave the wgpu HAL callback BEFORE any
        // create_texture_from_hal call; re-entering its device lock deadlocks.
        let metal_device = unsafe {
            device.as_hal::<wgpu::hal::api::Metal, _, _>(|raw| {
                raw.map(|raw| raw.raw_device().lock().clone())
            })
        }.flatten().ok_or_else(|| anyhow!("the selected Fast device has no Metal HAL device"))?;
        let budget = viewport.presentation_budget();
        if budget == 0 { bail!("native presentation memory budget is unknown; ring allocation is not admitted"); }
        {
            let producer = viewport.producer();
            producer.lock().map_err(|_| anyhow!("native presentation producer lock failed"))?
                .allocate_ring(&metal_device, width, height, metal_format(format)?, budget)
                .map_err(anyhow::Error::msg)?;
        }
        Ok(Self { viewport, metal_device, format, extent: [width, height], pending_extent: None })
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 || width > 16_384 || height > 16_384 { bail!("invalid shared Fast viewport extent"); }
        if self.extent != [width, height] { self.pending_extent = Some([width, height]); }
        else { self.pending_extent = None; }
        Ok(())
    }

    pub(crate) fn begin_frame(&mut self, device: &wgpu::Device) -> Result<Option<MacFastFrame>> {
        // Nonblocking polling drives queue-completion callbacks; it is not a
        // substitute for completion and does not wait for a renderer task.
        let _ = device.poll(wgpu::Maintain::Poll);
        let snapshot = self.viewport.snapshot();
        if snapshot.closed || !snapshot.visible || snapshot.epoch == 0 { return Ok(None); }
        if snapshot.extent != self.extent { self.pending_extent = Some(snapshot.extent); }
        // Snapshot Host state before locking producer: apply() takes the
        // viewport-state lock before producer, so never invert that ordering.
        let budget = self.viewport.presentation_budget();
        let producer = self.viewport.producer();
        let mut producer = producer.lock().map_err(|_| anyhow!("native presentation producer lock failed"))?;
        if let Some(extent) = self.pending_extent {
            if !producer.can_replace_ring() { return Ok(None); }
            if budget == 0 { bail!("resized native presentation memory budget is unknown"); }
            producer.allocate_ring(&self.metal_device, extent[0], extent[1], metal_format(self.format)?, budget)
                .map_err(anyhow::Error::msg)?;
            self.extent = extent;
            self.pending_extent = None;
        }
        let Some(lease) = producer.acquire_slot(snapshot.epoch).map_err(anyhow::Error::msg)? else { return Ok(None); };
        drop(producer);
        let raw = lease.texture().to_owned();
        let extent = wgpu::Extent3d { width: self.extent[0], height: self.extent[1], depth_or_array_layers: 1 };
        let description = wgpu::TextureDescriptor {
            label: Some("Nova native IOSurface Fast output"), size: extent,
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        // The ring was allocated on THIS raw MTLDevice, dimensions and pixel
        // format match exactly, and the exclusive lease owns the GPU write.
        let texture = unsafe {
            let raw = wgpu::hal::metal::Device::texture_from_raw(raw, self.format,
                metal::MTLTextureType::D2, 1, 1,
                wgpu::hal::CopyExtent { width: extent.width, height: extent.height, depth: 1 });
            device.create_texture_from_hal::<wgpu::hal::api::Metal>(raw, &description)
        };
        Ok(Some(MacFastFrame { texture, lease: Some(lease) }))
    }

    /// Call AFTER the existing ViewerState queue.submit for the real scene.
    /// Submission return is not completion: only the queue completion callback
    /// publishes the IOSurface, and only the later Host ACK frees that slot.
    pub(crate) fn submit(
        &mut self, mut frame: MacFastFrame, device: &wgpu::Device, queue: &wgpu::Queue,
        real_geometry: bool,
    ) -> Result<()> {
        let lease = frame.lease.take().ok_or_else(|| anyhow!("native Fast frame lease was already consumed"))?;
        let texture = frame.texture;
        let viewport = Arc::clone(&self.viewport);
        queue.on_submitted_work_done(move || {
            // wgpu signals this after preceding GPU work, not at Queue::submit.
            // Retain the imported MTLTexture through this completion interval.
            let _texture = texture;
            if let Err(error) = unsafe { lease.publish_after_gpu_complete(real_geometry) } {
                eprintln!("[NOVA_NATIVE_MACOS] stage=fast_publish_failed error={error}");
            }
            viewport.request_redraw();
        });
        let _ = device.poll(wgpu::Maintain::Poll);
        Ok(())
    }

    pub(crate) fn format(&self) -> wgpu::TextureFormat { self.format }
    pub(crate) fn extent(&self) -> [u32; 2] { self.extent }
}

fn metal_format(format: wgpu::TextureFormat) -> Result<metal::MTLPixelFormat> {
    match format {
        wgpu::TextureFormat::Bgra8Unorm => Ok(metal::MTLPixelFormat::BGRA8Unorm),
        wgpu::TextureFormat::Bgra8UnormSrgb => Ok(metal::MTLPixelFormat::BGRA8Unorm_sRGB),
        _ => bail!("native IOSurface presentation requires the renderer's real BGRA8 final-colour output"),
    }
}
