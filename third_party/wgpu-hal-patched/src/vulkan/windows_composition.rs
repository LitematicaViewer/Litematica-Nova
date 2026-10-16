//! Same-VkDevice shared NT texture import for a native DirectComposition target.
//! This contains presentation ownership/synchronization only, no renderer.
use std::{ffi::c_void, sync::{Arc, atomic::{AtomicBool, Ordering}}};
use ash::vk;

pub struct NativeCompositionImage {
    owner: Arc<super::DeviceShared>,
    pub(super) memory: vk::DeviceMemory,
    image: vk::Image,
    pool: vk::CommandPool,
    commands: [vk::CommandBuffer; 2],
    initialized: AtomicBool,
}

impl NativeCompositionImage {
    /// Arms exactly the next submit. The caller must not concurrently submit
    /// another frame, and must wait for D3D key 1 before rearming/dropping.
    pub unsafe fn arm_next_submit(self: &Arc<Self>) -> Result<(), crate::DeviceError> {
        let mut slot = self.owner.native_composition_submit.lock();
        if slot.as_ref().and_then(std::sync::Weak::upgrade).is_some() {
            return Err(crate::DeviceError::Lost);
        }
        let raw = &self.owner.raw;
        // These barriers do not depend on frame data. Record them once and
        // reuse without resetting a command buffer that may still be pending
        // from Vulkan's host-visible perspective: a D3D keyed-mutex wait is
        // real cross-API synchronization, but is not a Vulkan fence wait.
        if !self.initialized.load(Ordering::Relaxed) {
        for (index, &command) in self.commands.iter().enumerate() {
            unsafe { raw.reset_command_buffer(command, vk::CommandBufferResetFlags::empty())?; }
            unsafe { raw.begin_command_buffer(command, &vk::CommandBufferBeginInfo::builder()
                .flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE))?; }
            let acquire = index == 0;
            let barrier = vk::ImageMemoryBarrier::builder().image(self.image)
                .subresource_range(vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 })
                // Presentation fully overwrites the image on every frame; old
                // shared contents are deliberately discarded, never sampled.
                .old_layout(if acquire { vk::ImageLayout::UNDEFINED }
                    else { vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL })
                .new_layout(if acquire { vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL } else { vk::ImageLayout::GENERAL })
                .src_queue_family_index(if acquire { vk::QUEUE_FAMILY_EXTERNAL } else { self.owner.family_index })
                .dst_queue_family_index(if acquire { self.owner.family_index } else { vk::QUEUE_FAMILY_EXTERNAL })
                .src_access_mask(if acquire { vk::AccessFlags::empty() } else { vk::AccessFlags::COLOR_ATTACHMENT_WRITE })
                .dst_access_mask(if acquire { vk::AccessFlags::COLOR_ATTACHMENT_WRITE } else { vk::AccessFlags::empty() });
            unsafe { raw.cmd_pipeline_barrier(command, vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::ALL_COMMANDS, vk::DependencyFlags::empty(), &[], &[], &[*barrier]);
                raw.end_command_buffer(command)?; }
        }
        }
        *slot = Some(Arc::downgrade(self));
        Ok(())
    }

    pub(super) fn commands(&self) -> [vk::CommandBuffer; 2] { self.commands }
    pub(super) fn submitted(&self) { self.initialized.store(true, Ordering::Relaxed); }
}

impl Drop for NativeCompositionImage {
    fn drop(&mut self) { unsafe {
        // The application normally drains its frame before dropping the target.
        // Retire outstanding uses on error too, before destroying the alias.
        let _ = self.owner.raw.queue_wait_idle(self.owner.raw_queue);
        if self.pool != vk::CommandPool::null() { self.owner.raw.destroy_command_pool(self.pool, None); }
        if self.image != vk::Image::null() { self.owner.raw.destroy_image(self.image, None); }
        if self.memory != vk::DeviceMemory::null() { self.owner.raw.free_memory(self.memory, None); }
    } }
}

impl super::Adapter {
    pub fn adapter_luid(&self) -> Option<[u8; 8]> {
        let mut identity = vk::PhysicalDeviceIDProperties::default();
        let mut properties = vk::PhysicalDeviceProperties2::builder().push_next(&mut identity);
        unsafe { self.instance.raw.get_physical_device_properties2(self.raw, &mut properties); }
        (identity.device_luid_valid != 0).then_some(identity.device_luid)
    }
}

