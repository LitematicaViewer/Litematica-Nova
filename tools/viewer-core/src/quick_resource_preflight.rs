//! Resource accounting for the existing Fast viewer, not a renderer or a Full-mode policy.
//! Counts are metadata-derived upper bounds; the caller supplies the actual vertex ABI,
//! chunk buffer count, atlas dimensions and presentation configuration before GPU uploads.
use anyhow::{bail, ensure, Context, Result};
use serde::Serialize;

// D3D12_DEFAULT_RESOURCE_PLACEMENT_ALIGNMENT / TEXTURE_DATA_PITCH_ALIGNMENT.
const RESOURCE_ALIGNMENT: u64 = 65_536;
const UPLOAD_ROW_ALIGNMENT: u64 = 256;
const UPLOAD_PLACEMENT_ALIGNMENT: u64 = 512;
const TEXEL_BYTES: u64 = 4; // Fast RGBA8 atlas, surface color, Depth24Plus / shadow Depth32Float.

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickGpuBudget {
    pub adapter_name: String,
    pub budget_bytes: u64,
    pub current_usage_bytes: u64,
    pub available_for_reservation_bytes: u64,
    pub current_reservation_bytes: u64,
    /// Current process budget headroom, not physical VRAM or a reservation guarantee.
    pub safe_available_bytes: u64,
}

/// Query the exact DXGI adapter owned by the selected wgpu DX12 adapter (local node 0).
/// The borrowed COM pointer never transfers the HAL's ownership; QueryInterface owns
/// its extra reference until this query ends. Unsupported/zero budgets are errors.
#[cfg(windows)]
pub fn query_selected_adapter_budget(adapter: &wgpu::Adapter) -> Result<QuickGpuBudget> {
    use windows::core::{ComInterface, IUnknown, Interface};
    use windows::Win32::Graphics::Dxgi::{IDXGIAdapter3, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO};
    // Do not re-enter wgpu while its HAL adapter callback holds the adapter lock.
    let adapter_name = adapter.get_info().name;
    // SAFETY: as_hal holds the selected adapter alive throughout the callback. We only
    // query its IUnknown vtable and do not release/mutate the borrowed HAL pointer.
    unsafe {
        adapter.as_hal::<wgpu::hal::api::Dx12, _, _>(|selected| {
            let selected = selected.context("快速模式的选定适配器不是可查询预算的 DX12 适配器")?;
            let raw = selected.raw_adapter().as_mut_ptr().cast::<std::ffi::c_void>();
            let borrowed = IUnknown::from_raw_borrowed(&raw).context("选定 DX12 适配器没有有效的 DXGI 接口")?;
            let dxgi: IDXGIAdapter3 = borrowed.cast().context("选定 DX12 适配器不支持 IDXGIAdapter3 预算查询")?;
            let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            dxgi.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
                .context("选定 DX12 适配器的实时显存预算查询失败")?;
            ensure!(info.Budget > 0, "驱动返回零显存预算，预算状态未知");
            Ok(QuickGpuBudget {
                adapter_name,
                budget_bytes: info.Budget, current_usage_bytes: info.CurrentUsage,
                available_for_reservation_bytes: info.AvailableForReservation,
                current_reservation_bytes: info.CurrentReservation,
                safe_available_bytes: info.Budget.saturating_sub(info.CurrentUsage),
            })
        })
    }
}

#[cfg(not(windows))]
pub fn query_selected_adapter_budget(_adapter: &wgpu::Adapter) -> Result<QuickGpuBudget> {
    bail!("当前平台没有选定 DX12 适配器的 DXGI 实时预算，预算状态未知")
}