impl super::Device {
    /// Import a D3D11_TEXTURE NT handle into this exact wgpu Vulkan device.
    /// The application retains the D3D texture/mutex until the returned texture
    /// and token are dropped. The handle remains owned by the caller.
    pub unsafe fn import_native_composition_image(&self, handle: *mut c_void,
        desc: &crate::TextureDescriptor<'_>) -> Result<(super::Texture, Arc<NativeCompositionImage>), crate::DeviceError> {
        if handle.is_null() || desc.format != wgt::TextureFormat::Bgra8Unorm || desc.sample_count != 1
            || desc.dimension != wgt::TextureDimension::D2 || desc.mip_level_count != 1
            || desc.usage != crate::TextureUses::COLOR_TARGET
            || desc.size.width == 0 || desc.size.height == 0 || desc.size.depth_or_array_layers != 1 {
            return Err(crate::DeviceError::Lost);
        }
        for extension in [vk::KhrExternalMemoryWin32Fn::name(), vk::KhrWin32KeyedMutexFn::name()] {
            if !self.shared.enabled_extensions.contains(&extension) {
                log::error!("native Vulkan composition requires enabled {:?}", extension);
                return Err(crate::DeviceError::Lost);
            }
        }
        let mut external = vk::PhysicalDeviceExternalImageFormatInfo::builder()
            .handle_type(vk::ExternalMemoryHandleTypeFlags::D3D11_TEXTURE);
        let info = vk::PhysicalDeviceImageFormatInfo2::builder().format(vk::Format::B8G8R8A8_UNORM)
            .ty(vk::ImageType::TYPE_2D).tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT).push_next(&mut external);
        let mut external_properties = vk::ExternalImageFormatProperties::default();
        let mut properties = vk::ImageFormatProperties2::builder().push_next(&mut external_properties);
        unsafe { self.shared.instance.raw.get_physical_device_image_format_properties2(self.shared.physical_device, &info, &mut properties)?; }
        let limits = properties.image_format_properties;
        let capabilities = external_properties.external_memory_properties;
        if !capabilities.external_memory_features.contains(vk::ExternalMemoryFeatureFlags::IMPORTABLE)
            || !capabilities.compatible_handle_types.contains(vk::ExternalMemoryHandleTypeFlags::D3D11_TEXTURE)
            || desc.size.width > limits.max_extent.width || desc.size.height > limits.max_extent.height
            || !limits.sample_counts.contains(vk::SampleCountFlags::TYPE_1) {
            log::error!("native Vulkan composition BGRA import unsupported: {:?}, {:?}", capabilities, limits);
            return Err(crate::DeviceError::Lost);
        }
        let mut target = NativeCompositionImage { owner: Arc::clone(&self.shared), memory: vk::DeviceMemory::null(),
            image: vk::Image::null(), pool: vk::CommandPool::null(), commands: [vk::CommandBuffer::null(); 2], initialized: AtomicBool::new(false) };
        let raw = &self.shared.raw;
        let mut external = vk::ExternalMemoryImageCreateInfo::builder().handle_types(vk::ExternalMemoryHandleTypeFlags::D3D11_TEXTURE);
        target.image = unsafe { raw.create_image(&vk::ImageCreateInfo::builder().push_next(&mut external)
            .image_type(vk::ImageType::TYPE_2D).format(vk::Format::B8G8R8A8_UNORM)
            .extent(vk::Extent3D { width: desc.size.width, height: desc.size.height, depth: 1 })
            .mip_levels(1).array_layers(1).samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL).usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .sharing_mode(vk::SharingMode::EXCLUSIVE).initial_layout(vk::ImageLayout::UNDEFINED), None)? };
        let requirements = unsafe { raw.get_image_memory_requirements(target.image) };
        if requirements.size > limits.max_resource_size { return Err(crate::DeviceError::OutOfMemory); }
        let importer = ash::extensions::khr::ExternalMemoryWin32::new(&self.shared.instance.raw, raw);
        let handle_properties = unsafe { importer.get_memory_win32_handle_properties(vk::ExternalMemoryHandleTypeFlags::D3D11_TEXTURE, handle)? };
        let types = requirements.memory_type_bits & handle_properties.memory_type_bits;
        if types == 0 { return Err(crate::DeviceError::Lost); }
        let mut import = vk::ImportMemoryWin32HandleInfoKHR::builder().handle_type(vk::ExternalMemoryHandleTypeFlags::D3D11_TEXTURE).handle(handle);
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::builder().image(target.image);
        target.memory = unsafe { raw.allocate_memory(&vk::MemoryAllocateInfo::builder().allocation_size(requirements.size)
            .memory_type_index(types.trailing_zeros()).push_next(&mut import).push_next(&mut dedicated), None)? };
        unsafe { raw.bind_image_memory(target.image, target.memory, 0)?; }
        target.pool = unsafe { raw.create_command_pool(&vk::CommandPoolCreateInfo::builder()
            .queue_family_index(self.shared.family_index).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER), None)? };
        let commands = unsafe { raw.allocate_command_buffers(&vk::CommandBufferAllocateInfo::builder()
            .command_pool(target.pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(2))? };
        target.commands.copy_from_slice(&commands);
        let target = Arc::new(target);
        let texture = unsafe { Self::texture_from_raw(target.image, desc, Some(Box::new(Arc::clone(&target)))) };
        Ok((texture, target))
    }
}