#[derive(Debug, Clone)]
pub struct QuickResourceInput {
    pub vertices_upper_bound: u64,
    pub vertex_stride_bytes: u64,
    /// Combined solid + translucent u32 indices. They are not zero in the indexed Fast path.
    pub indices_upper_bound: u64,
    /// At most three buffers per occupied chunk (vertex, solid, translucent).
    pub geometry_buffer_count_upper_bound: u64,
    pub atlas_width: u32,
    pub atlas_height: u32,
    pub viewport_width: u32,
    pub viewport_height: u32,
    /// DX12 uses (maximum_frame_latency + 1).min(16); default Fast latency 2 means 3.
    pub surface_buffer_count: u32,
    pub shadow_resolution: Option<u32>,
    /// Actual camera/lighting/shadow uniform ABI sizes from the caller.
    pub uniform_buffer_bytes: Vec<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickMemoryEstimate {
    pub geometry_bytes: u64,
    pub geometry_alignment_padding_bytes: u64,
    pub atlas_resident_bytes: u64,
    pub presentation_resident_bytes: u64,
    pub shadow_resident_bytes: u64,
    pub uniform_resident_bytes: u64,
    pub gpu_resident_upper_bytes: u64,
    pub upload_staging_upper_bytes: u64,
    /// Conservatively includes transient staging in the pressure calculation, including UMA.
    pub gpu_peak_upper_bytes: u64,
    /// Typed geometry payload only: excludes Vec spare capacity, NBT and temporary builders.
    /// Reported separately, never counted as extra physical VRAM or total process RAM.
    pub cpu_geometry_upper_bytes: u64,
}

fn sum(values: &[u64]) -> Result<u64> {
    values.iter().try_fold(0u64, |total, value| total.checked_add(*value).context("快速资源估算加法溢出"))
}
fn product(left: u64, right: u64) -> Result<u64> {
    left.checked_mul(right).context("快速资源估算乘法溢出")
}
fn aligned(bytes: u64, alignment: u64) -> Result<u64> {
    if bytes == 0 { return Ok(0); }
    product(bytes.checked_add(alignment - 1).context("快速资源对齐估算溢出")? / alignment, alignment)
}
fn texture_resident_upper(width: u32, height: u32) -> Result<u64> {
    ensure!(width > 0 && height > 0, "快速资源估算需要实际非零纹理尺寸");
    // Pad both dimensions to 256 texels for conservative 4-byte texture tile/row waste,
    // rather than treating tightly packed CPU image length as committed GPU residency.
    aligned(product(product(aligned(width as u64, 256)?, aligned(height as u64, 256)?)?, TEXEL_BYTES)?, RESOURCE_ALIGNMENT)
}
fn texture_staging_upper(width: u32, height: u32) -> Result<u64> {
    aligned(product(aligned(product(width as u64, TEXEL_BYTES)?, UPLOAD_ROW_ALIGNMENT)?, height as u64)?, UPLOAD_PLACEMENT_ALIGNMENT)
}

pub fn estimate_quick_resources(input: &QuickResourceInput) -> Result<QuickMemoryEstimate> {
    ensure!(input.vertex_stride_bytes > 0 && input.vertex_stride_bytes % 4 == 0, "快速资源估算需要实际对齐的顶点 ABI");
    ensure!((1..=16).contains(&input.surface_buffer_count), "快速资源估算需要实际 DX12 交换链缓冲数量");
    let geometry_bytes = sum(&[product(input.vertices_upper_bound, input.vertex_stride_bytes)?, product(input.indices_upper_bound, std::mem::size_of::<u32>() as u64)?])?;
    ensure!(geometry_bytes == 0 || input.geometry_buffer_count_upper_bound > 0, "快速几何需要实际分块缓冲数量上界");
    let geometry_alignment_padding_bytes = product(input.geometry_buffer_count_upper_bound, RESOURCE_ALIGNMENT - 1)?;
    let atlas_resident_bytes = texture_resident_upper(input.atlas_width, input.atlas_height)?;
    let presentation_resident_bytes = product(texture_resident_upper(input.viewport_width, input.viewport_height)?, input.surface_buffer_count as u64 + 1)?; // color buffers + one depth
    let shadow_resident_bytes = match input.shadow_resolution { Some(size) => texture_resident_upper(size, size)?, None => 0 };
    let uniform_resident_bytes = input.uniform_buffer_bytes.iter().try_fold(0u64, |total, size| sum(&[total, aligned(*size, RESOURCE_ALIGNMENT)?]))?;
    let gpu_resident_upper_bytes = sum(&[geometry_bytes, geometry_alignment_padding_bytes, atlas_resident_bytes, presentation_resident_bytes, shadow_resident_bytes, uniform_resident_bytes])?;
    // One full geometry initialization upload, RGBA atlas upload and optional frame readback.
    // This is a peak upper estimate, not a promise about the driver's heap accounting.
    let upload_staging_upper_bytes = sum(&[aligned(geometry_bytes, RESOURCE_ALIGNMENT)?, texture_staging_upper(input.atlas_width, input.atlas_height)?, texture_staging_upper(input.viewport_width, input.viewport_height)?])?;
    Ok(QuickMemoryEstimate { geometry_bytes, geometry_alignment_padding_bytes, atlas_resident_bytes, presentation_resident_bytes,
        shadow_resident_bytes, uniform_resident_bytes, gpu_resident_upper_bytes, upload_staging_upper_bytes,
        gpu_peak_upper_bytes:sum(&[gpu_resident_upper_bytes, upload_staging_upper_bytes])?, cpu_geometry_upper_bytes:geometry_bytes })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickMemoryPressure { BelowWarning, Warning80, Stop95 }

/// `estimate` is incremental startup/pending work, not already-resident allocations.
/// Runtime callers must not add the full startup estimate to live usage a second time.
pub fn classify_quick_pressure(budget: &QuickGpuBudget, estimate: &QuickMemoryEstimate) -> Result<QuickMemoryPressure> {
    ensure!(budget.budget_bytes > 0, "实时预算未知，不能标记为低警线");
    let projected = sum(&[budget.current_usage_bytes, estimate.gpu_peak_upper_bytes])? as u128;
    let budget = budget.budget_bytes as u128;
    Ok(if projected * 100 >= budget * 95 { QuickMemoryPressure::Stop95 }
       else if projected * 100 >= budget * 80 { QuickMemoryPressure::Warning80 }
       else { QuickMemoryPressure::BelowWarning })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> QuickResourceInput { QuickResourceInput { vertices_upper_bound:36, vertex_stride_bytes:56, indices_upper_bound:36, geometry_buffer_count_upper_bound:3,
        atlas_width:16, atlas_height:16, viewport_width:640, viewport_height:480, surface_buffer_count:3, shadow_resolution:Some(1024), uniform_buffer_bytes:vec![64,64,64] } }
    #[test]
    fn fast_actual_vertex_index_texture_and_staging_contract() {
        let estimate=estimate_quick_resources(&input()).unwrap();
        assert_eq!(estimate.geometry_bytes,36*56+36*4);
        assert_eq!(estimate.geometry_alignment_padding_bytes,3*65_535);
        assert_eq!(estimate.atlas_resident_bytes,256*256*4);
        assert_eq!(estimate.presentation_resident_bytes,768*512*4*4);
        assert_eq!(estimate.shadow_resident_bytes,1024*1024*4);
        assert_eq!(estimate.uniform_resident_bytes,3*65_536);
        assert_eq!(estimate.cpu_geometry_upper_bytes,estimate.geometry_bytes);
        assert_eq!(estimate.gpu_peak_upper_bytes,estimate.gpu_resident_upper_bytes+estimate.upload_staging_upper_bytes);
    }
    #[test]
    fn metadata_bounds_do_not_need_or_allocate_a_mesh() {
        let mut input=input();input.vertices_upper_bound=88_534*36;input.indices_upper_bound=88_534*36;
        let estimate=estimate_quick_resources(&input).unwrap();
        assert_eq!(estimate.geometry_bytes,88_534*36*(56+4));
        input.vertices_upper_bound=u64::MAX;assert!(estimate_quick_resources(&input).is_err());
        input.vertices_upper_bound=1;input.atlas_width=0;assert!(estimate_quick_resources(&input).is_err());
    }
    #[test]
    fn real_budget_usage_not_vendor_capacity_drives_80_95_boundaries() {
        let mut budget=QuickGpuBudget{adapter_name:"fixture".into(),budget_bytes:1000,current_usage_bytes:200,available_for_reservation_bytes:0,current_reservation_bytes:0,safe_available_bytes:800};
        let mut estimate=estimate_quick_resources(&input()).unwrap();
        estimate.gpu_peak_upper_bytes=599;assert_eq!(classify_quick_pressure(&budget,&estimate).unwrap(),QuickMemoryPressure::BelowWarning);
        estimate.gpu_peak_upper_bytes=600;assert_eq!(classify_quick_pressure(&budget,&estimate).unwrap(),QuickMemoryPressure::Warning80);
        estimate.gpu_peak_upper_bytes=750;assert_eq!(classify_quick_pressure(&budget,&estimate).unwrap(),QuickMemoryPressure::Stop95);
        budget.budget_bytes=0;assert!(classify_quick_pressure(&budget,&estimate).is_err());
    }
}
