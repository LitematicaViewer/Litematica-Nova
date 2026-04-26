#![allow(
    clippy::too_many_arguments,
    clippy::result_large_err,
    clippy::while_let_loop,
    clippy::question_mark,
    clippy::format_in_format_args,
    clippy::explicit_counter_loop
)]

use std::collections::HashMap;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::f32::consts::FRAC_PI_4;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec3};
use image::{Rgb, RgbImage, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use wgpu::util::DeviceExt;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
#[cfg(target_os = "windows")]
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::WindowBuilder;

use crate::build_mode::{
    compact_cache_v2_enabled, headless_prebuild_enabled, writer_pipeline_enabled,
};
use crate::full_mode::FullModeMaterialCache;
use crate::full_mode_v2::build_full_mode_v2_scene_assets;
use crate::mesh::{ChunkKey, ChunkSceneIndex};
use crate::model::{CompactSurfaceOutput, MeshChunkOutput, MetadataOutput, TexturedVertexOutput};

const VIEWER_SHADER: &str = r#"
struct CameraUniform {
    view_proj: mat4x4<f32>,
    camera_position: vec4<f32>,
};

struct LightingUniform {
    enabled: f32,
    ambient_strength: f32,
    directional_strength: f32,
    min_brightness_floor: f32,
    light_direction: vec4<f32>,
    light_view_proj: mat4x4<f32>,
    shadow_enabled: f32,
    shadow_strength: f32,
    shadow_bias: f32,
    shadow_debug_view_mode: f32,
    shadow_force_test: f32,
    shadow_lighting_preset: f32,
    shadow_pcf_samples: f32,
    shadow_pcf_radius: f32,
    contact_shadow_enabled: f32,
    contact_shadow_strength: f32,
    ssao_enabled: f32,
    ssao_radius: f32,
    ssao_strength: f32,
    ao_debug_view_mode: f32,
    ao_sample_count: f32,
    ao_distance_fade_start: f32,
    ao_distance_fade_end: f32,
    ao_edge_guard: f32,
    ao_max_occlusion: f32,
    ao_padding: f32,
    tone_preset: f32,
    tone_exposure: f32,
    tone_gamma: f32,
    tone_saturation: f32,
    tone_contrast: f32,
    tone_highlight_rolloff: f32,
    tone_debug_view_mode: f32,
    tone_padding: f32,
    emissive_enabled: f32,
    emissive_strength: f32,
    bloom_enabled: f32,
    bloom_threshold: f32,
    bloom_intensity: f32,
    bloom_radius: f32,
    bloom_debug_view_mode: f32,
    emissive_debug_view_mode: f32,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

@group(1) @binding(0)
var atlas_texture: texture_2d<f32>;

@group(1) @binding(1)
var atlas_sampler: sampler;

@group(2) @binding(0)
var<uniform> lighting: LightingUniform;

@group(3) @binding(0)
var shadow_texture: texture_depth_2d;

@group(3) @binding(1)
var shadow_sampler: sampler_comparison;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) use_texture: f32,
    @location(4) normal: vec3<f32>,
    @location(5) emissive_tag: f32,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) use_texture: f32,
    @location(3) normal: vec3<f32>,
    @location(4) shadow_position: vec4<f32>,
    @location(5) world_position: vec3<f32>,
    @location(6) emissive_tag: f32,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = camera.view_proj * vec4<f32>(input.position, 1.0);
    output.color = input.color;
    output.uv = input.uv;
    output.use_texture = input.use_texture;
    output.normal = input.normal;
    output.shadow_position = lighting.light_view_proj * vec4<f32>(input.position, 1.0);
    output.world_position = input.position;
    output.emissive_tag = input.emissive_tag;
    return output;
}

fn apply_lba_tone_pipeline(color: vec3<f32>) -> vec3<f32> {
    if (lighting.tone_preset < 0.5) {
        return clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    let exposure_color = max(color * lighting.tone_exposure, vec3<f32>(0.0));
    let rolloff = clamp(lighting.tone_highlight_rolloff, 0.0, 1.0);
    var toned = exposure_color / (vec3<f32>(1.0) + exposure_color * rolloff);
    let luminance = dot(toned, vec3<f32>(0.2126, 0.7152, 0.0722));
    toned = mix(vec3<f32>(luminance), toned, lighting.tone_saturation);
    toned = (toned - vec3<f32>(0.5)) * lighting.tone_contrast + vec3<f32>(0.5);
    let gamma = max(lighting.tone_gamma, 0.01);
    toned = pow(max(toned, vec3<f32>(0.0)), vec3<f32>(1.0 / gamma));
    return clamp(toned, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn lba_luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    var color = input.color;
    var sampled_base_rgb = color.rgb;
    if (input.emissive_tag < -0.5) {
        let debug_id = max(-input.emissive_tag - 1.0, 0.0);
        let stripe = fract(debug_id * 0.173);
        sampled_base_rgb = vec3<f32>(
            0.18 + 0.72 * stripe,
            0.95,
            0.18 + 0.45 * (1.0 - stripe),
        );
        color = vec4<f32>(sampled_base_rgb, 1.0);
    } else if (input.use_texture > 0.5) {
        let sampled = textureSample(atlas_texture, atlas_sampler, input.uv);
        if (sampled.a <= 0.01) {
            discard;
        }
        color = vec4<f32>(sampled.rgb, sampled.a * input.color.a);
        sampled_base_rgb = sampled.rgb;
    }
    if (lighting.enabled > 0.5) {
        let normal = normalize(input.normal);
        let light_direction = normalize(lighting.light_direction.xyz);
        let diffuse = max(dot(normal, light_direction), 0.0);
        let shadow_ndc = input.shadow_position.xyz / input.shadow_position.w;
        let shadow_uv = vec2<f32>(
            shadow_ndc.x * 0.5 + 0.5,
            0.5 - shadow_ndc.y * 0.5,
        );
        let shadow_valid =
            input.shadow_position.w > 0.000001 &&
            shadow_ndc.x == shadow_ndc.x &&
            shadow_ndc.y == shadow_ndc.y &&
            shadow_ndc.z == shadow_ndc.z;
        let behind_or_depth_out = shadow_valid && (shadow_ndc.z < 0.0 || shadow_ndc.z > 1.0);
        let inside_light =
            shadow_valid &&
            all(shadow_uv >= vec2<f32>(0.0, 0.0)) &&
            all(shadow_uv <= vec2<f32>(1.0, 1.0)) &&
            shadow_ndc.z >= 0.0 &&
            shadow_ndc.z <= 1.0;
        let shadow_size_i = textureDimensions(shadow_texture);
        let shadow_size = vec2<f32>(shadow_size_i);
        var sampled_shadow_depth = 1.0;
        if (inside_light) {
            let shadow_texel = vec2<i32>(clamp(
                shadow_uv * shadow_size,
                vec2<f32>(0.0, 0.0),
                shadow_size - vec2<f32>(1.0, 1.0),
            ));
            sampled_shadow_depth = textureLoad(shadow_texture, shadow_texel, 0);
        }
        var shadow_visibility = 1.0;
        var raw_shadow_visibility = 1.0;
        if (lighting.shadow_enabled > 0.5) {
            if (lighting.shadow_force_test > 0.5) {
                let checker = floor(shadow_uv.x * 18.0) + floor(shadow_uv.y * 18.0);
                let lit = fract(checker * 0.5) * 2.0;
                shadow_visibility = mix(1.0 - lighting.shadow_strength, 1.0, lit);
                raw_shadow_visibility = shadow_visibility;
            } else if (diffuse > 0.001 && inside_light) {
                let compare_depth = shadow_ndc.z - lighting.shadow_bias;
                let raw_lit = select(0.0, 1.0, compare_depth <= sampled_shadow_depth);
                raw_shadow_visibility = mix(1.0 - lighting.shadow_strength, 1.0, raw_lit);
                var lit = raw_lit;
                if (lighting.shadow_pcf_samples > 0.5) {
                    var lit_sum = 0.0;
                    var sample_count = 0.0;
                    for (var i: i32 = 0; i < 12; i = i + 1) {
                        if (f32(i) < lighting.shadow_pcf_samples) {
                            var offset = vec2<f32>(-0.326, -0.406);
                            switch i {
                                case 1: { offset = vec2<f32>(-0.840, -0.074); }
                                case 2: { offset = vec2<f32>(-0.696, 0.457); }
                                case 3: { offset = vec2<f32>(-0.203, 0.621); }
                                case 4: { offset = vec2<f32>(0.962, -0.195); }
                                case 5: { offset = vec2<f32>(0.473, -0.480); }
                                case 6: { offset = vec2<f32>(0.519, 0.767); }
                                case 7: { offset = vec2<f32>(0.185, -0.893); }
                                case 8: { offset = vec2<f32>(0.507, 0.064); }
                                case 9: { offset = vec2<f32>(0.896, 0.412); }
                                case 10: { offset = vec2<f32>(-0.322, -0.933); }
                                case 11: { offset = vec2<f32>(-0.792, -0.598); }
                                default: {}
                            }
                            let sample_coord = shadow_uv * shadow_size + offset * lighting.shadow_pcf_radius;
                            let sample_texel = vec2<i32>(clamp(
                                sample_coord,
                                vec2<f32>(0.0, 0.0),
                                shadow_size - vec2<f32>(1.0, 1.0),
                            ));
                            let depth = textureLoad(shadow_texture, sample_texel, 0);
                            lit_sum = lit_sum + select(0.0, 1.0, compare_depth <= depth);
                            sample_count = sample_count + 1.0;
                        }
                    }
                    lit = lit_sum / max(sample_count, 1.0);
                }
                shadow_visibility = mix(1.0 - lighting.shadow_strength, 1.0, lit);
            }
        }
        var light_factor = clamp(
            max(
                lighting.min_brightness_floor,
                lighting.ambient_strength + diffuse * lighting.directional_strength
            ),
            0.0,
            1.0,
        );
        if (lighting.shadow_enabled > 0.5) {
            let preset = lighting.shadow_lighting_preset;
            var hemisphere_ground = 0.54;
            var hemisphere_sky = 0.72;
            var top_lift_strength = 0.055;
            var bottom_shade_strength = 0.070;
            var side_layer_x = 0.025;
            var side_layer_z = -0.018;
            var direct_strength = 0.18;
            var unshadowed_min = 0.44;
            var unshadowed_max = 0.96;
            var shadow_floor = 0.58;
            var final_min = 0.38;
            var final_max = 0.90;
            if (preset > 1.5) {
                hemisphere_ground = 0.50;
                hemisphere_sky = 0.72;
                top_lift_strength = 0.060;
                bottom_shade_strength = 0.090;
                side_layer_x = 0.032;
                side_layer_z = -0.024;
                direct_strength = 0.22;
                unshadowed_min = 0.40;
                unshadowed_max = 0.98;
                shadow_floor = 0.48;
                final_min = 0.34;
                final_max = 0.90;
            } else if (preset > 0.5) {
                hemisphere_ground = 0.54;
                hemisphere_sky = 0.75;
                top_lift_strength = 0.052;
                bottom_shade_strength = 0.062;
                side_layer_x = 0.020;
                side_layer_z = -0.014;
                direct_strength = 0.14;
                unshadowed_min = 0.46;
                unshadowed_max = 0.94;
                shadow_floor = 0.62;
                final_min = 0.36;
                final_max = 0.77;
            }
            let sky_hemisphere = clamp(normal.y * 0.5 + 0.5, 0.0, 1.0);
            let side_amount = 1.0 - abs(normal.y);
            let hemisphere_fill = mix(hemisphere_ground, hemisphere_sky, sky_hemisphere);
            let minecraft_top_lift = top_lift_strength * max(normal.y, 0.0);
            let minecraft_bottom_shade = bottom_shade_strength * max(-normal.y, 0.0);
            let minecraft_side_layer = side_amount * (side_layer_x * normal.x + side_layer_z * normal.z);
            let softened_direct = diffuse * direct_strength;
            let unshadowed_viewer_light = clamp(
                hemisphere_fill + softened_direct + minecraft_top_lift - minecraft_bottom_shade + minecraft_side_layer,
                unshadowed_min,
                unshadowed_max,
            );
            let softened_shadow = mix(shadow_floor, 1.0, shadow_visibility);
            light_factor = clamp(unshadowed_viewer_light * softened_shadow, final_min, final_max);
        }
        if (lighting.shadow_debug_view_mode > 0.5 && lighting.shadow_debug_view_mode < 1.5) {
            return vec4<f32>(vec3<f32>(shadow_visibility), color.a);
        }
        if (lighting.shadow_debug_view_mode > 1.5 && lighting.shadow_debug_view_mode < 2.5) {
            if (!shadow_valid) {
                return vec4<f32>(1.0, 0.0, 1.0, color.a);
            }
            if (behind_or_depth_out) {
                return vec4<f32>(0.0, 0.25, 1.0, color.a);
            }
            if (inside_light) {
                return vec4<f32>(0.0, 1.0, 0.0, color.a);
            }
            return vec4<f32>(1.0, 0.0, 0.0, color.a);
        }
        if (lighting.shadow_debug_view_mode > 2.5 && lighting.shadow_debug_view_mode < 3.5) {
            return vec4<f32>(vec3<f32>(select(0.0, clamp(shadow_ndc.z, 0.0, 1.0), inside_light)), color.a);
        }
        if (lighting.shadow_debug_view_mode > 3.5 && lighting.shadow_debug_view_mode < 4.5) {
            return vec4<f32>(vec3<f32>(select(0.0, sampled_shadow_depth, inside_light)), color.a);
        }
        if (lighting.shadow_debug_view_mode > 4.5 && lighting.shadow_debug_view_mode < 5.5) {
            return vec4<f32>(vec3<f32>(raw_shadow_visibility), color.a);
        }
        if (lighting.shadow_debug_view_mode > 5.5 && lighting.shadow_debug_view_mode < 6.5) {
            return vec4<f32>(vec3<f32>(shadow_visibility), color.a);
        }
        var contact_ao = 1.0;
        var ssao_ao = 1.0;
        var combined_ao = 1.0;
        let ao_requested =
            lighting.contact_shadow_enabled > 0.5 ||
            lighting.ssao_enabled > 0.5 ||
            lighting.ao_debug_view_mode > 0.5;
        if (ao_requested) {
            let radius = clamp(lighting.ssao_radius, 0.1, 3.0);
            let view_distance = length(input.world_position - camera.camera_position.xyz);
            let fade_start = min(lighting.ao_distance_fade_start, lighting.ao_distance_fade_end - 0.001);
            let fade_end = max(lighting.ao_distance_fade_end, fade_start + 0.001);
            let distance_fade = 1.0 - smoothstep(fade_start, fade_end, view_distance);
            let radius_scale = clamp(fade_start / max(view_distance, 0.001), 0.22, 1.0);
            let depth_edge = clamp(fwidth(input.position.z) * 420.0 * radius * radius_scale, 0.0, 1.0);
            let normal_edge = clamp((length(dpdx(normal)) + length(dpdy(normal))) * 0.85, 0.0, 1.0);
            let edge_guard = clamp(lighting.ao_edge_guard, 0.0, 1.0);
            let edge_protection = 1.0 - smoothstep(edge_guard, min(edge_guard + 0.28, 1.0), depth_edge);
            let top_protection = 1.0 - clamp(max(normal.y, 0.0) * 0.22, 0.0, 0.22);
            let ao_gate = distance_fade * edge_protection;
            let contact_seed = clamp(depth_edge * (0.52 + normal_edge * 0.18) * top_protection * ao_gate, 0.0, 0.70);
            let ssao_seed = clamp((depth_edge * 0.24 + normal_edge * 0.24 + (1.0 - shadow_visibility) * 0.08) * ao_gate, 0.0, 0.50);
            let max_occlusion = clamp(lighting.ao_max_occlusion, 0.0, 0.8);
            contact_ao = select(
                1.0,
                clamp(1.0 - min(contact_seed * lighting.contact_shadow_strength, max_occlusion), 0.0, 1.0),
                lighting.contact_shadow_enabled > 0.5,
            );
            ssao_ao = select(
                1.0,
                clamp(1.0 - min(ssao_seed * lighting.ssao_strength, max_occlusion), 0.0, 1.0),
                lighting.ssao_enabled > 0.5,
            );
            combined_ao = max(clamp(contact_ao * ssao_ao, 0.0, 1.0), 1.0 - max_occlusion);
        }
        if (lighting.ao_debug_view_mode > 0.5 && lighting.ao_debug_view_mode < 1.5) {
            return vec4<f32>(vec3<f32>(contact_ao), color.a);
        }
        if (lighting.ao_debug_view_mode > 1.5 && lighting.ao_debug_view_mode < 2.5) {
            return vec4<f32>(vec3<f32>(ssao_ao), color.a);
        }
        if (lighting.ao_debug_view_mode > 2.5 && lighting.ao_debug_view_mode < 3.5) {
            return vec4<f32>(vec3<f32>(combined_ao), color.a);
        }
        let final_effects_enabled = lighting.shadow_enabled > 0.5;
        let lit_color = color.rgb * light_factor * combined_ao;
        let emissive_tag_mask = clamp(input.emissive_tag, 0.0, 1.0);
        let emissive_mask = select(
            0.0,
            emissive_tag_mask,
            final_effects_enabled && lighting.emissive_enabled > 0.5 && input.use_texture > 0.5,
        );
        let emissive_color = sampled_base_rgb * emissive_mask * max(lighting.emissive_strength - 1.0, 0.0);
        var pre_tone_color = lit_color + emissive_color;
        let bloom_radius = clamp(lighting.bloom_radius, 0.25, 4.0);
        let bloom_mask = select(
            0.0,
            emissive_mask,
            final_effects_enabled && lighting.bloom_enabled > 0.5,
        );
        let bloom_color = pre_tone_color * bloom_mask * lighting.bloom_intensity * (0.65 + bloom_radius * 0.12);
        pre_tone_color = pre_tone_color + bloom_color;
        var post_tone_color = select(
            clamp(pre_tone_color, vec3<f32>(0.0), vec3<f32>(1.0)),
            apply_lba_tone_pipeline(pre_tone_color),
            lighting.shadow_enabled > 0.5,
        );
        if (lighting.emissive_debug_view_mode > 0.5 && lighting.emissive_debug_view_mode < 1.5) {
            return vec4<f32>(vec3<f32>(emissive_mask), color.a);
        }
        if (lighting.emissive_debug_view_mode > 1.5 && lighting.emissive_debug_view_mode < 2.5) {
            return vec4<f32>(vec3<f32>(0.0, emissive_tag_mask, 0.0), color.a);
        }
        if (lighting.emissive_debug_view_mode > 2.5 && lighting.emissive_debug_view_mode < 3.5) {
            return vec4<f32>(post_tone_color, color.a);
        }
        if (lighting.bloom_debug_view_mode > 0.5 && lighting.bloom_debug_view_mode < 1.5) {
            return vec4<f32>(vec3<f32>(max(emissive_mask, bloom_mask)), color.a);
        }
        if (lighting.bloom_debug_view_mode > 1.5 && lighting.bloom_debug_view_mode < 2.5) {
            return vec4<f32>(clamp(bloom_color, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
        }
        if (lighting.bloom_debug_view_mode > 2.5 && lighting.bloom_debug_view_mode < 3.5) {
            return vec4<f32>(post_tone_color, color.a);
        }
        if (lighting.tone_debug_view_mode > 0.5 && lighting.tone_debug_view_mode < 1.5) {
            return vec4<f32>(clamp(pre_tone_color, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
        }
        if (lighting.tone_debug_view_mode > 1.5 && lighting.tone_debug_view_mode < 2.5) {
            return vec4<f32>(post_tone_color, color.a);
        }
        if (lighting.tone_debug_view_mode > 2.5 && lighting.tone_debug_view_mode < 3.5) {
            let tone_luma = dot(post_tone_color, vec3<f32>(0.2126, 0.7152, 0.0722));
            return vec4<f32>(vec3<f32>(tone_luma), color.a);
        }
        color = vec4<f32>(post_tone_color, color.a);
    }
    return color;
}
"#;

const SHADOW_DEPTH_SHADER: &str = r#"
struct CameraUniform {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) use_texture: f32,
    @location(4) normal: vec3<f32>,
    @location(5) emissive_tag: f32,
};

@vertex
fn vs_main(input: VertexInput) -> @builtin(position) vec4<f32> {
    return camera.view_proj * vec4<f32>(input.position, 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct GpuVertex {
    position: [f32; 3],
    color: [f32; 4],
    uv: [f32; 2],
    use_texture: f32,
    normal: [f32; 3],
    emissive_tag: f32,
}

impl GpuVertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GpuVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 3]>() + std::mem::size_of::<[f32; 4]>())
                        as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 3]>()
                        + std::mem::size_of::<[f32; 4]>()
                        + std::mem::size_of::<[f32; 2]>())
                        as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 3]>()
                        + std::mem::size_of::<[f32; 4]>()
                        + std::mem::size_of::<[f32; 2]>()
                        + std::mem::size_of::<f32>())
                        as wgpu::BufferAddress,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 3]>()
                        + std::mem::size_of::<[f32; 4]>()
                        + std::mem::size_of::<[f32; 2]>()
                        + std::mem::size_of::<f32>()
                        + std::mem::size_of::<[f32; 3]>())
                        as wgpu::BufferAddress,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        }
    }
}

fn default_vertex_normal() -> [f32; 3] {
    [0.0, 1.0, 0.0]
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    camera_position: [f32; 4],
}

impl CameraUniform {
    fn from_matrix_and_eye(matrix: Mat4, eye: Vec3) -> Self {
        Self {
            view_proj: matrix.to_cols_array_2d(),
            camera_position: [eye.x, eye.y, eye.z, 0.0],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct LightingUniform {
    enabled: f32,
    ambient_strength: f32,
    directional_strength: f32,
    min_brightness_floor: f32,
    light_direction: [f32; 4],
    light_view_proj: [[f32; 4]; 4],
    shadow_enabled: f32,
    shadow_strength: f32,
    shadow_bias: f32,
    shadow_debug_view_mode: f32,
    shadow_force_test: f32,
    shadow_lighting_preset: f32,
    shadow_pcf_samples: f32,
    shadow_pcf_radius: f32,
    contact_shadow_enabled: f32,
    contact_shadow_strength: f32,
    ssao_enabled: f32,
    ssao_radius: f32,
    ssao_strength: f32,
    ao_debug_view_mode: f32,
    ao_sample_count: f32,
    ao_distance_fade_start: f32,
    ao_distance_fade_end: f32,
    ao_edge_guard: f32,
    ao_max_occlusion: f32,
    ao_padding: f32,
    tone_preset: f32,
    tone_exposure: f32,
    tone_gamma: f32,
    tone_saturation: f32,
    tone_contrast: f32,
    tone_highlight_rolloff: f32,
    tone_debug_view_mode: f32,
    tone_padding: f32,
    emissive_enabled: f32,
    emissive_strength: f32,
    bloom_enabled: f32,
    bloom_threshold: f32,
    bloom_intensity: f32,
    bloom_radius: f32,
    bloom_debug_view_mode: f32,
    emissive_debug_view_mode: f32,
}

impl LightingUniform {
    fn from_config(config: LightingConfig) -> Self {
        let basic = config.basic();
        let shadow = config.shadow();
        let ao = config.ao();
        let tone = config.tone();
        let emissive = config.emissive_bloom();
        let light_view_proj = shadow
            .light_camera
            .map(|camera| camera.view_proj)
            .unwrap_or(Mat4::IDENTITY)
            .to_cols_array_2d();
        Self {
            enabled: if basic.enabled { 1.0 } else { 0.0 },
            ambient_strength: basic.tuning.ambient_strength,
            directional_strength: basic.tuning.directional_strength,
            min_brightness_floor: basic.tuning.min_brightness_floor,
            light_direction: [
                basic.tuning.light_direction[0],
                basic.tuning.light_direction[1],
                basic.tuning.light_direction[2],
                0.0,
            ],
            light_view_proj,
            shadow_enabled: if shadow.enabled { 1.0 } else { 0.0 },
            shadow_strength: shadow.strength,
            shadow_bias: shadow.bias,
            shadow_debug_view_mode: shadow.debug_view.shader_code(),
            shadow_force_test: if shadow.force_test { 1.0 } else { 0.0 },
            shadow_lighting_preset: shadow.lighting_preset.shader_code(),
            shadow_pcf_samples: shadow.pcf.samples as f32,
            shadow_pcf_radius: shadow.pcf.radius,
            contact_shadow_enabled: if ao.contact_enabled { 1.0 } else { 0.0 },
            contact_shadow_strength: ao.contact_strength,
            ssao_enabled: if ao.ssao_enabled { 1.0 } else { 0.0 },
            ssao_radius: ao.ssao_radius,
            ssao_strength: ao.ssao_strength,
            ao_debug_view_mode: ao.debug_view.shader_code(),
            ao_sample_count: AoConfig::SAMPLE_COUNT as f32,
            ao_distance_fade_start: ao.distance_fade_start,
            ao_distance_fade_end: ao.distance_fade_end,
            ao_edge_guard: ao.edge_guard,
            ao_max_occlusion: ao.max_occlusion,
            ao_padding: 0.0,
            tone_preset: tone.preset.shader_code(),
            tone_exposure: tone.exposure,
            tone_gamma: tone.gamma,
            tone_saturation: tone.saturation,
            tone_contrast: tone.contrast,
            tone_highlight_rolloff: tone.highlight_rolloff,
            tone_debug_view_mode: tone.debug_view.shader_code(),
            tone_padding: 0.0,
            emissive_enabled: if emissive.emissive_enabled { 1.0 } else { 0.0 },
            emissive_strength: emissive.emissive_strength,
            bloom_enabled: if emissive.bloom_enabled { 1.0 } else { 0.0 },
            bloom_threshold: emissive.bloom_threshold,
            bloom_intensity: emissive.bloom_intensity,
            bloom_radius: emissive.bloom_radius,
            bloom_debug_view_mode: emissive.bloom_debug_view.shader_code(),
            emissive_debug_view_mode: emissive.emissive_debug_view.shader_code(),
        }
    }
}

struct ViewerArgs {
    input: Option<PathBuf>,
    chunk_size: u32,
    probe_scene: bool,
    prebuild_only: bool,
    auto_exit_seconds: Option<u64>,
    prebuild_before_show: bool,
    cache_input: Option<PathBuf>,
    ready_file: Option<PathBuf>,
    cache_file: Option<PathBuf>,
    preview_output: Option<PathBuf>,
    build_workers: usize,
    cpu_cache_max_chunks: usize,
    cpu_cache_max_bytes: usize,
    resident_max_chunks: usize,
    resident_max_bytes: usize,
    stress_limits: bool,
    embed_parent_hwnd: Option<isize>,
    preview_mode: bool,
    preview_spin: bool,
    display_mode: ViewerDisplayMode,
    basic_lighting: bool,
    basic_shadows: bool,
    shadow_debug: bool,
    shadow_debug_view: ShadowDebugViewMode,
    shadow_force_test: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewerDisplayMode {
    Standard,
    FastExperimental,
    Full,
}

impl ViewerDisplayMode {
    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "full" | "complete" => Self::Full,
            "fast" | "fast_experimental" | "experimental" => Self::FastExperimental,
            _ => Self::Standard,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::FastExperimental => "fast_experimental",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct EmbeddedViewportInfo {
    parent_hwnd: isize,
    client_size: PhysicalSize<u32>,
}

#[derive(Debug, Clone, Copy)]
struct CacheBudgetConfig {
    max_chunks: usize,
    max_bytes: usize,
}

impl CacheBudgetConfig {
    const fn new(max_chunks: usize, max_bytes: usize) -> Self {
        Self {
            max_chunks,
            max_bytes,
        }
    }
}

const DEFAULT_CPU_CACHE_BUDGET: CacheBudgetConfig =
    CacheBudgetConfig::new(8192, 1024 * 1024 * 1024);
const DEFAULT_RESIDENT_BUDGET: CacheBudgetConfig = CacheBudgetConfig::new(4096, 768 * 1024 * 1024);
const STRESS_CPU_CACHE_BUDGET: CacheBudgetConfig = CacheBudgetConfig::new(128, 32 * 1024 * 1024);
const STRESS_RESIDENT_BUDGET: CacheBudgetConfig = CacheBudgetConfig::new(48, 12 * 1024 * 1024);
const INITIAL_BOOTSTRAP_CHUNKS: usize = 48;
const PREBUILD_BUILD_BATCH_SIZE: usize = 32;
const PREBUILD_TAIL_LOG_INTERVAL: Duration = Duration::from_secs(2);
const PREBUILD_TAIL_STALL_SECONDS: u64 = 15;
const DEFAULT_BLOCK_COLOR: [f32; 3] = [0.6, 0.6, 0.6];
const FACE_VERTICES_RUNTIME: [[[f32; 3]; 4]; 6] = [
    [
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [0.0, 1.0, 0.0],
    ],
    [
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
    ],
    [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
    ],
    [
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        [1.0, 1.0, 0.0],
    ],
    [
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
    ],
    [
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ],
];
const NATIVE_CACHE_CHUNK_BINARY_MAGIC_V2: &[u8; 8] = b"LPNC2\0\0\0";
const NATIVE_CACHE_CHUNK_BINARY_MAGIC_V3: &[u8; 8] = b"LPNC3\0\0\0";
const NATIVE_CACHE_CHUNK_BINARY_MAGIC_V4: &[u8; 8] = b"LPNC4\0\0\0";
static BLOCK_COLOR_CACHE: OnceLock<HashMap<String, [f32; 3]>> = OnceLock::new();

#[derive(Debug, Default)]
struct TraceCounters {
    worker_busy_ms: AtomicU64,
    worker_wait_ms: AtomicU64,
    worker_batches: AtomicUsize,
    worker_chunks: AtomicUsize,
    worker_result_send_ms: AtomicU64,
    writer_busy_ms: AtomicU64,
    writer_wait_ms: AtomicU64,
    writer_chunks: AtomicUsize,
    writer_errors: AtomicUsize,
    cache_file_create_ms: AtomicU64,
    build_enqueue_ms: AtomicU64,
    build_enqueue_batches: AtomicUsize,
    build_enqueue_chunks: AtomicUsize,
    build_dequeue_ms: AtomicU64,
    build_dequeue_batches: AtomicUsize,
    build_dequeue_chunks: AtomicUsize,
    max_pending_build_queue: AtomicUsize,
    max_in_flight_builds: AtomicUsize,
    max_pending_upload_queue: AtomicUsize,
}

impl TraceCounters {
    fn add_ms(target: &AtomicU64, duration: Duration) {
        target.fetch_add(duration.as_millis() as u64, Ordering::Relaxed);
    }

    fn update_max(target: &AtomicUsize, value: usize) {
        let mut current = target.load(Ordering::Relaxed);
        while value > current {
            match target.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(next) => current = next,
            }
        }
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" {
    fn SetParent(hwnd_child: isize, hwnd_new_parent: isize) -> isize;
    fn ShowWindow(hwnd: isize, n_cmd_show: i32) -> i32;
    fn SetWindowLongPtrW(hwnd: isize, n_index: i32, dw_new_long: isize) -> isize;
    fn GetWindowLongPtrW(hwnd: isize, n_index: i32) -> isize;
    fn GetClientRect(hwnd: isize, lp_rect: *mut WinRect) -> i32;
    fn SetWindowPos(
        hwnd: isize,
        hwnd_insert_after: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
}

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct WinRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[derive(Debug, Clone)]
struct ResolvedBlockColor {
    rgb: [f32; 3],
    normalized_key: String,
    matched_key: Option<String>,
    used_default: bool,
}

fn parse_byte_size(text: &str) -> Result<usize> {
    let normalized = text.trim().to_ascii_lowercase();
    let split_index = normalized
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(normalized.len());
    let number_part = normalized[..split_index].trim();
    let suffix = normalized[split_index..].trim();
    let value = number_part.parse::<usize>()?;
    let multiplier = match suffix {
        "" | "b" => 1_usize,
        "k" | "kb" | "kib" => 1024_usize,
        "m" | "mb" | "mib" => 1024_usize * 1024,
        "g" | "gb" | "gib" => 1024_usize * 1024 * 1024,
        _ => bail!("unsupported byte-size suffix: {suffix}"),
    };
    Ok(value.saturating_mul(multiplier))
}

#[derive(Debug, Clone, Copy)]
struct SceneBounds {
    center: Vec3,
    radius: f32,
}

impl SceneBounds {
    fn from_metadata(metadata: &MetadataOutput) -> Self {
        let size_x = metadata.enclosing_size.x.max(1) as f32;
        let size_y = metadata.enclosing_size.y.max(1) as f32;
        let size_z = metadata.enclosing_size.z.max(1) as f32;
        let center = Vec3::new(size_x * 0.5, size_y * 0.5, size_z * 0.5);
        let radius = Vec3::new(size_x, size_y, size_z).length().max(2.0) * 0.5;
        Self { center, radius }
    }

    fn from_meshes(meshes: &[PreparedChunkMesh]) -> Self {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        let mut has_vertex = false;

        for mesh in meshes {
            include_prepared_mesh_bounds(mesh, &mut min, &mut max, &mut has_vertex);
        }

        if !has_vertex {
            return Self {
                center: Vec3::ZERO,
                radius: 2.0,
            };
        }

        let center = (min + max) * 0.5;
        let radius = (max - min).length().max(2.0) * 0.5;
        Self { center, radius }
    }

    fn from_prepared_mesh(mesh: &PreparedChunkMesh) -> Self {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        let mut has_vertex = false;
        include_prepared_mesh_bounds(mesh, &mut min, &mut max, &mut has_vertex);
        if !has_vertex {
            return Self {
                center: Vec3::ZERO,
                radius: 2.0,
            };
        }
        let center = (min + max) * 0.5;
        let radius = (max - min).length().max(2.0) * 0.5;
        Self { center, radius }
    }
}

fn include_prepared_mesh_bounds(
    mesh: &PreparedChunkMesh,
    min: &mut Vec3,
    max: &mut Vec3,
    has_vertex: &mut bool,
) {
    for vertex in &mesh.vertices {
        let position = Vec3::from_array(vertex.position);
        *min = min.min(position);
        *max = max.max(position);
        *has_vertex = true;
    }
    for surface in &mesh.compact_surfaces {
        *min = min.min(Vec3::from_array(surface.min));
        *max = max.max(Vec3::from_array(surface.max));
        *has_vertex = true;
    }
}

fn rgb_to_rgba(color: [f32; 3]) -> [f32; 4] {
    [color[0], color[1], color[2], 1.0]
}

fn load_full_mode_scene_assets(
    chunk_scene_index: &ChunkSceneIndex,
    litematic_path: &Path,
) -> Result<(Option<Arc<FullModeMaterialCache>>, Option<RgbaImage>)> {
    let (materials, atlas) = build_full_mode_v2_scene_assets(chunk_scene_index, litematic_path)?;
    println!(
        "[LBA_FULL_MODE_V2] runtime_assets_loaded palette_entries={} material_slots={} baked_palette_entries={} fallback_palette_entries={}",
        materials.palette_materials.len(),
        materials.materials.len(),
        materials
            .stats
            .as_ref()
            .map(|stats| stats.baked_palette_entries)
            .unwrap_or(0),
        materials
            .stats
            .as_ref()
            .map(|stats| stats.fallback_palette_entries)
            .unwrap_or(0)
    );
    Ok((Some(Arc::new(materials)), Some(atlas)))
}

#[derive(Debug, Clone, Copy)]
struct ChunkBounds {
    min: Vec3,
    max: Vec3,
    center: Vec3,
    radius: f32,
}

impl ChunkBounds {
    fn from_key(key: ChunkKey, chunk_size: u32) -> Self {
        let chunk_size = chunk_size as f32;
        let min = Vec3::new(
            key.cx as f32 * chunk_size,
            key.cy as f32 * chunk_size,
            key.cz as f32 * chunk_size,
        );
        let max = min + Vec3::splat(chunk_size);
        let center = (min + max) * 0.5;
        let radius = (max - min).length() * 0.5;
        Self {
            min,
            max,
            center,
            radius,
        }
    }

    fn expanded(&self, margin: f32) -> Self {
        let half = Vec3::splat(margin.max(0.0));
        let min = self.min - half;
        let max = self.max + half;
        let center = (min + max) * 0.5;
        let radius = (max - min).length() * 0.5;
        Self {
            min,
            max,
            center,
            radius,
        }
    }

    fn positive_vertex(&self, normal: Vec3) -> Vec3 {
        Vec3::new(
            if normal.x >= 0.0 {
                self.max.x
            } else {
                self.min.x
            },
            if normal.y >= 0.0 {
                self.max.y
            } else {
                self.min.y
            },
            if normal.z >= 0.0 {
                self.max.z
            } else {
                self.min.z
            },
        )
    }
}

struct ViewerScene {
    label: String,
    bounds: SceneBounds,
    palette_colors: Vec<[f32; 3]>,
    chunk_scene_index: Option<ChunkSceneIndex>,
    bootstrap_meshes: Vec<PreparedChunkMesh>,
    bootstrap_visible_first: usize,
    full_mode_materials: Option<Arc<FullModeMaterialCache>>,
    texture_atlas: Option<RgbaImage>,
}

impl ViewerScene {
    fn placeholder() -> Self {
        let prepared = PreparedChunkMesh::placeholder();
        let bounds = SceneBounds::from_prepared_mesh(&prepared);
        Self {
            label: "Placeholder Cube".to_string(),
            bounds,
            palette_colors: Vec::new(),
            chunk_scene_index: None,
            bootstrap_meshes: vec![prepared],
            bootstrap_visible_first: 1,
            full_mode_materials: None,
            texture_atlas: None,
        }
    }

    fn from_litematic(
        path: &std::path::Path,
        chunk_size: u32,
        display_mode: ViewerDisplayMode,
    ) -> Result<Self> {
        println!(
            "[VIEWER_SCENE] loading_litematic file={} chunk_size={}",
            path.display(),
            chunk_size
        );
        let chunk_scene_index = ChunkSceneIndex::load(path, chunk_size)?;
        let palette_colors = resolve_scene_palette_colors(&chunk_scene_index, display_mode);
        let (full_mode_materials, texture_atlas) = if display_mode == ViewerDisplayMode::Full {
            load_full_mode_scene_assets(&chunk_scene_index, path)?
        } else {
            (None, None)
        };
        let bounds = SceneBounds::from_metadata(chunk_scene_index.metadata());
        println!(
            "[VIEWER_CHUNK] index_ready total_chunks={} chunk_size={} scene_radius={:.2}",
            chunk_scene_index.chunk_count(),
            chunk_size,
            bounds.radius
        );
        let bootstrap_camera = OrbitCamera::from_bounds(bounds, 1280, 720);
        let bootstrap_plan = ChunkResidencyPlan::build(
            &chunk_scene_index,
            &bootstrap_camera,
            &ChunkStreamingConfig::default(),
            0,
            false,
            false,
        );
        let bootstrap_keys = bootstrap_plan
            .upload_order
            .iter()
            .copied()
            .take(INITIAL_BOOTSTRAP_CHUNKS)
            .collect::<Vec<_>>();
        let bootstrap_meshes = if let Some(materials) = full_mode_materials.as_ref() {
            chunk_scene_index
                .build_textured_chunk_meshes(&bootstrap_keys, materials)?
                .into_iter()
                .filter_map(|chunk| {
                    PreparedChunkMesh::from_mesh_chunk(
                        chunk,
                        palette_colors.as_slice(),
                        chunk_scene_index.chunk_size(),
                    )
                })
                .collect::<Vec<_>>()
        } else {
            chunk_scene_index
                .build_chunk_meshes(&bootstrap_keys)?
                .into_iter()
                .filter_map(|chunk| {
                    PreparedChunkMesh::from_mesh_chunk(
                        chunk,
                        palette_colors.as_slice(),
                        chunk_scene_index.chunk_size(),
                    )
                })
                .collect::<Vec<_>>()
        };
        println!(
            "[VIEWER_BOOTSTRAP] bootstrap_chunks={} visible_first={} target_chunks={} retain_chunks={}",
            bootstrap_meshes.len(),
            bootstrap_keys.len(),
            bootstrap_plan.visible_target_chunks,
            bootstrap_plan.retain_chunks
        );
        Ok(Self {
            label: path
                .file_name()
                .and_then(|name| name.to_str())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| path.display().to_string()),
            bounds,
            palette_colors,
            chunk_scene_index: Some(chunk_scene_index),
            bootstrap_meshes,
            bootstrap_visible_first: bootstrap_keys.len(),
            full_mode_materials,
            texture_atlas,
        })
    }

    fn from_prebuild_cache(path: &Path, chunk_size: u32) -> Result<Self> {
        println!(
            "[VIEWER_CACHE] loading_prebuild_cache path={}",
            path.display()
        );
        let manifest_text = std::fs::read_to_string(path)
            .with_context(|| format!("read cache manifest failed: {}", path.display()))?;
        let manifest: NativePreviewCacheManifest = serde_json::from_str(&manifest_text)
            .with_context(|| format!("parse cache manifest failed: {}", path.display()))?;
        let chunk_dir = path
            .parent()
            .map(|parent| parent.join(&manifest.chunk_data_dir))
            .ok_or_else(|| anyhow!("invalid cache manifest path: {}", path.display()))?;

        let mut meshes = Vec::with_capacity(manifest.chunks.len());
        for chunk in &manifest.chunks {
            let chunk_path = chunk_dir.join(&chunk.file);
            if chunk_path.extension().and_then(|ext| ext.to_str()) == Some("bin") {
                let prepared = read_prepared_cache_chunk_binary(&chunk_path, chunk_size)?;
                if !prepared.vertices.is_empty()
                    && (!prepared.indices.is_empty() || !prepared.translucent_indices.is_empty())
                {
                    meshes.push(prepared);
                }
            } else {
                let chunk_text = std::fs::read_to_string(&chunk_path).with_context(|| {
                    format!("read cache chunk failed: {}", chunk_path.display())
                })?;
                let payload: NativePreviewCacheChunkFile = serde_json::from_str(&chunk_text)
                    .with_context(|| {
                        format!("parse cache chunk failed: {}", chunk_path.display())
                    })?;
                let vertices = payload
                    .vertices
                    .into_iter()
                    .enumerate()
                    .map(|(index, position)| GpuVertex {
                        position,
                        color: rgb_to_rgba(
                            payload
                                .colors
                                .get(index)
                                .copied()
                                .unwrap_or(DEFAULT_BLOCK_COLOR),
                        ),
                        uv: [0.0, 0.0],
                        use_texture: 0.0,
                        normal: default_vertex_normal(),
                        emissive_tag: 0.0,
                    })
                    .collect::<Vec<_>>();
                if vertices.is_empty() || payload.indices.is_empty() {
                    continue;
                }
                let key = ChunkKey::new(payload.cx, payload.cy, payload.cz);
                meshes.push(PreparedChunkMesh {
                    key,
                    bounds: ChunkBounds::from_key(key, chunk_size),
                    vertices,
                    indices: payload.indices,
                    translucent_indices: Vec::new(),
                    compact_surfaces: Vec::new(),
                });
            }
        }

        let bounds = SceneBounds::from_meshes(&meshes);
        println!(
            "[VIEWER_CACHE] prebuild_cache_ready chunks={} color_chain={} scene_radius={:.2} scene_center=({:.2},{:.2},{:.2}) bounds_source=full_mesh_bounds metadata_size_center=({:.2},{:.2},{:.2})",
            meshes.len(),
            manifest.color_chain,
            bounds.radius,
            bounds.center.x,
            bounds.center.y,
            bounds.center.z,
            manifest.metadata.enclosing_size.x.max(1) as f32 * 0.5,
            manifest.metadata.enclosing_size.y.max(1) as f32 * 0.5,
            manifest.metadata.enclosing_size.z.max(1) as f32 * 0.5,
        );
        println!("[VIEWER_COLOR] vertex_color_enabled=true");
        println!("[VIEWER_COLOR] shading_mode=vertex_color_flat");
        println!(
            "[VIEWER_COLOR] color_source=prebuild_cache native_chain=cache_vertex_color cache_path={}",
            path.display()
        );
        Ok(Self {
            label: path
                .file_name()
                .and_then(|name| name.to_str())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| path.display().to_string()),
            bounds,
            palette_colors: Vec::new(),
            chunk_scene_index: None,
            bootstrap_visible_first: meshes.len(),
            bootstrap_meshes: meshes,
            full_mode_materials: None,
            texture_atlas: None,
        })
    }
}

fn resolve_scene_palette_colors(
    chunk_scene_index: &ChunkSceneIndex,
    display_mode: ViewerDisplayMode,
) -> Vec<[f32; 3]> {
    let resolved_palette_colors = chunk_scene_index
        .palette()
        .iter()
        .map(|entry| {
            let properties = chunk_scene_index.property_pool().get(entry.property_id);
            resolve_palette_color(&entry.block_id, properties, display_mode)
        })
        .collect::<Vec<_>>();
    let palette_colors = resolved_palette_colors
        .iter()
        .map(|entry| entry.rgb)
        .collect::<Vec<_>>();
    let palette_misses = resolved_palette_colors
        .iter()
        .filter(|entry| entry.used_default)
        .count();
    let palette_hits = resolved_palette_colors.len().saturating_sub(palette_misses);
    let cached_entries = if display_mode == ViewerDisplayMode::Full {
        0
    } else {
        block_color_cache().len()
    };
    println!("[VIEWER_COLOR] vertex_color_enabled=true");
    println!("[VIEWER_COLOR] shading_mode=vertex_color_flat");
    let color_source = if display_mode == ViewerDisplayMode::Full {
        "runtime_v2_texture_atlas"
    } else {
        "data/blockColorCache.json"
    };
    println!(
        "[VIEWER_COLOR] display_mode={} color_source={} preview_chain={} native_chain={} palette_entries={} cache_entries={} palette_hits={} palette_misses={}",
        display_mode.label(),
        color_source,
        if display_mode == ViewerDisplayMode::Full {
            "runtime_v2_texture_atlas"
        } else {
            "desktop_ui_cache"
        },
        if display_mode == ViewerDisplayMode::Full {
            "runtime_atlas_sample"
        } else {
            "palette_vertex_color"
        },
        palette_colors.len(),
        cached_entries,
        palette_hits,
        palette_misses
    );
    for (entry, color) in chunk_scene_index
        .palette()
        .iter()
        .zip(resolved_palette_colors.iter())
        .take(3)
    {
        println!(
            "[VIEWER_COLOR] sample_block_color block_id={} normalized_key={} matched_key={} fallback={} rgb=({:.3},{:.3},{:.3})",
            entry.block_id,
            color.normalized_key,
            color.matched_key.as_deref().unwrap_or("<default_gray>"),
            color.used_default,
            color.rgb[0],
            color.rgb[1],
            color.rgb[2]
        );
    }
    palette_colors
}

#[derive(Debug, Clone)]
struct PreparedChunkMesh {
    key: ChunkKey,
    bounds: ChunkBounds,
    vertices: Vec<GpuVertex>,
    indices: Vec<u32>,
    translucent_indices: Vec<u32>,
    compact_surfaces: Vec<CompactSurfaceRecord>,
}

#[derive(Debug, Clone, Copy)]
struct CompactSurfaceRecord {
    face_index: u8,
    min: [f32; 3],
    max: [f32; 3],
    color: [f32; 3],
}

#[derive(Serialize, Deserialize)]
struct NativePreviewCacheChunkFile {
    format: String,
    cx: i32,
    cy: i32,
    cz: i32,
    vertices: Vec<[f32; 3]>,
    indices: Vec<u32>,
    colors: Vec<[f32; 3]>,
}

#[derive(Serialize, Deserialize)]
struct NativePreviewCacheManifestChunk {
    cx: i32,
    cy: i32,
    cz: i32,
    file: String,
    vertex_count: usize,
    index_count: usize,
    bytes: usize,
}

#[derive(Serialize, Deserialize)]
struct NativePreviewCacheManifest {
    format: String,
    color_chain: String,
    metadata: MetadataOutput,
    total_chunks: usize,
    renderable_chunks: usize,
    empty_mesh_chunks: usize,
    chunk_data_dir: String,
    #[serde(default)]
    layer_index_file: Option<String>,
    chunks: Vec<NativePreviewCacheManifestChunk>,
}

struct CacheWriterRequest {
    mesh: PreparedChunkMesh,
    priority: QueuePriority,
    submitted_at: Instant,
}

struct CacheWriterResult {
    mesh: PreparedChunkMesh,
    priority: QueuePriority,
    manifest_chunk: Option<NativePreviewCacheManifestChunk>,
    bytes: usize,
    wait_ms: u128,
    write_ms: u128,
    error: Option<String>,
}

struct PrebuildCachePipeline {
    request_tx: Option<Sender<CacheWriterRequest>>,
    result_rx: Receiver<CacheWriterResult>,
    worker: Option<JoinHandle<()>>,
    chunk_dir: PathBuf,
    chunk_dir_name: String,
}

impl PrebuildCachePipeline {
    fn start(path: &Path, trace: Arc<TraceCounters>) -> Result<Self> {
        let create_started_at = Instant::now();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create cache parent failed: {}", parent.display()))?;
        }
        let manifest_name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "native_preview_cache.json".to_string());
        let chunk_dir_name = format!("{manifest_name}.chunks");
        let chunk_dir = path
            .parent()
            .map(|parent| parent.join(&chunk_dir_name))
            .unwrap_or_else(|| PathBuf::from(&chunk_dir_name));
        let _ = std::fs::remove_dir_all(&chunk_dir);
        std::fs::create_dir_all(&chunk_dir)
            .with_context(|| format!("create cache chunk dir failed: {}", chunk_dir.display()))?;
        TraceCounters::add_ms(&trace.cache_file_create_ms, create_started_at.elapsed());
        println!(
            "[TRACE_WALL] stage=cache_file_create event=end elapsed_ms={} path={} chunk_dir={}",
            create_started_at.elapsed().as_millis(),
            path.display(),
            chunk_dir.display()
        );

        let (request_tx, request_rx) = mpsc::channel::<CacheWriterRequest>();
        let (result_tx, result_rx) = mpsc::channel::<CacheWriterResult>();
        let worker_chunk_dir = chunk_dir.clone();
        let worker = thread::spawn(move || {
            loop {
                let wait_started_at = Instant::now();
                let request = match request_rx.recv() {
                    Ok(request) => request,
                    Err(_) => break,
                };
                let wait_ms = wait_started_at.elapsed();
                TraceCounters::add_ms(&trace.writer_wait_ms, wait_ms);
                let write_started_at = Instant::now();
                let file_name = cache_chunk_file_name(request.mesh.key);
                let chunk_path = worker_chunk_dir.join(&file_name);
                let write_result = write_prepared_cache_chunk_binary(&chunk_path, &request.mesh);
                let write_ms = write_started_at.elapsed();
                TraceCounters::add_ms(&trace.writer_busy_ms, write_ms);
                trace.writer_chunks.fetch_add(1, Ordering::Relaxed);
                let (manifest_chunk, bytes, error) = match write_result {
                    Ok(bytes) => {
                        let manifest_chunk = NativePreviewCacheManifestChunk {
                            cx: request.mesh.key.cx,
                            cy: request.mesh.key.cy,
                            cz: request.mesh.key.cz,
                            file: file_name,
                            vertex_count: request.mesh.vertex_count(),
                            index_count: request.mesh.index_count(),
                            bytes,
                        };
                        (Some(manifest_chunk), bytes, None)
                    }
                    Err(error) => {
                        trace.writer_errors.fetch_add(1, Ordering::Relaxed);
                        (None, 0, Some(error.to_string()))
                    }
                };
                println!(
                    "[TRACE_WALL] stage=cache_write event=chunk_written key=({}, {}, {}) wait_ms={} write_ms={} bytes={} error={}",
                    request.mesh.key.cx,
                    request.mesh.key.cy,
                    request.mesh.key.cz,
                    wait_ms.as_millis(),
                    write_ms.as_millis(),
                    bytes,
                    error.as_deref().unwrap_or("none")
                );
                if result_tx
                    .send(CacheWriterResult {
                        mesh: request.mesh,
                        priority: request.priority,
                        manifest_chunk,
                        bytes,
                        wait_ms: request.submitted_at.elapsed().as_millis(),
                        write_ms: write_ms.as_millis(),
                        error,
                    })
                    .is_err()
                {
                    break;
                }
            }
            println!("[TRACE_WALL] stage=cache_write event=writer_exit");
        });

        println!(
            "[TRACE_WALL] stage=cache_write event=pipeline_start path={} chunk_dir={}",
            path.display(),
            chunk_dir.display()
        );
        Ok(Self {
            request_tx: Some(request_tx),
            result_rx,
            worker: Some(worker),
            chunk_dir,
            chunk_dir_name,
        })
    }
}

fn cache_chunk_file_name(key: ChunkKey) -> String {
    format!("chunk_{}_{}_{}.bin", key.cx, key.cy, key.cz)
}

impl PreparedChunkMesh {
    fn placeholder() -> Self {
        let vertices = vec![
            GpuVertex {
                position: [-1.0, -1.0, 1.0],
                color: [0.91, 0.31, 0.24, 1.0],
                uv: [0.0, 0.0],
                use_texture: 0.0,
                normal: default_vertex_normal(),
                emissive_tag: 0.0,
            },
            GpuVertex {
                position: [1.0, -1.0, 1.0],
                color: [0.91, 0.31, 0.24, 1.0],
                uv: [0.0, 0.0],
                use_texture: 0.0,
                normal: default_vertex_normal(),
                emissive_tag: 0.0,
            },
            GpuVertex {
                position: [1.0, 1.0, 1.0],
                color: [0.91, 0.31, 0.24, 1.0],
                uv: [0.0, 0.0],
                use_texture: 0.0,
                normal: default_vertex_normal(),
                emissive_tag: 0.0,
            },
            GpuVertex {
                position: [-1.0, 1.0, 1.0],
                color: [0.91, 0.31, 0.24, 1.0],
                uv: [0.0, 0.0],
                use_texture: 0.0,
                normal: default_vertex_normal(),
                emissive_tag: 0.0,
            },
            GpuVertex {
                position: [-1.0, -1.0, -1.0],
                color: [0.13, 0.60, 0.95, 1.0],
                uv: [0.0, 0.0],
                use_texture: 0.0,
                normal: default_vertex_normal(),
                emissive_tag: 0.0,
            },
            GpuVertex {
                position: [1.0, -1.0, -1.0],
                color: [0.13, 0.60, 0.95, 1.0],
                uv: [0.0, 0.0],
                use_texture: 0.0,
                normal: default_vertex_normal(),
                emissive_tag: 0.0,
            },
            GpuVertex {
                position: [1.0, 1.0, -1.0],
                color: [0.13, 0.60, 0.95, 1.0],
                uv: [0.0, 0.0],
                use_texture: 0.0,
                normal: default_vertex_normal(),
                emissive_tag: 0.0,
            },
            GpuVertex {
                position: [-1.0, 1.0, -1.0],
                color: [0.13, 0.60, 0.95, 1.0],
                uv: [0.0, 0.0],
                use_texture: 0.0,
                normal: default_vertex_normal(),
                emissive_tag: 0.0,
            },
        ];
        let indices = vec![
            0, 1, 2, 0, 2, 3, 1, 5, 6, 1, 6, 2, 5, 4, 7, 5, 7, 6, 4, 0, 3, 4, 3, 7, 3, 2, 6, 3, 6,
            7, 4, 5, 1, 4, 1, 0,
        ];
        Self {
            key: ChunkKey::new(0, 0, 0),
            bounds: ChunkBounds {
                min: Vec3::splat(-1.0),
                max: Vec3::splat(1.0),
                center: Vec3::ZERO,
                radius: Vec3::splat(2.0).length() * 0.5,
            },
            vertices,
            indices,
            translucent_indices: Vec::new(),
            compact_surfaces: Vec::new(),
        }
    }

    fn from_mesh_chunk(
        mesh_chunk: MeshChunkOutput,
        palette_colors: &[[f32; 3]],
        chunk_size: u32,
    ) -> Option<Self> {
        if mesh_chunk.vertices.is_empty()
            && mesh_chunk.textured_vertices.is_empty()
            && mesh_chunk.indices.is_empty()
            && mesh_chunk.translucent_indices.is_empty()
            && mesh_chunk.compact_surfaces.is_empty()
        {
            return None;
        }

        let vertices = if !mesh_chunk.textured_vertices.is_empty() {
            mesh_chunk
                .textured_vertices
                .into_iter()
                .map(textured_vertex_from_output)
                .collect::<Vec<_>>()
        } else {
            let mut vertices = Vec::<GpuVertex>::with_capacity(mesh_chunk.vertices.len());
            for (vertex_index, position) in mesh_chunk.vertices.into_iter().enumerate() {
                let face_index = vertex_index / 4;
                let palette_index = mesh_chunk
                    .color_indices
                    .get(face_index)
                    .copied()
                    .unwrap_or(0) as usize;
                let color = palette_colors
                    .get(palette_index)
                    .copied()
                    .unwrap_or(DEFAULT_BLOCK_COLOR);
                vertices.push(GpuVertex {
                    position,
                    color: rgb_to_rgba(color),
                    uv: [0.0, 0.0],
                    use_texture: 0.0,
                    normal: default_vertex_normal(),
                    emissive_tag: 0.0,
                });
            }
            vertices
        };

        let key = ChunkKey::new(mesh_chunk.cx, mesh_chunk.cy, mesh_chunk.cz);
        let compact_surfaces = mesh_chunk
            .compact_surfaces
            .into_iter()
            .map(|surface| compact_surface_from_output(surface, palette_colors))
            .collect::<Vec<_>>();

        Some(Self {
            key,
            bounds: ChunkBounds::from_key(key, chunk_size),
            vertices,
            indices: mesh_chunk.indices,
            translucent_indices: mesh_chunk.translucent_indices,
            compact_surfaces,
        })
    }

    fn vertex_count(&self) -> usize {
        self.vertices
            .len()
            .saturating_add(self.compact_surfaces.len().saturating_mul(4))
    }

    fn index_count(&self) -> usize {
        self.indices
            .len()
            .saturating_add(self.translucent_indices.len())
            .saturating_add(self.compact_surfaces.len().saturating_mul(6))
    }

    fn estimated_bytes(&self) -> usize {
        self.vertices
            .len()
            .saturating_mul(size_of::<GpuVertex>())
            .saturating_add(self.indices.len().saturating_mul(size_of::<u32>()))
            .saturating_add(
                self.translucent_indices
                    .len()
                    .saturating_mul(size_of::<u32>()),
            )
            .saturating_add(
                self.compact_surfaces
                    .len()
                    .saturating_mul(size_of::<CompactSurfaceRecord>()),
            )
            .saturating_add(256)
    }
}

fn textured_vertex_from_output(vertex: TexturedVertexOutput) -> GpuVertex {
    GpuVertex {
        position: vertex.position,
        color: [1.0, 1.0, 1.0, 1.0],
        uv: vertex.uv,
        use_texture: 1.0,
        normal: default_vertex_normal(),
        emissive_tag: 0.0,
    }
}

fn compact_surface_from_output(
    surface: CompactSurfaceOutput,
    palette_colors: &[[f32; 3]],
) -> CompactSurfaceRecord {
    CompactSurfaceRecord {
        face_index: surface.face_index,
        min: surface.min,
        max: surface.max,
        color: palette_colors
            .get(surface.palette_id as usize)
            .copied()
            .unwrap_or(DEFAULT_BLOCK_COLOR),
    }
}

fn vertex_rgb(vertex: &GpuVertex) -> [f32; 3] {
    [vertex.color[0], vertex.color[1], vertex.color[2]]
}

fn write_prepared_cache_chunk_binary(
    path: &Path,
    mesh: &PreparedChunkMesh,
) -> std::io::Result<usize> {
    if compact_cache_v2_enabled() {
        return write_prepared_cache_chunk_compact_v2(path, mesh);
    }
    let mut writer = BufWriter::new(File::create(path)?);
    let face_color_count = mesh.vertices.len().div_ceil(4);
    writer.write_all(NATIVE_CACHE_CHUNK_BINARY_MAGIC_V3)?;
    writer.write_all(&mesh.key.cx.to_le_bytes())?;
    writer.write_all(&mesh.key.cy.to_le_bytes())?;
    writer.write_all(&mesh.key.cz.to_le_bytes())?;
    writer.write_all(&(mesh.vertices.len() as u32).to_le_bytes())?;
    writer.write_all(&(mesh.indices.len() as u32).to_le_bytes())?;
    writer.write_all(&(face_color_count as u32).to_le_bytes())?;
    for vertex in &mesh.vertices {
        for value in vertex.position {
            writer.write_all(&value.to_le_bytes())?;
        }
    }
    for face_index in 0..face_color_count {
        let color = mesh
            .vertices
            .get(face_index.saturating_mul(4))
            .map(vertex_rgb)
            .unwrap_or(DEFAULT_BLOCK_COLOR);
        for channel in color {
            let byte = (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
            writer.write_all(&[byte])?;
        }
    }
    for index in &mesh.indices {
        writer.write_all(&index.to_le_bytes())?;
    }
    writer.flush()?;
    Ok(std::fs::metadata(path)?.len() as usize)
}

fn write_prepared_cache_chunk_compact_v2(
    path: &Path,
    mesh: &PreparedChunkMesh,
) -> std::io::Result<usize> {
    let mut compact_quads = mesh.compact_surfaces.clone();
    compact_quads.extend(collect_compact_quad_records(mesh));
    if compact_quads.is_empty() && !mesh.vertices.is_empty() {
        return write_prepared_cache_chunk_binary_v3(path, mesh);
    }
    let mut writer = BufWriter::new(File::create(path)?);
    writer.write_all(NATIVE_CACHE_CHUNK_BINARY_MAGIC_V4)?;
    writer.write_all(&mesh.key.cx.to_le_bytes())?;
    writer.write_all(&mesh.key.cy.to_le_bytes())?;
    writer.write_all(&mesh.key.cz.to_le_bytes())?;
    writer.write_all(&(compact_quads.len() as u32).to_le_bytes())?;
    writer.write_all(&(mesh.vertices.len() as u32).to_le_bytes())?;
    writer.write_all(&(mesh.indices.len() as u32).to_le_bytes())?;
    let face_color_count = mesh.vertices.len().div_ceil(4);
    writer.write_all(&(face_color_count as u32).to_le_bytes())?;
    for quad in &compact_quads {
        writer.write_all(&[quad.face_index])?;
        for value in quad.min {
            writer.write_all(&value.to_le_bytes())?;
        }
        for value in quad.max {
            writer.write_all(&value.to_le_bytes())?;
        }
        for channel in quad.color {
            let byte = (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
            writer.write_all(&[byte])?;
        }
    }
    for vertex in &mesh.vertices {
        for value in vertex.position {
            writer.write_all(&value.to_le_bytes())?;
        }
    }
    for face_index in 0..face_color_count {
        let color = mesh
            .vertices
            .get(face_index.saturating_mul(4))
            .map(vertex_rgb)
            .unwrap_or(DEFAULT_BLOCK_COLOR);
        for channel in color {
            let byte = (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
            writer.write_all(&[byte])?;
        }
    }
    for index in &mesh.indices {
        writer.write_all(&index.to_le_bytes())?;
    }
    writer.flush()?;
    Ok(std::fs::metadata(path)?.len() as usize)
}

fn write_prepared_cache_chunk_binary_v3(
    path: &Path,
    mesh: &PreparedChunkMesh,
) -> std::io::Result<usize> {
    let mut writer = BufWriter::new(File::create(path)?);
    let face_color_count = mesh.vertices.len().div_ceil(4);
    writer.write_all(NATIVE_CACHE_CHUNK_BINARY_MAGIC_V3)?;
    writer.write_all(&mesh.key.cx.to_le_bytes())?;
    writer.write_all(&mesh.key.cy.to_le_bytes())?;
    writer.write_all(&mesh.key.cz.to_le_bytes())?;
    writer.write_all(&(mesh.vertices.len() as u32).to_le_bytes())?;
    writer.write_all(&(mesh.indices.len() as u32).to_le_bytes())?;
    writer.write_all(&(face_color_count as u32).to_le_bytes())?;
    for vertex in &mesh.vertices {
        for value in vertex.position {
            writer.write_all(&value.to_le_bytes())?;
        }
    }
    for face_index in 0..face_color_count {
        let color = mesh
            .vertices
            .get(face_index.saturating_mul(4))
            .map(vertex_rgb)
            .unwrap_or(DEFAULT_BLOCK_COLOR);
        for channel in color {
            let byte = (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
            writer.write_all(&[byte])?;
        }
    }
    for index in &mesh.indices {
        writer.write_all(&index.to_le_bytes())?;
    }
    writer.flush()?;
    Ok(std::fs::metadata(path)?.len() as usize)
}

fn collect_compact_quad_records(mesh: &PreparedChunkMesh) -> Vec<CompactSurfaceRecord> {
    let mut records = Vec::with_capacity(mesh.indices.len() / 6);
    for indices in mesh.indices.chunks_exact(6) {
        let expected = [
            indices[0],
            indices[0] + 1,
            indices[0] + 2,
            indices[0],
            indices[0] + 2,
            indices[0] + 3,
        ];
        if indices != expected {
            return Vec::new();
        }
        let base = indices[0] as usize;
        let Some(vertices) = mesh.vertices.get(base..base.saturating_add(4)) else {
            return Vec::new();
        };
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for vertex in vertices {
            for axis in 0..3 {
                min[axis] = min[axis].min(vertex.position[axis]);
                max[axis] = max[axis].max(vertex.position[axis]);
            }
        }
        let Some(face_index) = compact_face_index(vertices) else {
            return Vec::new();
        };
        records.push(CompactSurfaceRecord {
            face_index,
            min,
            max,
            color: vertex_rgb(&vertices[0]),
        });
    }
    records
}

fn compact_face_index(vertices: &[GpuVertex]) -> Option<u8> {
    let same = |a: f32, b: f32| (a - b).abs() <= 0.0001;
    let all_same = |axis: usize| {
        let first = vertices.first().map(|vertex| vertex.position[axis])?;
        vertices
            .iter()
            .all(|vertex| same(vertex.position[axis], first))
            .then_some(first)
    };
    let a = Vec3::from(vertices.first()?.position);
    let b = Vec3::from(vertices.get(1)?.position);
    let c = Vec3::from(vertices.get(2)?.position);
    let normal = (b - a).cross(c - a);
    let abs = normal.abs();
    if abs.x >= abs.y && abs.x >= abs.z {
        let _ = all_same(0)?;
        return Some(if normal.x < 0.0 { 0 } else { 1 });
    }
    if abs.y >= abs.x && abs.y >= abs.z {
        let _ = all_same(1)?;
        return Some(if normal.y < 0.0 { 2 } else { 3 });
    }
    if abs.z >= abs.x && abs.z >= abs.y {
        let _ = all_same(2)?;
        return Some(if normal.z < 0.0 { 4 } else { 5 });
    }
    None
}

fn append_compact_quad(
    vertices: &mut Vec<GpuVertex>,
    indices: &mut Vec<u32>,
    face_index: usize,
    min: [f32; 3],
    max: [f32; 3],
    color: [f32; 3],
) -> Result<()> {
    let templates = match face_index {
        0..=5 => FACE_VERTICES_RUNTIME[face_index],
        _ => bail!("invalid compact cache face index: {face_index}"),
    };
    let base = vertices.len() as u32;
    for template in templates {
        vertices.push(GpuVertex {
            position: [
                if template[0] == 0.0 { min[0] } else { max[0] },
                if template[1] == 0.0 { min[1] } else { max[1] },
                if template[2] == 0.0 { min[2] } else { max[2] },
            ],
            color: [color[0], color[1], color[2], 1.0],
            uv: [0.0, 0.0],
            use_texture: 0.0,
            normal: default_vertex_normal(),
            emissive_tag: 0.0,
        });
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    Ok(())
}

fn read_i32_le(bytes: &[u8], offset: &mut usize) -> Result<i32> {
    let end = offset.saturating_add(4);
    let raw = bytes
        .get(*offset..end)
        .ok_or_else(|| anyhow!("invalid binary cache chunk: truncated i32"))?;
    *offset = end;
    Ok(i32::from_le_bytes(raw.try_into().expect("slice length")))
}

fn read_u32_le(bytes: &[u8], offset: &mut usize) -> Result<u32> {
    let end = offset.saturating_add(4);
    let raw = bytes
        .get(*offset..end)
        .ok_or_else(|| anyhow!("invalid binary cache chunk: truncated u32"))?;
    *offset = end;
    Ok(u32::from_le_bytes(raw.try_into().expect("slice length")))
}

fn read_f32_le(bytes: &[u8], offset: &mut usize) -> Result<f32> {
    let end = offset.saturating_add(4);
    let raw = bytes
        .get(*offset..end)
        .ok_or_else(|| anyhow!("invalid binary cache chunk: truncated f32"))?;
    *offset = end;
    Ok(f32::from_le_bytes(raw.try_into().expect("slice length")))
}

fn read_prepared_cache_chunk_binary(path: &Path, chunk_size: u32) -> Result<PreparedChunkMesh> {
    let mut bytes = Vec::new();
    File::open(path)
        .with_context(|| format!("read binary cache chunk failed: {}", path.display()))?
        .read_to_end(&mut bytes)
        .with_context(|| format!("read binary cache chunk failed: {}", path.display()))?;
    if bytes.len() < NATIVE_CACHE_CHUNK_BINARY_MAGIC_V2.len() {
        bail!("invalid binary cache chunk magic: {}", path.display());
    }
    let magic = &bytes[..NATIVE_CACHE_CHUNK_BINARY_MAGIC_V2.len()];
    let is_v2 = magic == NATIVE_CACHE_CHUNK_BINARY_MAGIC_V2;
    let is_v3 = magic == NATIVE_CACHE_CHUNK_BINARY_MAGIC_V3;
    let is_v4 = magic == NATIVE_CACHE_CHUNK_BINARY_MAGIC_V4;
    if !is_v2 && !is_v3 && !is_v4 {
        bail!("invalid binary cache chunk magic: {}", path.display());
    }
    let mut offset = NATIVE_CACHE_CHUNK_BINARY_MAGIC_V2.len();
    let cx = read_i32_le(&bytes, &mut offset)?;
    let cy = read_i32_le(&bytes, &mut offset)?;
    let cz = read_i32_le(&bytes, &mut offset)?;
    if is_v4 {
        let quad_count = read_u32_le(&bytes, &mut offset)? as usize;
        let raw_vertex_count = read_u32_le(&bytes, &mut offset)? as usize;
        let raw_index_count = read_u32_le(&bytes, &mut offset)? as usize;
        let raw_face_color_count = read_u32_le(&bytes, &mut offset)? as usize;
        let mut vertices = Vec::with_capacity(
            quad_count
                .saturating_mul(4)
                .saturating_add(raw_vertex_count),
        );
        let mut indices =
            Vec::with_capacity(quad_count.saturating_mul(6).saturating_add(raw_index_count));
        for _ in 0..quad_count {
            let face_index = *bytes
                .get(offset)
                .ok_or_else(|| anyhow!("invalid compact cache chunk: truncated face"))?
                as usize;
            offset += 1;
            let min = [
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
            ];
            let max = [
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
            ];
            let rgb = bytes
                .get(offset..offset.saturating_add(3))
                .ok_or_else(|| anyhow!("invalid compact cache chunk: truncated rgb"))?;
            offset += 3;
            let color = [
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
            ];
            append_compact_quad(&mut vertices, &mut indices, face_index, min, max, color)?;
        }
        let raw_vertex_base = vertices.len() as u32;
        for _ in 0..raw_vertex_count {
            let position = [
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
            ];
            vertices.push(GpuVertex {
                position,
                color: rgb_to_rgba(DEFAULT_BLOCK_COLOR),
                uv: [0.0, 0.0],
                use_texture: 0.0,
                normal: default_vertex_normal(),
                emissive_tag: 0.0,
            });
        }
        let mut raw_face_colors = Vec::with_capacity(raw_face_color_count);
        for _ in 0..raw_face_color_count {
            let rgb = bytes
                .get(offset..offset.saturating_add(3))
                .ok_or_else(|| anyhow!("invalid compact cache chunk: truncated raw rgb"))?;
            offset += 3;
            raw_face_colors.push([
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
            ]);
        }
        for (raw_index, vertex) in vertices
            .iter_mut()
            .skip(raw_vertex_base as usize)
            .enumerate()
        {
            vertex.color = raw_face_colors
                .get(raw_index / 4)
                .copied()
                .map(rgb_to_rgba)
                .unwrap_or(rgb_to_rgba(DEFAULT_BLOCK_COLOR));
        }
        for _ in 0..raw_index_count {
            indices.push(raw_vertex_base + read_u32_le(&bytes, &mut offset)?);
        }
        let key = ChunkKey::new(cx, cy, cz);
        return Ok(PreparedChunkMesh {
            key,
            bounds: ChunkBounds::from_key(key, chunk_size),
            vertices,
            indices,
            translucent_indices: Vec::new(),
            compact_surfaces: Vec::new(),
        });
    }
    let vertex_count = read_u32_le(&bytes, &mut offset)? as usize;
    let index_count = read_u32_le(&bytes, &mut offset)? as usize;
    let face_color_count = if is_v3 {
        read_u32_le(&bytes, &mut offset)? as usize
    } else {
        0
    };
    let mut vertices = Vec::with_capacity(vertex_count);
    for vertex_index in 0..vertex_count {
        let position = [
            read_f32_le(&bytes, &mut offset)?,
            read_f32_le(&bytes, &mut offset)?,
            read_f32_le(&bytes, &mut offset)?,
        ];
        let color = if is_v2 {
            [
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
            ]
        } else {
            let _ = vertex_index;
            DEFAULT_BLOCK_COLOR
        };
        vertices.push(GpuVertex {
            position,
            color: rgb_to_rgba(color),
            uv: [0.0, 0.0],
            use_texture: 0.0,
            normal: default_vertex_normal(),
            emissive_tag: 0.0,
        });
    }
    if is_v3 {
        let mut face_colors = Vec::with_capacity(face_color_count);
        for _ in 0..face_color_count {
            let rgb = bytes
                .get(offset..offset.saturating_add(3))
                .ok_or_else(|| anyhow!("invalid binary cache chunk: truncated rgb"))?;
            offset += 3;
            face_colors.push([
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
            ]);
        }
        for (vertex_index, vertex) in vertices.iter_mut().enumerate() {
            vertex.color = face_colors
                .get(vertex_index / 4)
                .copied()
                .map(rgb_to_rgba)
                .unwrap_or(rgb_to_rgba(DEFAULT_BLOCK_COLOR));
        }
    }
    let mut indices = Vec::with_capacity(index_count);
    for _ in 0..index_count {
        indices.push(read_u32_le(&bytes, &mut offset)?);
    }
    let key = ChunkKey::new(cx, cy, cz);
    Ok(PreparedChunkMesh {
        key,
        bounds: ChunkBounds::from_key(key, chunk_size),
        vertices,
        indices,
        translucent_indices: Vec::new(),
        compact_surfaces: Vec::new(),
    })
}

fn prebuild_cache_total_bytes(path: &Path, include_chunks: bool) -> u64 {
    let manifest_bytes = std::fs::metadata(path)
        .ok()
        .map(|meta| meta.len())
        .unwrap_or(0);
    if !include_chunks {
        return manifest_bytes;
    }
    let Some(file_name) = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
    else {
        return manifest_bytes;
    };
    let chunk_dir = path
        .parent()
        .map(|parent| parent.join(format!("{file_name}.chunks")))
        .unwrap_or_else(|| PathBuf::from(format!("{file_name}.chunks")));
    let chunk_bytes = std::fs::read_dir(chunk_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(|entry| entry.ok()))
        .filter_map(|entry| entry.metadata().ok().map(|metadata| metadata.len()))
        .sum::<u64>();
    manifest_bytes.saturating_add(chunk_bytes)
}

fn default_build_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get().clamp(2, 8))
        .unwrap_or(2)
}

fn spawn_build_worker_pool(
    scene_index: Arc<ChunkSceneIndex>,
    palette_colors: Arc<Vec<[f32; 3]>>,
    full_mode_materials: Option<Arc<FullModeMaterialCache>>,
    worker_count: usize,
    trace: Arc<TraceCounters>,
) -> (
    Sender<BuildWorkerRequest>,
    Receiver<BuildWorkerResult>,
    Vec<JoinHandle<()>>,
) {
    let (request_tx, request_rx) = mpsc::channel::<BuildWorkerRequest>();
    let (result_tx, result_rx) = mpsc::channel::<BuildWorkerResult>();
    let shared_request_rx = Arc::new(Mutex::new(request_rx));
    let mut workers = Vec::with_capacity(worker_count);
    for worker_id in 0..worker_count {
        let request_rx = shared_request_rx.clone();
        let result_tx = result_tx.clone();
        let scene_index = scene_index.clone();
        let palette_colors = palette_colors.clone();
        let full_mode_materials = full_mode_materials.clone();
        let trace = trace.clone();
        let worker = thread::spawn(move || {
            let execution = std::panic::catch_unwind(move || {
                loop {
                    let wait_started_at = Instant::now();
                    let request = match request_rx.lock() {
                        Ok(receiver) => match receiver.recv() {
                            Ok(request) => request,
                            Err(_) => {
                                println!(
                                    "[VIEWER_ASYNC] worker_exit worker_id={} reason=request_channel_closed",
                                    worker_id
                                );
                                break;
                            }
                        },
                        Err(_) => {
                            println!(
                                "[VIEWER_ASYNC] worker_exit worker_id={} reason=request_queue_poisoned",
                                worker_id
                            );
                            break;
                        }
                    };
                    TraceCounters::add_ms(&trace.worker_wait_ms, wait_started_at.elapsed());

                    let requested_keys = request.keys;
                    let build_queue_wait_ms = request.enqueued_at.elapsed().as_millis() as u64;
                    trace
                        .build_dequeue_ms
                        .fetch_add(build_queue_wait_ms, Ordering::Relaxed);
                    trace.build_dequeue_batches.fetch_add(1, Ordering::Relaxed);
                    trace
                        .build_dequeue_chunks
                        .fetch_add(requested_keys.len(), Ordering::Relaxed);
                    println!(
                        "[TRACE_WALL] stage=chunk_task event=dequeue worker_id={} batch_id={} chunks={} queue_wait_ms={}",
                        worker_id,
                        request.batch_id,
                        requested_keys.len(),
                        build_queue_wait_ms
                    );
                    let requested_set = requested_keys.iter().copied().collect::<HashSet<_>>();
                    let worker_started_at = Instant::now();
                    println!(
                        "[TRACE_WALL] stage=mesh_build_worker event=start worker_id={} batch_id={} chunks={}",
                        worker_id,
                        request.batch_id,
                        requested_keys.len()
                    );
                    let build_result = if let Some(materials) = full_mode_materials.as_ref() {
                        scene_index.build_textured_chunk_meshes(&requested_keys, materials)
                    } else {
                        scene_index.build_chunk_meshes(&requested_keys)
                    };
                    match build_result {
                        Ok(chunk_meshes) => {
                            let worker_busy_elapsed = worker_started_at.elapsed();
                            TraceCounters::add_ms(&trace.worker_busy_ms, worker_busy_elapsed);
                            trace.worker_batches.fetch_add(1, Ordering::Relaxed);
                            trace
                                .worker_chunks
                                .fetch_add(requested_keys.len(), Ordering::Relaxed);
                            let mut prepared_by_key =
                                BTreeMap::<ChunkKey, PreparedChunkMesh>::new();
                            for mesh_chunk in chunk_meshes {
                                if let Some(prepared) = PreparedChunkMesh::from_mesh_chunk(
                                    mesh_chunk,
                                    palette_colors.as_slice(),
                                    scene_index.chunk_size(),
                                ) {
                                    prepared_by_key.insert(prepared.key, prepared);
                                }
                            }
                            println!(
                                "[VIEWER_ASYNC] worker_batch_built worker_id={} batch_id={} requested_chunks={} mesh_chunks={}",
                                worker_id,
                                request.batch_id,
                                requested_set.len(),
                                prepared_by_key.len()
                            );
                            println!(
                                "[TRACE_WALL] stage=mesh_build_worker event=end worker_id={} batch_id={} elapsed_ms={} requested_chunks={} mesh_chunks={}",
                                worker_id,
                                request.batch_id,
                                worker_busy_elapsed.as_millis(),
                                requested_set.len(),
                                prepared_by_key.len()
                            );
                            for key in requested_keys {
                                let result = BuildWorkerResult {
                                    batch_id: request.batch_id,
                                    key,
                                    priority: request.priority,
                                    mesh: prepared_by_key.remove(&key),
                                    error: None,
                                };
                                let send_started_at = Instant::now();
                                if result_tx.send(result).is_err() {
                                    println!(
                                        "[VIEWER_ASYNC] worker_exit worker_id={} reason=result_channel_closed",
                                        worker_id
                                    );
                                    return;
                                }
                                TraceCounters::add_ms(
                                    &trace.worker_result_send_ms,
                                    send_started_at.elapsed(),
                                );
                            }
                        }
                        Err(error) => {
                            let worker_busy_elapsed = worker_started_at.elapsed();
                            TraceCounters::add_ms(&trace.worker_busy_ms, worker_busy_elapsed);
                            trace.worker_batches.fetch_add(1, Ordering::Relaxed);
                            trace
                                .worker_chunks
                                .fetch_add(requested_keys.len(), Ordering::Relaxed);
                            println!(
                                "[TRACE_WALL] stage=mesh_build_worker event=end worker_id={} batch_id={} elapsed_ms={} error=true",
                                worker_id,
                                request.batch_id,
                                worker_busy_elapsed.as_millis()
                            );
                            let error = error.to_string();
                            for key in requested_keys {
                                let result = BuildWorkerResult {
                                    batch_id: request.batch_id,
                                    key,
                                    priority: request.priority,
                                    mesh: None,
                                    error: Some(error.clone()),
                                };
                                if result_tx.send(result).is_err() {
                                    println!(
                                        "[VIEWER_ASYNC] worker_exit worker_id={} reason=result_channel_closed",
                                        worker_id
                                    );
                                    return;
                                }
                            }
                        }
                    }
                }
            });
            if execution.is_err() {
                println!(
                    "[VIEWER_ASYNC] worker_panic worker_id={} reason=panic_in_build_loop",
                    worker_id
                );
            }
        });
        workers.push(worker);
    }
    drop(result_tx);
    (request_tx, result_rx, workers)
}

struct CachedChunkMesh {
    mesh: PreparedChunkMesh,
    last_used_frame: u64,
    built_frame: u64,
    estimated_bytes: usize,
}

struct ChunkMeshCpuCache {
    entries: BTreeMap<ChunkKey, CachedChunkMesh>,
    max_chunks: usize,
    max_bytes: usize,
    current_bytes: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct EvictionStats {
    candidate_count: usize,
    evicted_count: usize,
    evicted_recent_count: usize,
    before_count: usize,
    after_count: usize,
    before_bytes: usize,
    after_bytes: usize,
    evicted_bytes: usize,
}

#[derive(Debug, Default, Clone, Copy)]
struct QueuePreparationStats {
    target_cache_hits: usize,
    target_cache_misses: usize,
    target_pending_hits: usize,
    target_queued_builds: usize,
    target_queued_uploads: usize,
    target_reenter_chunks: usize,
    target_cache_hit_reuploads: usize,
    target_cache_miss_rebuilds: usize,
    preload_cache_hits: usize,
    preload_cache_misses: usize,
    preload_pending_hits: usize,
    preload_queued_builds: usize,
    preload_queued_uploads: usize,
    preload_reenter_chunks: usize,
    preload_cache_hit_reuploads: usize,
    preload_cache_miss_rebuilds: usize,
}

#[derive(Debug, Default, Clone, Copy)]
struct QueueProcessingStats {
    target_usage: BudgetUsage,
    preload_usage: BudgetUsage,
    target_submitted_uploads: usize,
    preload_submitted_uploads: usize,
    target_submitted_builds: usize,
    preload_submitted_builds: usize,
    deferred_target_chunks: usize,
    deferred_preload_chunks: usize,
}

#[derive(Debug, Clone, Copy)]
struct BudgetSplit<T> {
    target: T,
    preload: T,
}

#[derive(Debug, Default, Clone, Copy)]
struct AsyncBuildDrainStats {
    completed_builds: usize,
    dropped_build_results: usize,
    retained_build_results: usize,
    reused_completed_results: usize,
    build_result_to_cache: usize,
    build_result_to_upload: usize,
}

#[derive(Debug, Clone)]
struct BuildWorkerRequest {
    batch_id: u64,
    keys: Vec<ChunkKey>,
    priority: QueuePriority,
    enqueued_at: Instant,
}

#[derive(Debug)]
struct BuildWorkerResult {
    batch_id: u64,
    key: ChunkKey,
    priority: QueuePriority,
    mesh: Option<PreparedChunkMesh>,
    error: Option<String>,
}

impl ChunkMeshCpuCache {
    fn new(max_chunks: usize, max_bytes: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            max_chunks,
            max_bytes,
            current_bytes: 0,
        }
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn contains(&self, key: ChunkKey) -> bool {
        self.entries.contains_key(&key)
    }

    fn touch(&mut self, key: ChunkKey, frame_index: u64) {
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used_frame = frame_index;
        }
    }

    fn get(&mut self, key: ChunkKey, frame_index: u64) -> Option<&PreparedChunkMesh> {
        let entry = self.entries.get_mut(&key)?;
        entry.last_used_frame = frame_index;
        Some(&entry.mesh)
    }

    fn get_cloned(&mut self, key: ChunkKey, frame_index: u64) -> Option<PreparedChunkMesh> {
        self.get(key, frame_index).cloned()
    }

    fn mesh_cost(&mut self, key: ChunkKey, frame_index: u64) -> Option<(usize, usize)> {
        let mesh = self.get(key, frame_index)?;
        Some((mesh.vertex_count(), mesh.index_count()))
    }

    fn insert(&mut self, mesh: PreparedChunkMesh, frame_index: u64) {
        let estimated_bytes = mesh.estimated_bytes();
        self.entries.insert(
            mesh.key,
            CachedChunkMesh {
                mesh,
                last_used_frame: frame_index,
                built_frame: frame_index,
                estimated_bytes,
            },
        );
        self.current_bytes = self.current_bytes.saturating_add(estimated_bytes);
    }

    fn prune(
        &mut self,
        protected_keys: &HashSet<ChunkKey>,
        recent_cutoff_frame: u64,
    ) -> EvictionStats {
        let before_count = self.entries.len();
        let before_bytes = self.current_bytes;
        let mut evictable = self
            .entries
            .iter()
            .filter(|(key, _)| !protected_keys.contains(key))
            .map(|(key, entry)| {
                (
                    entry.last_used_frame,
                    entry.built_frame,
                    entry.estimated_bytes,
                    *key,
                )
            })
            .collect::<Vec<_>>();
        let candidate_count = evictable.len();

        if self.entries.len() <= self.max_chunks && self.current_bytes <= self.max_bytes {
            return EvictionStats {
                candidate_count,
                evicted_count: 0,
                evicted_recent_count: 0,
                before_count,
                after_count: before_count,
                before_bytes,
                after_bytes: before_bytes,
                evicted_bytes: 0,
            };
        }

        evictable.sort_by_key(|(last_used_frame, built_frame, estimated_bytes, key)| {
            (*last_used_frame, *built_frame, *estimated_bytes, *key)
        });

        let mut evicted_count = 0_usize;
        let mut evicted_recent_count = 0_usize;
        let mut evicted_bytes = 0_usize;
        for (_, _, _, key) in evictable {
            if self.entries.len() <= self.max_chunks && self.current_bytes <= self.max_bytes {
                break;
            }
            if let Some(removed) = self.entries.remove(&key) {
                evicted_count += 1;
                if removed.last_used_frame >= recent_cutoff_frame {
                    evicted_recent_count += 1;
                }
                evicted_bytes = evicted_bytes.saturating_add(removed.estimated_bytes);
                self.current_bytes = self.current_bytes.saturating_sub(removed.estimated_bytes);
            }
        }
        EvictionStats {
            candidate_count,
            evicted_count,
            evicted_recent_count,
            before_count,
            after_count: self.entries.len(),
            before_bytes,
            after_bytes: self.current_bytes,
            evicted_bytes,
        }
    }
}

struct GpuChunkBuffer {
    vertex_buffer: wgpu::Buffer,
    solid_index_buffer: wgpu::Buffer,
    translucent_index_buffer: Option<wgpu::Buffer>,
    vertex_count: u32,
    solid_index_count: u32,
    translucent_index_count: u32,
}

struct ResidentChunkMesh {
    key: ChunkKey,
    bounds: ChunkBounds,
    gpu: GpuChunkBuffer,
    last_touched_frame: u64,
    estimated_bytes: usize,
}

struct PendingChunkUpload {
    key: ChunkKey,
    enqueued_frame: u64,
    vertex_count: usize,
    index_count: usize,
    priority: QueuePriority,
}

#[derive(Debug, Clone, Copy)]
struct PendingChunkBuild {
    key: ChunkKey,
    enqueued_frame: u64,
    estimated_vertex_count: usize,
    estimated_index_count: usize,
    priority: QueuePriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueuePriority {
    Target,
    Preload,
}

#[derive(Debug, Clone, Copy)]
struct UploadBudget {
    max_chunks_per_frame: usize,
    max_vertices_per_frame: usize,
    max_indices_per_frame: usize,
}

impl Default for UploadBudget {
    fn default() -> Self {
        Self {
            max_chunks_per_frame: 48,
            max_vertices_per_frame: 1_500_000,
            max_indices_per_frame: 2_250_000,
        }
    }
}

impl UploadBudget {
    fn saturating_sub(self, other: Self) -> Self {
        Self {
            max_chunks_per_frame: self
                .max_chunks_per_frame
                .saturating_sub(other.max_chunks_per_frame),
            max_vertices_per_frame: self
                .max_vertices_per_frame
                .saturating_sub(other.max_vertices_per_frame),
            max_indices_per_frame: self
                .max_indices_per_frame
                .saturating_sub(other.max_indices_per_frame),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BuildBudget {
    max_chunks_per_frame: usize,
    max_vertices_per_frame: usize,
    max_indices_per_frame: usize,
}

impl Default for BuildBudget {
    fn default() -> Self {
        Self {
            max_chunks_per_frame: 24,
            max_vertices_per_frame: 900_000,
            max_indices_per_frame: 1_350_000,
        }
    }
}

impl BuildBudget {
    fn saturating_sub(self, other: Self) -> Self {
        Self {
            max_chunks_per_frame: self
                .max_chunks_per_frame
                .saturating_sub(other.max_chunks_per_frame),
            max_vertices_per_frame: self
                .max_vertices_per_frame
                .saturating_sub(other.max_vertices_per_frame),
            max_indices_per_frame: self
                .max_indices_per_frame
                .saturating_sub(other.max_indices_per_frame),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct BudgetUsage {
    chunks: usize,
    vertices: usize,
    indices: usize,
}

impl BudgetUsage {
    fn can_fit(
        self,
        vertex_count: usize,
        index_count: usize,
        max_chunks: usize,
        max_vertices: usize,
        max_indices: usize,
    ) -> bool {
        self.chunks < max_chunks
            && self.vertices + vertex_count <= max_vertices
            && self.indices + index_count <= max_indices
    }

    fn consume(&mut self, vertex_count: usize, index_count: usize) {
        self.chunks += 1;
        self.vertices += vertex_count;
        self.indices += index_count;
    }
}

#[derive(Debug, Clone, Copy)]
struct ChunkStreamingConfig {
    base_radius_chunks: i32,
    max_radius_chunks: i32,
    preload_margin_chunks: i32,
    release_margin_chunks: i32,
    preload_ring_chunks: i32,
    preload_near_margin_chunks: i32,
    preload_forward_dot_min: f32,
    preload_forward_prefer_dot_min: f32,
    max_preload_chunks: usize,
    preload_build_soft_budget: BuildBudget,
    preload_upload_soft_budget: UploadBudget,
    frustum_aabb_margin_blocks: f32,
    frustum_keep_near_chunks: i32,
    sticky_target_frames: u64,
    startup_target_chunk_limit: usize,
    steady_target_chunk_limit: usize,
    background_fill_start_frame: u64,
    background_fill_log_interval_frames: u64,
    warm_cache_frames: u64,
}

impl Default for ChunkStreamingConfig {
    fn default() -> Self {
        Self {
            base_radius_chunks: 2,
            max_radius_chunks: 8,
            preload_margin_chunks: 1,
            release_margin_chunks: 3,
            preload_ring_chunks: 2,
            preload_near_margin_chunks: 1,
            preload_forward_dot_min: 0.2,
            preload_forward_prefer_dot_min: 0.55,
            max_preload_chunks: 4096,
            preload_build_soft_budget: BuildBudget {
                max_chunks_per_frame: 2,
                max_vertices_per_frame: 72_000,
                max_indices_per_frame: 108_000,
            },
            preload_upload_soft_budget: UploadBudget {
                max_chunks_per_frame: 4,
                max_vertices_per_frame: 120_000,
                max_indices_per_frame: 180_000,
            },
            frustum_aabb_margin_blocks: 24.0,
            frustum_keep_near_chunks: 4,
            sticky_target_frames: 12,
            startup_target_chunk_limit: 160,
            steady_target_chunk_limit: 640,
            background_fill_start_frame: 12,
            background_fill_log_interval_frames: 30,
            warm_cache_frames: 300,
        }
    }
}

impl ChunkStreamingConfig {
    fn target_radius_chunks(
        &self,
        camera: &OrbitCamera,
        chunk_size: u32,
        scene_horizontal_radius_chunks: i32,
    ) -> i32 {
        let dynamic_radius = (camera.distance / chunk_size.max(1) as f32).ceil() as i32;
        let soft_max_radius = self
            .max_radius_chunks
            .max(scene_horizontal_radius_chunks + 1);
        (dynamic_radius + self.preload_margin_chunks)
            .clamp(self.base_radius_chunks, soft_max_radius)
    }

    fn target_chunk_limit(&self, frame_index: u64) -> usize {
        if frame_index < self.background_fill_start_frame {
            self.startup_target_chunk_limit
        } else {
            self.steady_target_chunk_limit
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FrustumPlane {
    normal: Vec3,
    distance: f32,
}

impl FrustumPlane {
    fn from_components(x: f32, y: f32, z: f32, w: f32) -> Option<Self> {
        let normal = Vec3::new(x, y, z);
        let length = normal.length();
        if length <= f32::EPSILON {
            return None;
        }
        Some(Self {
            normal: normal / length,
            distance: w / length,
        })
    }

    fn signed_distance(&self, point: Vec3) -> f32 {
        self.normal.dot(point) + self.distance
    }
}

#[derive(Debug, Clone, Copy)]
struct ViewFrustum {
    planes: [FrustumPlane; 6],
}

impl ViewFrustum {
    fn from_view_projection(view_proj: Mat4) -> Option<Self> {
        let cols = view_proj.to_cols_array_2d();
        let row0 = [cols[0][0], cols[1][0], cols[2][0], cols[3][0]];
        let row1 = [cols[0][1], cols[1][1], cols[2][1], cols[3][1]];
        let row2 = [cols[0][2], cols[1][2], cols[2][2], cols[3][2]];
        let row3 = [cols[0][3], cols[1][3], cols[2][3], cols[3][3]];
        Some(Self {
            planes: [
                FrustumPlane::from_components(
                    row3[0] + row0[0],
                    row3[1] + row0[1],
                    row3[2] + row0[2],
                    row3[3] + row0[3],
                )?,
                FrustumPlane::from_components(
                    row3[0] - row0[0],
                    row3[1] - row0[1],
                    row3[2] - row0[2],
                    row3[3] - row0[3],
                )?,
                FrustumPlane::from_components(
                    row3[0] + row1[0],
                    row3[1] + row1[1],
                    row3[2] + row1[2],
                    row3[3] + row1[3],
                )?,
                FrustumPlane::from_components(
                    row3[0] - row1[0],
                    row3[1] - row1[1],
                    row3[2] - row1[2],
                    row3[3] - row1[3],
                )?,
                FrustumPlane::from_components(row2[0], row2[1], row2[2], row2[3])?,
                FrustumPlane::from_components(
                    row3[0] - row2[0],
                    row3[1] - row2[1],
                    row3[2] - row2[2],
                    row3[3] - row2[3],
                )?,
            ],
        })
    }
}

fn chunk_visible_in_frustum(
    frustum: &ViewFrustum,
    bounds: ChunkBounds,
    extra_margin_blocks: f32,
) -> bool {
    let expanded_bounds = bounds.expanded(extra_margin_blocks);
    frustum.planes.iter().all(|plane| {
        let positive_vertex = expanded_bounds.positive_vertex(plane.normal);
        plane.signed_distance(positive_vertex) >= 0.0
    })
}

struct ChunkResidencyPlan {
    focus_chunk: ChunkKey,
    target_radius_chunks: i32,
    retain_radius_chunks: i32,
    preload_radius_chunks: i32,
    coarse_target_chunks: usize,
    visible_target_chunks: usize,
    target_chunks: usize,
    background_visible_chunks: usize,
    retain_chunks: usize,
    preload_chunks: usize,
    preload_forward_selected: usize,
    target_keys: HashSet<ChunkKey>,
    retain_keys: HashSet<ChunkKey>,
    preload_keys: HashSet<ChunkKey>,
    upload_order: Vec<ChunkKey>,
    preload_order: Vec<ChunkKey>,
}

impl ChunkResidencyPlan {
    fn build(
        scene_index: &ChunkSceneIndex,
        camera: &OrbitCamera,
        config: &ChunkStreamingConfig,
        frame_index: u64,
        background_fill_enabled: bool,
        force_full_scene: bool,
    ) -> Self {
        let focus_chunk =
            ChunkKey::from_world_position(camera.target.to_array(), scene_index.chunk_size());
        let scene_horizontal_radius_chunks = (((scene_index
            .metadata()
            .enclosing_size
            .x
            .max(scene_index.metadata().enclosing_size.z))
            as f32)
            / scene_index.chunk_size().max(1) as f32
            * 0.5)
            .ceil() as i32;
        let target_radius_chunks = if force_full_scene {
            scene_horizontal_radius_chunks.max(1)
        } else {
            config.target_radius_chunks(
                camera,
                scene_index.chunk_size(),
                scene_horizontal_radius_chunks,
            )
        };
        let retain_radius_chunks = if force_full_scene {
            target_radius_chunks
        } else {
            target_radius_chunks + config.release_margin_chunks
        };
        let preload_radius_chunks = if force_full_scene {
            retain_radius_chunks
        } else {
            retain_radius_chunks + config.preload_ring_chunks
        };
        let frustum = ViewFrustum::from_view_projection(camera.view_proj_matrix());
        let keep_near_radius = config.frustum_keep_near_chunks;
        let preload_near_radius = target_radius_chunks + config.preload_near_margin_chunks;
        let forward = (camera.target - camera.eye()).normalize_or_zero();

        let mut coarse_target_with_distance = Vec::<(i32, ChunkKey)>::new();
        let mut preload_with_score = Vec::<(i32, i32, i32, ChunkKey)>::new();
        let mut preload_forward_selected = 0_usize;
        let mut retain_keys = HashSet::<ChunkKey>::new();
        let full_scene_horizontal = target_radius_chunks >= scene_horizontal_radius_chunks;
        for entry in scene_index.chunk_entries() {
            let horizontal_distance = entry.key.horizontal_axis_distance(focus_chunk);
            if horizontal_distance <= retain_radius_chunks {
                retain_keys.insert(entry.key);
            }
            if horizontal_distance <= target_radius_chunks {
                coarse_target_with_distance.push((horizontal_distance, entry.key));
                continue;
            }
            if horizontal_distance > preload_radius_chunks {
                continue;
            }

            let bounds = ChunkBounds::from_key(entry.key, scene_index.chunk_size());
            let to_chunk = (bounds.center - camera.target).normalize_or_zero();
            let forward_dot = forward.dot(to_chunk);
            let in_preload_shell = horizontal_distance <= preload_near_radius;
            let in_forward_region = forward_dot >= config.preload_forward_dot_min;
            if in_preload_shell || in_forward_region {
                let strongly_forward = forward_dot >= config.preload_forward_prefer_dot_min;
                if strongly_forward {
                    preload_forward_selected += 1;
                }
                let forward_group = if strongly_forward {
                    0
                } else if in_forward_region {
                    1
                } else {
                    2
                };
                let ring_distance = horizontal_distance.saturating_sub(target_radius_chunks);
                let direction_bias = (forward_dot * 1000.0) as i32;
                preload_with_score.push((forward_group, ring_distance, -direction_bias, entry.key));
            }
        }

        if force_full_scene {
            let upload_order = scene_index
                .chunk_entries()
                .iter()
                .map(|entry| entry.key)
                .collect::<Vec<_>>();
            let target_keys = upload_order.iter().copied().collect::<HashSet<_>>();
            let retain_keys = target_keys.clone();
            return Self {
                focus_chunk,
                target_radius_chunks,
                retain_radius_chunks,
                preload_radius_chunks,
                coarse_target_chunks: scene_index.chunk_count(),
                visible_target_chunks: scene_index.chunk_count(),
                target_chunks: upload_order.len(),
                background_visible_chunks: 0,
                retain_chunks: retain_keys.len(),
                preload_chunks: 0,
                preload_forward_selected: 0,
                target_keys,
                retain_keys,
                preload_keys: HashSet::new(),
                upload_order,
                preload_order: Vec::new(),
            };
        }

        let coarse_target_chunks = coarse_target_with_distance.len();
        let retain_chunks = retain_keys.len();
        let mut target_with_distance = Vec::<(i32, ChunkKey)>::new();
        for (horizontal_distance, key) in coarse_target_with_distance {
            let keep_near = horizontal_distance <= keep_near_radius;
            let visible = if full_scene_horizontal {
                true
            } else {
                frustum
                    .as_ref()
                    .map(|frustum| {
                        chunk_visible_in_frustum(
                            frustum,
                            ChunkBounds::from_key(key, scene_index.chunk_size()),
                            config.frustum_aabb_margin_blocks,
                        )
                    })
                    .unwrap_or(true)
            };
            if keep_near || visible {
                target_with_distance.push((horizontal_distance, key));
            }
        }

        target_with_distance.sort_by_key(|(horizontal_distance, key)| {
            (*horizontal_distance, key.cy, key.cx, key.cz)
        });
        let visible_target_chunks = target_with_distance.len();
        let target_chunk_limit = config
            .target_chunk_limit(frame_index)
            .min(target_with_distance.len());
        let upload_order = target_with_distance
            .iter()
            .take(target_chunk_limit)
            .map(|(_, key)| *key)
            .collect::<Vec<_>>();
        let background_visible_order = target_with_distance
            .iter()
            .skip(target_chunk_limit)
            .map(|(_, key)| *key)
            .collect::<Vec<_>>();
        let target_keys = upload_order.iter().copied().collect::<HashSet<_>>();
        preload_with_score.sort_by_key(|(forward_group, ring_distance, direction_bias, key)| {
            (
                *forward_group,
                *ring_distance,
                *direction_bias,
                key.cx,
                key.cy,
                key.cz,
            )
        });
        let mut preload_order = Vec::new();
        if background_fill_enabled {
            preload_order.extend(background_visible_order.iter().copied());
            preload_order.extend(
                preload_with_score
                    .into_iter()
                    .filter_map(|(_, _, _, key)| {
                        (!target_keys.contains(&key) && !background_visible_order.contains(&key))
                            .then_some(key)
                    })
                    .take(
                        config
                            .max_preload_chunks
                            .saturating_sub(preload_order.len()),
                    ),
            );
        }
        let preload_keys = preload_order.iter().copied().collect::<HashSet<_>>();
        Self {
            focus_chunk,
            target_radius_chunks,
            retain_radius_chunks,
            preload_radius_chunks,
            coarse_target_chunks,
            visible_target_chunks,
            target_chunks: upload_order.len(),
            background_visible_chunks: background_visible_order.len(),
            retain_chunks,
            preload_chunks: preload_order.len(),
            preload_forward_selected,
            target_keys,
            retain_keys,
            preload_keys,
            upload_order,
            preload_order,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct OrbitCamera {
    target: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
    min_distance: f32,
    max_distance: f32,
    aspect: f32,
    fovy_radians: f32,
    near: f32,
    far: f32,
}

impl OrbitCamera {
    fn fit_distance(bounds: SceneBounds) -> f32 {
        bounds.radius.max(2.0) * 2.5
    }

    fn min_distance_for_bounds(bounds: SceneBounds) -> f32 {
        (bounds.radius.max(2.0) * 0.18).max(0.75)
    }

    fn max_distance_for_bounds(bounds: SceneBounds) -> f32 {
        bounds.radius.max(2.0) * 10.0
    }

    fn from_bounds(bounds: SceneBounds, width: u32, height: u32) -> Self {
        let aspect = if height == 0 {
            1.0
        } else {
            width as f32 / height as f32
        };
        let radius = bounds.radius.max(2.0);
        Self {
            target: bounds.center,
            yaw: 0.8,
            pitch: 0.6,
            distance: Self::fit_distance(bounds),
            min_distance: Self::min_distance_for_bounds(bounds),
            max_distance: Self::max_distance_for_bounds(bounds),
            aspect,
            fovy_radians: FRAC_PI_4,
            near: (radius / 512.0).max(0.05),
            far: (radius * 16.0).max(256.0),
        }
    }

    fn reset(&mut self, bounds: SceneBounds, width: u32, height: u32) {
        *self = Self::from_bounds(bounds, width, height);
    }

    fn update_aspect(&mut self, width: u32, height: u32) {
        self.aspect = if height == 0 {
            1.0
        } else {
            width as f32 / height as f32
        };
    }

    fn eye(&self) -> Vec3 {
        self.target + Self::offset_from_angles(self.yaw, self.pitch, self.distance)
    }

    fn offset_from_angles(yaw: f32, pitch: f32, distance: f32) -> Vec3 {
        let sin_pitch = pitch.sin();
        let cos_pitch = pitch.cos();
        let sin_yaw = yaw.sin();
        let cos_yaw = yaw.cos();
        Vec3::new(
            distance * cos_pitch * sin_yaw,
            distance * sin_pitch,
            distance * cos_pitch * cos_yaw,
        )
    }

    fn view_proj_matrix(&self) -> Mat4 {
        let view = Mat4::look_at_rh(self.eye(), self.target, Vec3::Y);
        let proj = Mat4::perspective_rh(
            self.fovy_radians,
            self.aspect.max(0.01),
            self.near,
            self.far,
        );
        proj * view
    }

    fn orbit(&mut self, delta: Vec2) {
        self.orbit_around_target(delta);
    }

    fn orbit_around_target(&mut self, delta: Vec2) {
        self.yaw -= delta.x * 0.01;
        self.pitch = (self.pitch + delta.y * 0.01).clamp(-1.54, 1.54);
    }

    fn pan(&mut self, delta: Vec2) {
        let eye = self.eye();
        let forward = (self.target - eye).normalize_or_zero();
        let right = forward.cross(Vec3::Y).normalize_or_zero();
        let up = right.cross(forward).normalize_or_zero();
        let scale = self.distance * 0.0015;
        self.target += (-right * delta.x + up * delta.y) * scale;
    }

    fn zoom(&mut self, amount: f32) {
        if amount.abs() <= f32::EPSILON {
            return;
        }

        let next = self.distance * (1.0 - amount * 0.12);
        if amount < 0.0 {
            self.distance = next.clamp(self.min_distance, self.max_distance);
            return;
        }

        if next >= self.min_distance {
            self.distance = next.clamp(self.min_distance, self.max_distance);
            return;
        }

        let forward = (self.target - self.eye()).normalize_or_zero();
        let overshoot = (self.min_distance - next).max(0.0);
        self.distance = self.min_distance;
        if forward.length_squared() > f32::EPSILON {
            self.target += forward * overshoot;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragMode {
    Rotate,
    Pan,
}

struct DepthTexture {
    view: wgpu::TextureView,
}

impl DepthTexture {
    fn create(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("native_viewer_depth"),
            size: wgpu::Extent3d {
                width: config.width.max(1),
                height: config.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth24Plus,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { view }
    }
}

struct ShadowMapResources {
    config: ShadowMapConfig,
    depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
    depth_pipeline: wgpu::RenderPipeline,
    camera_bind_group: wgpu::BindGroup,
    bind_group: wgpu::BindGroup,
}

fn create_shadow_map_resources(
    device: &wgpu::Device,
    config: ShadowMapConfig,
    shadow_bind_group_layout: &wgpu::BindGroupLayout,
    camera_bind_group_layout: &wgpu::BindGroupLayout,
    depth_shader: &wgpu::ShaderModule,
) -> ShadowMapResources {
    let resolution = config.resolution.max(1);
    let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("native_viewer_shadow_depth"),
        size: wgpu::Extent3d {
            width: resolution,
            height: resolution,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("native_viewer_shadow_sampler"),
        compare: Some(wgpu::CompareFunction::LessEqual),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        ..Default::default()
    });
    let camera_uniform = CameraUniform {
        view_proj: config
            .light_camera
            .map(|camera| camera.view_proj)
            .unwrap_or(Mat4::IDENTITY)
            .to_cols_array_2d(),
        camera_position: [0.0, 0.0, 0.0, 0.0],
    };
    let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("native_viewer_shadow_camera"),
        contents: bytemuck::bytes_of(&camera_uniform),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("native_viewer_shadow_camera_bind_group"),
        layout: camera_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: camera_buffer.as_entire_binding(),
        }],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("native_viewer_shadow_bind_group"),
        layout: shadow_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&depth_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("native_viewer_shadow_depth_pipeline_layout"),
        bind_group_layouts: &[camera_bind_group_layout],
        push_constant_ranges: &[],
    });
    let depth_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("native_viewer_shadow_depth_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: depth_shader,
            entry_point: "vs_main",
            buffers: &[GpuVertex::desc()],
        },
        fragment: None,
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    });
    ShadowMapResources {
        config,
        depth_texture,
        depth_view,
        depth_pipeline,
        camera_bind_group,
        bind_group,
    }
}

fn create_atlas_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    atlas: Option<&RgbaImage>,
) -> wgpu::BindGroup {
    let used_runtime_atlas = atlas.is_some();
    let atlas = atlas
        .cloned()
        .unwrap_or_else(|| RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 255])));
    let width = atlas.width().max(1);
    let height = atlas.height().max(1);
    println!(
        "[VIEWER_TEXTURE] atlas_upload source={} width={} height={} bytes={}",
        if used_runtime_atlas {
            "runtime_v2"
        } else {
            "default_white"
        },
        width,
        height,
        atlas.as_raw().len()
    );
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("native_viewer_atlas"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        atlas.as_raw(),
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("native_viewer_atlas_sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("native_viewer_atlas_bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    })
}

#[derive(Debug, Clone, Copy)]
struct BasicLightingTuning {
    ambient_strength: f32,
    directional_strength: f32,
    min_brightness_floor: f32,
    light_direction: [f32; 3],
}

#[derive(Debug, Clone, Copy)]
struct BasicLightingConfig {
    enabled: bool,
    tuning: BasicLightingTuning,
}

impl BasicLightingConfig {
    const FROZEN_BASELINE_TUNING: BasicLightingTuning = BasicLightingTuning {
        ambient_strength: 0.76,
        directional_strength: 0.24,
        min_brightness_floor: 0.80,
        light_direction: [0.55, 1.0, 0.35],
    };

    fn from_enabled(enabled: bool) -> Self {
        Self {
            enabled,
            tuning: BasicLightingTuning {
                light_direction: normalize_lighting_direction(
                    Self::FROZEN_BASELINE_TUNING.light_direction,
                ),
                ..Self::FROZEN_BASELINE_TUNING
            },
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
struct LightingConfig {
    basic: BasicLightingConfig,
    shadow: ShadowMapConfig,
    ao: AoConfig,
    tone: ToneConfig,
    emissive_bloom: EmissiveBloomConfig,
}

impl LightingConfig {
    fn from_args(
        basic_enabled: bool,
        shadow_requested: bool,
        shadow_debug_view: ShadowDebugViewMode,
        shadow_force_test: bool,
        scene_bounds: SceneBounds,
    ) -> Self {
        let basic = BasicLightingConfig::from_enabled(basic_enabled);
        let shadow = ShadowMapConfig::from_flags(
            basic_enabled && shadow_requested,
            basic.tuning.light_direction,
            shadow_debug_view,
            shadow_force_test,
            scene_bounds,
        );
        Self {
            basic,
            shadow,
            ao: AoConfig::from_env(),
            tone: ToneConfig::from_env(),
            emissive_bloom: EmissiveBloomConfig::from_env(),
        }
    }

    fn basic(self) -> BasicLightingConfig {
        self.basic
    }

    fn shadow(self) -> ShadowMapConfig {
        self.shadow
    }

    fn ao(self) -> AoConfig {
        self.ao
    }

    fn tone(self) -> ToneConfig {
        self.tone
    }

    fn emissive_bloom(self) -> EmissiveBloomConfig {
        self.emissive_bloom
    }
}

fn env_on_off(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on" | "yes"
            )
        })
        .unwrap_or(default)
}

fn env_f32(name: &str, default: f32, min: f32, max: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(default)
        .clamp(min, max)
}

fn env_usize(name: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(default)
        .clamp(min, max)
}

fn normalize_lighting_direction(direction: [f32; 3]) -> [f32; 3] {
    let vector = Vec3::new(direction[0], direction[1], direction[2]).normalize_or_zero();
    if vector.length_squared() <= f32::EPSILON {
        [0.55, 1.0, 0.35]
    } else {
        [vector.x, vector.y, vector.z]
    }
}

fn apply_basic_lighting(
    rgb: [f32; 3],
    normal: [f32; 3],
    lighting: BasicLightingConfig,
) -> [f32; 3] {
    if !lighting.enabled {
        return rgb;
    }
    let n = Vec3::new(normal[0], normal[1], normal[2]).normalize_or_zero();
    let l = Vec3::from_array(lighting.tuning.light_direction).normalize_or_zero();
    let diffuse = n.dot(l).max(0.0);
    let factor = (lighting.tuning.ambient_strength
        + diffuse * lighting.tuning.directional_strength)
        .max(lighting.tuning.min_brightness_floor)
        .clamp(0.0, 1.0);
    [rgb[0] * factor, rgb[1] * factor, rgb[2] * factor]
}

#[derive(Debug, Clone, Copy)]
struct ShadowLightCamera {
    view_proj: Mat4,
    light_bounds_min: Vec3,
    light_bounds_max: Vec3,
    near: f32,
    far: f32,
    width: f32,
    height: f32,
    depth: f32,
    world_units_per_texel: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShadowDebugViewMode {
    Off,
    Factor,
    Frustum,
    ReceiverDepth,
    ShadowMapDepth,
    Raw,
    Final,
}

impl ShadowDebugViewMode {
    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "factor" | "shadow" => Self::Factor,
            "frustum" => Self::Frustum,
            "receiver-depth" | "receiver_depth" => Self::ReceiverDepth,
            "shadow-map-depth" | "shadow_map_depth" => Self::ShadowMapDepth,
            "raw" => Self::Raw,
            "final" => Self::Final,
            _ => Self::Off,
        }
    }

    fn shader_code(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Factor => 1.0,
            Self::Frustum => 2.0,
            Self::ReceiverDepth => 3.0,
            Self::ShadowMapDepth => 4.0,
            Self::Raw => 5.0,
            Self::Final => 6.0,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Factor => "factor",
            Self::Frustum => "frustum",
            Self::ReceiverDepth => "receiver-depth",
            Self::ShadowMapDepth => "shadow-map-depth",
            Self::Raw => "raw",
            Self::Final => "final",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShadowLightingPreset {
    Soft,
}

impl ShadowLightingPreset {
    fn from_env() -> Self {
        Self::Soft
    }
    fn shader_code(self) -> f32 {
        1.0
    }
    fn label(self) -> &'static str {
        "soft"
    }
    fn tuning(self) -> ShadowLightingTuning {
        ShadowLightingTuning {
            final_min: 0.36,
            final_max: 0.77,
            hemisphere_ground: 0.54,
            hemisphere_sky: 0.75,
            top_lift_strength: 0.052,
            bottom_shade_strength: 0.062,
            side_layer_x: 0.020,
            side_layer_z: -0.014,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ShadowLightingTuning {
    final_min: f32,
    final_max: f32,
    hemisphere_ground: f32,
    hemisphere_sky: f32,
    top_lift_strength: f32,
    bottom_shade_strength: f32,
    side_layer_x: f32,
    side_layer_z: f32,
}

#[derive(Debug, Clone, Copy)]
struct ShadowPcfConfig {
    samples: u32,
    radius: f32,
}

impl ShadowPcfConfig {
    fn from_env() -> Self {
        Self {
            samples: env_usize("LBA_SHADOW_PCF", 8, 0, 12) as u32,
            radius: env_f32("LBA_SHADOW_PCF_RADIUS", 1.25, 0.0, 8.0),
        }
    }
    fn label(self) -> &'static str {
        match self.samples {
            0 => "off",
            8 => "pcf8",
            _ => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ShadowFrustumConfig {
    padding: f32,
    mode: ShadowFrustumMode,
}

#[derive(Debug, Clone, Copy)]
struct ShadowFrustumMode;

impl ShadowFrustumMode {
    fn label(self) -> &'static str {
        "scene_bounds"
    }
}

#[derive(Debug, Clone, Copy)]
struct ShadowMapConfig {
    enabled: bool,
    strength: f32,
    bias: f32,
    resolution: u32,
    pcf: ShadowPcfConfig,
    debug_view: ShadowDebugViewMode,
    force_test: bool,
    lighting_preset: ShadowLightingPreset,
    frustum: ShadowFrustumConfig,
    scene_bounds: SceneBounds,
    light_direction: [f32; 3],
    light_camera: Option<ShadowLightCamera>,
}

impl ShadowMapConfig {
    fn from_flags(
        enabled: bool,
        light_direction: [f32; 3],
        debug_view: ShadowDebugViewMode,
        force_test: bool,
        scene_bounds: SceneBounds,
    ) -> Self {
        let resolution = env_usize("LBA_SHADOW_MAP_SIZE", 2048, 256, 8192) as u32;
        let direction = normalize_lighting_direction(light_direction);
        let light_camera =
            enabled.then(|| build_shadow_light_camera(scene_bounds, direction, resolution));
        Self {
            enabled,
            strength: env_f32("LBA_SHADOW_STRENGTH", 0.72, 0.0, 1.0),
            bias: env_f32("LBA_SHADOW_BIAS", 0.00035, 0.0, 0.05),
            resolution,
            pcf: ShadowPcfConfig::from_env(),
            debug_view,
            force_test,
            lighting_preset: ShadowLightingPreset::from_env(),
            frustum: ShadowFrustumConfig {
                padding: 4.0,
                mode: ShadowFrustumMode,
            },
            scene_bounds,
            light_direction: direction,
            light_camera,
        }
    }
}

fn build_shadow_light_camera(
    bounds: SceneBounds,
    direction: [f32; 3],
    resolution: u32,
) -> ShadowLightCamera {
    let dir = Vec3::from_array(direction).normalize_or_zero();
    let eye = bounds.center - dir * bounds.radius.max(8.0) * 2.0;
    let view = Mat4::look_at_rh(eye, bounds.center, Vec3::Y);
    let extent = bounds.radius.max(4.0) + 4.0;
    let near = 0.1;
    let far = extent * 4.0;
    let proj = Mat4::orthographic_rh(-extent, extent, -extent, extent, near, far);
    ShadowLightCamera {
        view_proj: proj * view,
        light_bounds_min: bounds.center - Vec3::splat(extent),
        light_bounds_max: bounds.center + Vec3::splat(extent),
        near,
        far,
        width: extent * 2.0,
        height: extent * 2.0,
        depth: far - near,
        world_units_per_texel: (extent * 2.0) / resolution.max(1) as f32,
    }
}

#[derive(Debug, Clone, Copy)]
struct ShadowDepthPassPlan {
    enabled: bool,
}

impl ShadowDepthPassPlan {
    fn from_config(config: ShadowMapConfig) -> Self {
        Self {
            enabled: config.enabled && config.light_camera.is_some(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct EmissiveBloomConfig {
    emissive_enabled: bool,
    emissive_strength: f32,
    bloom_enabled: bool,
    bloom_threshold: f32,
    bloom_intensity: f32,
    bloom_radius: f32,
    emissive_debug_view: EmissiveDebugViewMode,
    bloom_debug_view: BloomDebugViewMode,
}

impl EmissiveBloomConfig {
    fn from_env() -> Self {
        Self {
            emissive_enabled: env_on_off("LBA_EMISSIVE", true),
            emissive_strength: env_f32("LBA_EMISSIVE_STRENGTH", 1.45, 1.0, 4.0),
            bloom_enabled: env_on_off("LBA_BLOOM", true),
            bloom_threshold: env_f32("LBA_BLOOM_THRESHOLD", 0.78, 0.0, 2.0),
            bloom_intensity: env_f32("LBA_BLOOM_INTENSITY", 0.30, 0.0, 2.0),
            bloom_radius: env_f32("LBA_BLOOM_RADIUS", 1.20, 0.25, 4.0),
            emissive_debug_view: EmissiveDebugViewMode::from_env(),
            bloom_debug_view: BloomDebugViewMode::from_env(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmissiveDebugViewMode {
    Off,
    Mask,
    Material,
    Final,
}

impl EmissiveDebugViewMode {
    fn from_env() -> Self {
        match std::env::var("LBA_EMISSIVE_DEBUG_VIEW")
            .ok()
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("mask") => Self::Mask,
            Some("material") => Self::Material,
            Some("final") => Self::Final,
            _ => Self::Off,
        }
    }
    fn shader_code(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Mask => 1.0,
            Self::Material => 2.0,
            Self::Final => 3.0,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Mask => "mask",
            Self::Material => "material",
            Self::Final => "final",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BloomDebugViewMode {
    Off,
    Mask,
    Bloom,
    Final,
}

impl BloomDebugViewMode {
    fn from_env() -> Self {
        match std::env::var("LBA_BLOOM_DEBUG_VIEW")
            .ok()
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("mask") => Self::Mask,
            Some("bloom") => Self::Bloom,
            Some("final") => Self::Final,
            _ => Self::Off,
        }
    }
    fn shader_code(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Mask => 1.0,
            Self::Bloom => 2.0,
            Self::Final => 3.0,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Mask => "mask",
            Self::Bloom => "bloom",
            Self::Final => "final",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AoConfig {
    contact_enabled: bool,
    contact_strength: f32,
    ssao_enabled: bool,
    ssao_radius: f32,
    ssao_strength: f32,
    debug_view: AoDebugViewMode,
    distance_fade_start: f32,
    distance_fade_end: f32,
    edge_guard: f32,
    max_occlusion: f32,
}

impl AoConfig {
    const DEFAULT_CONTACT_STRENGTH: f32 = 0.20;
    const DEFAULT_SSAO_RADIUS: f32 = 0.75;
    const DEFAULT_SSAO_STRENGTH: f32 = 0.20;
    const DEFAULT_DISTANCE_FADE_START: f32 = 24.0;
    const DEFAULT_DISTANCE_FADE_END: f32 = 96.0;
    const DEFAULT_EDGE_GUARD: f32 = 0.45;
    const DEFAULT_MAX_OCCLUSION: f32 = 0.42;
    const SAMPLE_COUNT: u32 = 6;

    fn from_env() -> Self {
        Self {
            contact_enabled: env_on_off("LBA_CONTACT_SHADOWS", false),
            contact_strength: env_f32(
                "LBA_CONTACT_SHADOW_STRENGTH",
                Self::DEFAULT_CONTACT_STRENGTH,
                0.0,
                0.8,
            ),
            ssao_enabled: env_on_off("LBA_SSAO", false),
            ssao_radius: env_f32("LBA_SSAO_RADIUS", Self::DEFAULT_SSAO_RADIUS, 0.1, 3.0),
            ssao_strength: env_f32("LBA_SSAO_STRENGTH", Self::DEFAULT_SSAO_STRENGTH, 0.0, 0.8),
            debug_view: AoDebugViewMode::from_env(),
            distance_fade_start: env_f32(
                "LBA_AO_DISTANCE_FADE_START",
                Self::DEFAULT_DISTANCE_FADE_START,
                0.0,
                2048.0,
            ),
            distance_fade_end: env_f32(
                "LBA_AO_DISTANCE_FADE_END",
                Self::DEFAULT_DISTANCE_FADE_END,
                1.0,
                4096.0,
            ),
            edge_guard: env_f32("LBA_AO_EDGE_GUARD", Self::DEFAULT_EDGE_GUARD, 0.0, 1.0),
            max_occlusion: env_f32(
                "LBA_AO_MAX_OCCLUSION",
                Self::DEFAULT_MAX_OCCLUSION,
                0.0,
                0.8,
            ),
        }
    }

    fn combined_min_estimate(self) -> f32 {
        let contact_min = if self.contact_enabled {
            1.0 - 0.70 * self.contact_strength
        } else {
            1.0
        };
        let ssao_min = if self.ssao_enabled {
            1.0 - 0.50 * self.ssao_strength
        } else {
            1.0
        };
        (contact_min * ssao_min)
            .max(1.0 - self.max_occlusion)
            .clamp(0.0, 1.0)
    }

    fn combined_avg_estimate(self) -> f32 {
        ((self.combined_min_estimate() + 1.0) * 0.5).clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AoDebugViewMode {
    Off,
    Contact,
    Ssao,
    Combined,
}

impl AoDebugViewMode {
    fn from_env() -> Self {
        match std::env::var("LBA_AO_DEBUG_VIEW")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("contact") | Some("contact-shadow") | Some("contact_shadows") => Self::Contact,
            Some("ssao") => Self::Ssao,
            Some("combined") | Some("ao") => Self::Combined,
            Some("") | None => Self::Off,
            Some(_) => Self::Off,
        }
    }

    fn shader_code(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Contact => 1.0,
            Self::Ssao => 2.0,
            Self::Combined => 3.0,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Contact => "contact",
            Self::Ssao => "ssao",
            Self::Combined => "combined",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ToneConfig {
    preset: TonePreset,
    exposure: f32,
    gamma: f32,
    saturation: f32,
    contrast: f32,
    highlight_rolloff: f32,
    debug_view: ToneDebugViewMode,
}

impl ToneConfig {
    fn from_env() -> Self {
        let preset = TonePreset::from_env();
        let defaults = preset.defaults();
        Self {
            preset,
            exposure: env_f32("LBA_TONE_EXPOSURE", defaults.exposure, 0.25, 2.0),
            gamma: env_f32("LBA_TONE_GAMMA", defaults.gamma, 0.5, 3.0),
            saturation: env_f32("LBA_TONE_SATURATION", defaults.saturation, 0.0, 2.0),
            contrast: env_f32("LBA_TONE_CONTRAST", defaults.contrast, 0.5, 2.0),
            highlight_rolloff: env_f32(
                "LBA_TONE_HIGHLIGHT_ROLLOFF",
                defaults.highlight_rolloff,
                0.0,
                1.0,
            ),
            debug_view: ToneDebugViewMode::from_env(),
        }
    }

    fn estimated_range(self) -> (f32, f32) {
        if self.preset == TonePreset::Off {
            return (0.0, 1.0);
        }
        let low = tone_map_scalar(0.0, self);
        let high = tone_map_scalar(1.0, self);
        (low.min(high), low.max(high))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TonePreset {
    Off,
    Soft,
    Filmic,
    Contrast,
}

impl TonePreset {
    fn from_env() -> Self {
        match std::env::var("LBA_TONE_PRESET")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("off") | Some("none") | Some("0") | Some("false") => Self::Off,
            Some("filmic") => Self::Filmic,
            Some("contrast") => Self::Contrast,
            Some("soft") | Some("") | None => Self::Soft,
            Some(_) => Self::Soft,
        }
    }

    fn defaults(self) -> ToneDefaults {
        match self {
            Self::Off => ToneDefaults {
                exposure: 1.0,
                gamma: 1.0,
                saturation: 1.0,
                contrast: 1.0,
                highlight_rolloff: 0.0,
            },
            Self::Soft => ToneDefaults {
                exposure: 0.98,
                gamma: 1.0,
                saturation: 1.03,
                contrast: 1.04,
                highlight_rolloff: 0.25,
            },
            Self::Filmic => ToneDefaults {
                exposure: 0.96,
                gamma: 1.0,
                saturation: 1.04,
                contrast: 1.06,
                highlight_rolloff: 0.40,
            },
            Self::Contrast => ToneDefaults {
                exposure: 1.0,
                gamma: 1.0,
                saturation: 1.08,
                contrast: 1.10,
                highlight_rolloff: 0.28,
            },
        }
    }

    fn shader_code(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Soft => 1.0,
            Self::Filmic => 2.0,
            Self::Contrast => 3.0,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Soft => "soft",
            Self::Filmic => "filmic",
            Self::Contrast => "contrast",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ToneDefaults {
    exposure: f32,
    gamma: f32,
    saturation: f32,
    contrast: f32,
    highlight_rolloff: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToneDebugViewMode {
    Off,
    PreTone,
    PostTone,
    Luminance,
}

impl ToneDebugViewMode {
    fn from_env() -> Self {
        match std::env::var("LBA_TONE_DEBUG_VIEW")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("pre-tone") | Some("pretone") | Some("pre_tone") => Self::PreTone,
            Some("post-tone") | Some("posttone") | Some("post_tone") => Self::PostTone,
            Some("luminance") | Some("luma") => Self::Luminance,
            Some("") | None => Self::Off,
            Some(_) => Self::Off,
        }
    }

    fn shader_code(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::PreTone => 1.0,
            Self::PostTone => 2.0,
            Self::Luminance => 3.0,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::PreTone => "pre-tone",
            Self::PostTone => "post-tone",
            Self::Luminance => "luminance",
        }
    }
}

fn tone_map_scalar(value: f32, config: ToneConfig) -> f32 {
    let exposed = (value * config.exposure).max(0.0);
    let rolled = exposed / (1.0 + exposed * config.highlight_rolloff.clamp(0.0, 1.0));
    let contrasted = (rolled - 0.5) * config.contrast + 0.5;
    let gamma = config.gamma.max(0.01);
    contrasted.max(0.0).powf(1.0 / gamma).clamp(0.0, 1.0)
}

fn log_shadow_startup_debug(
    basic_lighting: bool,
    basic_shadows: bool,
    config: ShadowMapConfig,
    uniform: &LightingUniform,
) {
    let camera_valid = config.light_camera.is_some_and(|camera| {
        camera
            .view_proj
            .to_cols_array()
            .iter()
            .all(|value| value.is_finite())
    });
    let light_bounds = config
        .light_camera
        .map(|camera| {
            format!(
                "min=({:.2},{:.2},{:.2}) max=({:.2},{:.2},{:.2})",
                camera.light_bounds_min.x,
                camera.light_bounds_min.y,
                camera.light_bounds_min.z,
                camera.light_bounds_max.x,
                camera.light_bounds_max.y,
                camera.light_bounds_max.z,
            )
        })
        .unwrap_or_else(|| "none".to_string());
    let light_near = config.light_camera.map(|camera| camera.near).unwrap_or(0.0);
    let light_far = config.light_camera.map(|camera| camera.far).unwrap_or(0.0);
    let frustum_width = config
        .light_camera
        .map(|camera| camera.width)
        .unwrap_or(0.0);
    let frustum_height = config
        .light_camera
        .map(|camera| camera.height)
        .unwrap_or(0.0);
    let frustum_depth = config
        .light_camera
        .map(|camera| camera.depth)
        .unwrap_or(0.0);
    let world_units_per_shadow_texel = config
        .light_camera
        .map(|camera| camera.world_units_per_texel)
        .unwrap_or(0.0);
    let tuning = config.lighting_preset.tuning();
    let shadow_texel_size = if config.resolution == 0 {
        0.0
    } else {
        1.0 / config.resolution as f32
    };
    println!(
        "[LBA_SHADOW_DEBUG] stage=baseline basic_lighting={} basic_shadows_requested={} basic_shadows_effective={} preset={} final_min={:.3} final_max={:.3} map_size={} pcf_samples={} pcf_radius={:.3} bias={:.6} frustum_mode={} frustum_padding={:.3} world_units_per_shadow_texel={:.6} shadow_texel_uv={:.8}",
        basic_lighting,
        basic_shadows,
        config.enabled,
        config.lighting_preset.label(),
        tuning.final_min,
        tuning.final_max,
        config.resolution,
        config.pcf.label(),
        config.pcf.radius,
        config.bias,
        config.frustum.mode.label(),
        config.frustum.padding,
        world_units_per_shadow_texel,
        shadow_texel_size,
    );
    println!(
        "[LBA_SHADOW_DEBUG] stage=frustum scene_center=({:.2},{:.2},{:.2}) scene_radius={:.2} light_dir=({:.4},{:.4},{:.4}) light_bounds={} near={:.3} far={:.3} width={:.3} height={:.3} depth={:.3} light_vp_valid={} shader_shadow_enabled={:.1} shader_pcf_samples={:.1} shader_pcf_radius={:.3} debug_view={} debug_view_code={:.1} force_test={}",
        config.scene_bounds.center.x,
        config.scene_bounds.center.y,
        config.scene_bounds.center.z,
        config.scene_bounds.radius,
        config.light_direction[0],
        config.light_direction[1],
        config.light_direction[2],
        light_bounds,
        light_near,
        light_far,
        frustum_width,
        frustum_height,
        frustum_depth,
        camera_valid,
        uniform.shadow_enabled,
        uniform.shadow_pcf_samples,
        uniform.shadow_pcf_radius,
        config.debug_view.label(),
        uniform.shadow_debug_view_mode,
        config.force_test,
    );
}

fn log_ao_startup_debug(config: AoConfig, shadow: ShadowMapConfig) {
    let tuning = shadow.lighting_preset.tuning();
    println!(
        "[LBA_AO_DEBUG] contact_effective={} contact_strength={:.3} ssao_effective={} ssao_radius={:.3} ssao_strength={:.3} distance_fade_start={:.3} distance_fade_end={:.3} edge_guard={:.3} max_occlusion={:.3} sample_count={} debug_view={} combined_ao_min={:.3} combined_ao_max=1.000 combined_ao_avg_est={:.3} hemisphere_ground={:.3} hemisphere_sky={:.3} top_lift={:.3} bottom_shade={:.3} side_bias_x={:.3} side_bias_z={:.3}",
        config.contact_enabled,
        config.contact_strength,
        config.ssao_enabled,
        config.ssao_radius,
        config.ssao_strength,
        config.distance_fade_start,
        config
            .distance_fade_end
            .max(config.distance_fade_start + 1.0),
        config.edge_guard,
        config.max_occlusion,
        AoConfig::SAMPLE_COUNT,
        config.debug_view.label(),
        config.combined_min_estimate(),
        config.combined_avg_estimate(),
        tuning.hemisphere_ground,
        tuning.hemisphere_sky,
        tuning.top_lift_strength,
        tuning.bottom_shade_strength,
        tuning.side_layer_x,
        tuning.side_layer_z,
    );
}

fn log_ao_smoke_check(config: AoConfig) {
    let fade_end = config
        .distance_fade_end
        .max(config.distance_fade_start + 1.0);
    let pass = config.contact_strength.is_finite()
        && config.ssao_radius.is_finite()
        && config.ssao_strength.is_finite()
        && config.distance_fade_start.is_finite()
        && fade_end.is_finite()
        && config.edge_guard.is_finite()
        && config.max_occlusion.is_finite()
        && fade_end > config.distance_fade_start
        && config.combined_min_estimate().is_finite();
    println!(
        "[LBA_AO_SMOKE] pass={} contact_effective={} ssao_effective={} contact_strength={:.3} ssao_radius={:.3} ssao_strength={:.3} fade=({:.3},{:.3}) edge_guard={:.3} max_occlusion={:.3} combined_ao_min={:.3}",
        pass,
        config.contact_enabled,
        config.ssao_enabled,
        config.contact_strength,
        config.ssao_radius,
        config.ssao_strength,
        config.distance_fade_start,
        fade_end,
        config.edge_guard,
        config.max_occlusion,
        config.combined_min_estimate(),
    );
}

fn log_tone_startup_debug(config: ToneConfig) {
    let (range_min, range_max) = config.estimated_range();
    println!(
        "[LBA_TONE_DEBUG] preset={} exposure={:.3} gamma={:.3} saturation={:.3} contrast={:.3} highlight_rolloff={:.3} debug_view={} final_color_range_est=({:.3},{:.3}) clamp=(0.000,1.000)",
        config.preset.label(),
        config.exposure,
        config.gamma,
        config.saturation,
        config.contrast,
        config.highlight_rolloff,
        config.debug_view.label(),
        range_min,
        range_max,
    );
}

fn log_emissive_bloom_debug(
    config: EmissiveBloomConfig,
    materials: Option<&FullModeMaterialCache>,
) {
    let matched_material_count = materials
        .map(count_emissive_material_candidates)
        .unwrap_or(0);
    let unmatched_emissive_looking_count = materials
        .map(count_unmatched_emissive_looking_material_candidates)
        .unwrap_or(0);
    let material_slots = materials
        .map(|materials| materials.materials.len())
        .unwrap_or(0);
    println!(
        "[LBA_EMISSIVE_BLOOM_DEBUG] emissive_effective={} emissive_strength={:.3} emissive_debug_view={} bloom_effective={} threshold={:.3} intensity={:.3} radius={:.3} bloom_debug_view={} material_id_available=false emissive_tag_available=true matched_emissive_count={} unmatched_emissive_looking_candidates={} material_slots={} emissive_mask_mode=emissive_tag fallback_reason=none",
        config.emissive_enabled,
        config.emissive_strength,
        config.emissive_debug_view.label(),
        config.bloom_enabled,
        config.bloom_threshold,
        config.bloom_intensity,
        config.bloom_radius,
        config.bloom_debug_view.label(),
        matched_material_count,
        unmatched_emissive_looking_count,
        material_slots,
    );
    if let Some(materials) = materials {
        for (index, key) in unmatched_emissive_looking_material_candidates(materials)
            .into_iter()
            .take(32)
        {
            println!(
                "[LBA_EMISSIVE_UNMATCHED_CANDIDATE] material_index={} key={}",
                index, key
            );
        }
    }
}

fn count_emissive_material_candidates(materials: &FullModeMaterialCache) -> usize {
    materials
        .materials
        .iter()
        .filter(|slot| emissive_material_key_candidate(&slot.key))
        .count()
}

fn emissive_material_key_candidate(key: &str) -> bool {
    emissive_material_key_match(key).is_some()
}

fn emissive_material_key_match(key: &str) -> Option<(&'static str, f32)> {
    let key = key.to_ascii_lowercase();
    let local = key.rsplit([':', '/', '\\']).next().unwrap_or(key.as_str());
    const EMISSIVE_NAMES: [(&str, f32); 23] = [
        ("glowstone", 1.0),
        ("sea_lantern", 2.0),
        ("shroomlight", 3.0),
        ("ochre_froglight", 4.0),
        ("verdant_froglight", 4.0),
        ("pearlescent_froglight", 4.0),
        ("froglight", 4.0),
        ("redstone_lamp", 5.0),
        ("soul_lantern", 6.0),
        ("lantern", 6.0),
        ("soul_wall_torch", 8.0),
        ("soul_torch", 8.0),
        ("redstone_wall_torch", 7.0),
        ("redstone_torch", 7.0),
        ("wall_torch", 7.0),
        ("torch", 7.0),
        ("end_rod", 9.0),
        ("soul_campfire", 10.0),
        ("campfire", 10.0),
        ("jack_o_lantern", 11.0),
        ("beacon", 12.0),
        ("cave_vines_lit", 13.0),
        ("cave_vines_plant_lit", 13.0),
    ];
    EMISSIVE_NAMES
        .iter()
        .find_map(|(name, tag)| key.contains(name).then_some((*name, *tag)))
        .or_else(|| {
            matches!(local, "glow_berries" | "glow_berry_vines").then_some(("glow_berries", 13.0))
        })
}

fn count_unmatched_emissive_looking_material_candidates(
    materials: &FullModeMaterialCache,
) -> usize {
    unmatched_emissive_looking_material_candidates(materials).len()
}

fn unmatched_emissive_looking_material_candidates(
    materials: &FullModeMaterialCache,
) -> Vec<(usize, String)> {
    materials
        .materials
        .iter()
        .enumerate()
        .filter(|(_, slot)| {
            !emissive_material_key_candidate(&slot.key)
                && emissive_looking_material_key_candidate(&slot.key)
        })
        .map(|(index, slot)| (index, slot.key.clone()))
        .collect()
}

fn emissive_looking_material_key_candidate(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "glow", "lit", "light", "lamp", "torch", "lantern", "beacon", "berry", "rod",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

#[derive(Debug, Default, Clone, Copy)]
struct ShadowScalarStats {
    count: u64,
    min: f32,
    max: f32,
    sum: f64,
}

impl ShadowScalarStats {
    fn record(&mut self, value: f32) {
        if !value.is_finite() {
            return;
        }
        if self.count == 0 {
            self.min = value;
            self.max = value;
        } else {
            self.min = self.min.min(value);
            self.max = self.max.max(value);
        }
        self.count += 1;
        self.sum += f64::from(value);
    }

    fn avg(self) -> f32 {
        if self.count == 0 {
            0.0
        } else {
            (self.sum / self.count as f64) as f32
        }
    }

    fn format(self) -> String {
        if self.count == 0 {
            "count=0 min=n/a max=n/a avg=n/a".to_string()
        } else {
            format!(
                "count={} min={:.6} max={:.6} avg={:.6}",
                self.count,
                self.min,
                self.max,
                self.avg()
            )
        }
    }
}

#[derive(Debug, Default)]
struct ShadowCompareStats {
    total: u64,
    inside: u64,
    outside: u64,
    invalid: u64,
    depth_out: u64,
    lit: u64,
    shadowed: u64,
    receiver_depth: ShadowScalarStats,
    sampled_shadow_depth: ShadowScalarStats,
}

impl ShadowCompareStats {
    fn log(&self, config: ShadowMapConfig) {
        let inside_ratio = ratio(self.inside, self.total);
        let outside_ratio = ratio(self.outside, self.total);
        let invalid_ratio = ratio(self.invalid, self.total);
        let depth_out_ratio = ratio(self.depth_out, self.total);
        let shadowed_ratio = ratio(self.shadowed, self.inside);
        println!(
            "[LBA_SHADOW_DEBUG] stage=coverage samples={} inside={}({:.3}) outside={}({:.3}) invalid={}({:.3}) depth_out={}({:.3}) lit={} shadowed={} shadowed_inside_ratio={:.3} receiver_depth=[{}] sampled_shadow_depth=[{}]",
            self.total,
            self.inside,
            inside_ratio,
            self.outside,
            outside_ratio,
            self.invalid,
            invalid_ratio,
            self.depth_out,
            depth_out_ratio,
            self.lit,
            self.shadowed,
            shadowed_ratio,
            self.receiver_depth.format(),
            self.sampled_shadow_depth.format(),
        );
        if shadow_smoke_check_enabled() {
            log_shadow_smoke_check(self, config);
        }
    }
}

fn ratio(value: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        value as f32 / total as f32
    }
}

fn shadow_smoke_check_enabled() -> bool {
    env_bool("LBA_SHADOW_SMOKE_CHECK")
}

fn log_shadow_smoke_check(stats: &ShadowCompareStats, config: ShadowMapConfig) {
    let texel_density_ok = config
        .light_camera
        .map(|camera| {
            camera.world_units_per_texel.is_finite() && camera.world_units_per_texel > 0.0
        })
        .unwrap_or(false);
    let inside_ratio = ratio(stats.inside, stats.total);
    let abnormal_ratio = ratio(stats.outside + stats.invalid + stats.depth_out, stats.total);
    let pass = config.enabled
        && config.pcf.samples > 0
        && texel_density_ok
        && stats.total > 0
        && inside_ratio >= 0.01
        && abnormal_ratio < 0.75;
    println!(
        "[LBA_SHADOW_SMOKE] pass={} shadows_enabled={} pcf_effective={} texel_density_ok={} samples={} inside_ratio={:.3} abnormal_ratio={:.3}",
        pass,
        config.enabled,
        config.pcf.samples > 0,
        texel_density_ok,
        stats.total,
        inside_ratio,
        abnormal_ratio,
    );
}

fn collect_shadow_compare_stats(
    resident_chunks: &BTreeMap<ChunkKey, ResidentChunkMesh>,
    cpu_mesh_cache: &ChunkMeshCpuCache,
    depth_values: &[f32],
    config: ShadowMapConfig,
) -> ShadowCompareStats {
    let mut stats = ShadowCompareStats::default();
    let Some(light_camera) = config.light_camera else {
        return stats;
    };
    let resolution = config.resolution as usize;
    if resolution == 0 || depth_values.len() < resolution.saturating_mul(resolution) {
        return stats;
    }
    for resident in resident_chunks.values() {
        let Some(entry) = cpu_mesh_cache.entries.get(&resident.key) else {
            continue;
        };
        let vertex_stride = (entry.mesh.vertices.len() / 120_000).max(1);
        for vertex in entry.mesh.vertices.iter().step_by(vertex_stride) {
            stats.total += 1;
            let clip = light_camera.view_proj * Vec3::from_array(vertex.position).extend(1.0);
            if clip.w <= 0.000001
                || !clip.x.is_finite()
                || !clip.y.is_finite()
                || !clip.z.is_finite()
            {
                stats.invalid += 1;
                continue;
            }
            let ndc = clip.truncate() / clip.w;
            let uv = Vec2::new(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
            if ndc.z < 0.0 || ndc.z > 1.0 {
                stats.depth_out += 1;
                continue;
            }
            if uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 {
                stats.outside += 1;
                continue;
            }
            stats.inside += 1;
            stats.receiver_depth.record(ndc.z);
            let x = ((uv.x * resolution as f32).floor() as usize).min(resolution - 1);
            let y = ((uv.y * resolution as f32).floor() as usize).min(resolution - 1);
            let sampled = depth_values[y * resolution + x];
            stats.sampled_shadow_depth.record(sampled);
            if ndc.z - config.bias <= sampled {
                stats.lit += 1;
            } else {
                stats.shadowed += 1;
            }
        }
    }
    stats
}

fn collect_shadow_depth_stats(depth_values: &[f32]) -> ShadowScalarStats {
    let mut stats = ShadowScalarStats::default();
    for value in depth_values.iter().copied() {
        if value < 0.999_999 {
            stats.record(value);
        }
    }
    stats
}

fn read_shadow_depth_values(
    device: &wgpu::Device,
    buffer: &wgpu::Buffer,
    byte_len: usize,
) -> Option<Vec<f32>> {
    let slice = buffer.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device.poll(wgpu::Maintain::Wait);
    if rx.recv().ok()?.is_err() {
        return None;
    }
    let view = slice.get_mapped_range();
    let values = bytemuck::cast_slice::<u8, f32>(&view[..byte_len]).to_vec();
    drop(view);
    buffer.unmap();
    Some(values)
}

struct ViewerState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: PhysicalSize<u32>,
    render_pipeline_solid: wgpu::RenderPipeline,
    render_pipeline_translucent: wgpu::RenderPipeline,
    camera: OrbitCamera,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    lighting_bind_group: wgpu::BindGroup,
    shadow_resources: ShadowMapResources,
    atlas_bind_group: wgpu::BindGroup,
    depth_texture: DepthTexture,
    drag_mode: Option<DragMode>,
    last_cursor_position: Option<Vec2>,
    scene_bounds: SceneBounds,
    chunk_scene_index: Option<Arc<ChunkSceneIndex>>,
    cpu_mesh_cache: ChunkMeshCpuCache,
    pending_build_queue: VecDeque<PendingChunkBuild>,
    pending_build_keys: HashSet<ChunkKey>,
    in_flight_build_keys: HashSet<ChunkKey>,
    max_in_flight_builds: usize,
    build_request_tx: Option<Sender<BuildWorkerRequest>>,
    build_result_rx: Receiver<BuildWorkerResult>,
    build_workers: Vec<JoinHandle<()>>,
    build_worker_count: usize,
    trace: Arc<TraceCounters>,
    build_budget: BuildBudget,
    pending_upload_queue: VecDeque<PendingChunkUpload>,
    pending_upload_keys: HashSet<ChunkKey>,
    upload_budget: UploadBudget,
    current_target_keys: HashSet<ChunkKey>,
    current_preload_keys: HashSet<ChunkKey>,
    current_retain_keys: HashSet<ChunkKey>,
    sticky_target_keys: BTreeMap<ChunkKey, u64>,
    warm_cache_keys: BTreeMap<ChunkKey, u64>,
    resident_chunks: BTreeMap<ChunkKey, ResidentChunkMesh>,
    max_resident_chunks: usize,
    max_resident_bytes: usize,
    prebuild_finished_keys: HashSet<ChunkKey>,
    prebuild_empty_mesh_keys: HashSet<ChunkKey>,
    next_build_batch_id: u64,
    streaming_config: ChunkStreamingConfig,
    stress_camera_sweep: bool,
    background_fill_enabled: bool,
    bootstrap_visible_first: usize,
    prebuild_before_show: bool,
    prebuild_cache_only: bool,
    prebuild_completed: bool,
    ready_file: Option<PathBuf>,
    cache_file: Option<PathBuf>,
    prebuild_cache_pipeline: Option<PrebuildCachePipeline>,
    prebuild_manifest_chunks: Vec<NativePreviewCacheManifestChunk>,
    prebuild_cache_written_keys: HashSet<ChunkKey>,
    prebuild_chunk_bytes: usize,
    prebuild_writer_submitted_chunks: usize,
    prebuild_writer_completed_chunks: usize,
    total_scene_chunks: usize,
    prebuild_last_progress_write: Instant,
    prebuild_last_eta_update: Instant,
    prebuild_eta_sample_time: Instant,
    prebuild_eta_sample_resident: usize,
    prebuild_eta_seconds: Option<u64>,
    prebuild_last_tail_log: Instant,
    prebuild_last_completed_count: usize,
    prebuild_last_completed_at: Instant,
    residency_dirty: bool,
    frame_index: u64,
    embedded_viewport: Option<EmbeddedViewportInfo>,
    preview_mode: bool,
    preview_spin: bool,
    last_focus_chunk: Option<ChunkKey>,
    last_target_radius_chunks: Option<i32>,
    last_background_fill_log_frame: u64,
    last_runtime_diag_frame: u64,
    last_pressure_skip_log_frame: u64,
    last_camera_diag_frame: u64,
    shadow_debug: bool,
    shadow_debug_logged: bool,
}

impl ViewerState {
    async fn new(
        window: Arc<winit::window::Window>,
        scene: ViewerScene,
        args: &ViewerArgs,
    ) -> Result<Self> {
        let wgpu_init_started_at = Instant::now();
        let size = window.inner_size();
        println!(
            "[WGPU_INIT] create_window size={}x{} title={}",
            size.width, size.height, scene.label
        );
        let ViewerScene {
            label: _,
            bounds: scene_bounds,
            palette_colors,
            chunk_scene_index: scene_chunk_scene_index,
            bootstrap_meshes,
            bootstrap_visible_first,
            full_mode_materials,
            texture_atlas,
        } = scene;

        let instance_started_at = Instant::now();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        println!(
            "[TRACE_STARTUP] stage=wgpu_instance elapsed_ms={}",
            instance_started_at.elapsed().as_millis()
        );
        let surface_started_at = Instant::now();
        let surface = instance
            .create_surface(window.clone())
            .context("failed to create wgpu surface")?;
        println!(
            "[TRACE_STARTUP] stage=wgpu_surface elapsed_ms={}",
            surface_started_at.elapsed().as_millis()
        );

        let adapter_started_at = Instant::now();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("failed to acquire suitable GPU adapter")?;
        println!(
            "[TRACE_STARTUP] stage=wgpu_adapter elapsed_ms={}",
            adapter_started_at.elapsed().as_millis()
        );
        let adapter_info = adapter.get_info();
        println!(
            "[WGPU_INIT] adapter name={} backend={:?} device_type={:?}",
            adapter_info.name, adapter_info.backend, adapter_info.device_type
        );

        let device_started_at = Instant::now();
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("native_viewer_device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .context("failed to create wgpu device")?;
        println!(
            "[TRACE_STARTUP] stage=wgpu_device elapsed_ms={}",
            device_started_at.elapsed().as_millis()
        );

        let surface_config_started_at = Instant::now();
        let capabilities = surface.get_capabilities(&adapter);
        let surface_format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(capabilities.formats[0]);
        let present_mode = if capabilities
            .present_modes
            .contains(&wgpu::PresentMode::Mailbox)
        {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        println!(
            "[TRACE_STARTUP] stage=wgpu_surface_config elapsed_ms={}",
            surface_config_started_at.elapsed().as_millis()
        );
        println!(
            "[WGPU_INIT] surface format={:?} present_mode={:?}",
            surface_format, present_mode
        );

        let pipeline_started_at = Instant::now();
        let camera = OrbitCamera::from_bounds(scene_bounds, config.width, config.height);
        let camera_uniform =
            CameraUniform::from_matrix_and_eye(camera.view_proj_matrix(), camera.eye());
        let lighting_config = LightingConfig::from_args(
            args.basic_lighting,
            args.basic_shadows,
            args.shadow_debug_view,
            args.shadow_force_test,
            scene_bounds,
        );
        let lighting_uniform = LightingUniform::from_config(lighting_config);
        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("native_viewer_camera"),
            contents: bytemuck::bytes_of(&camera_uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("native_viewer_camera_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("native_viewer_camera_bind_group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
        let lighting_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("native_viewer_basic_lighting"),
            contents: bytemuck::bytes_of(&lighting_uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let lighting_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("native_viewer_basic_lighting_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let lighting_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("native_viewer_basic_lighting_bind_group"),
            layout: &lighting_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: lighting_buffer.as_entire_binding(),
            }],
        });
        let shadow_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("native_viewer_basic_shadow_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                        count: None,
                    },
                ],
            });
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("native_viewer_texture_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let atlas_bind_group = create_atlas_bind_group(
            &device,
            &queue,
            &texture_bind_group_layout,
            texture_atlas.as_ref(),
        );

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("native_viewer_shader"),
            source: wgpu::ShaderSource::Wgsl(VIEWER_SHADER.into()),
        });
        let shadow_depth_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("native_viewer_basic_shadow_depth_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADOW_DEPTH_SHADER.into()),
        });
        let shadow_resources = create_shadow_map_resources(
            &device,
            lighting_config.shadow(),
            &shadow_bind_group_layout,
            &camera_bind_group_layout,
            &shadow_depth_shader,
        );
        if args.shadow_debug {
            log_shadow_startup_debug(
                args.basic_lighting,
                args.basic_shadows,
                lighting_config.shadow(),
                &lighting_uniform,
            );
        }
        if env_bool("LBA_AO_DEBUG") || lighting_config.ao().debug_view != AoDebugViewMode::Off {
            log_ao_startup_debug(lighting_config.ao(), lighting_config.shadow());
        }
        if env_bool("LBA_AO_SMOKE_CHECK") {
            log_ao_smoke_check(lighting_config.ao());
        }
        if env_bool("LBA_TONE_DEBUG") || lighting_config.tone().debug_view != ToneDebugViewMode::Off
        {
            log_tone_startup_debug(lighting_config.tone());
        }
        if env_bool("LBA_EMISSIVE_DEBUG")
            || env_bool("LBA_BLOOM_DEBUG")
            || lighting_config.emissive_bloom().emissive_debug_view != EmissiveDebugViewMode::Off
            || lighting_config.emissive_bloom().bloom_debug_view != BloomDebugViewMode::Off
        {
            log_emissive_bloom_debug(
                lighting_config.emissive_bloom(),
                full_mode_materials.as_deref(),
            );
        }
        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("native_viewer_pipeline_layout"),
                bind_group_layouts: &[
                    &camera_bind_group_layout,
                    &texture_bind_group_layout,
                    &lighting_bind_group_layout,
                    &shadow_bind_group_layout,
                ],
                push_constant_ranges: &[],
            });
        let render_pipeline_solid =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("native_viewer_pipeline_solid"),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &[GpuVertex::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth24Plus,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });
        let render_pipeline_translucent =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("native_viewer_pipeline_translucent"),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &[GpuVertex::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth24Plus,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        let depth_texture = DepthTexture::create(&device, &config);
        println!(
            "[TRACE_STARTUP] stage=wgpu_pipeline elapsed_ms={}",
            pipeline_started_at.elapsed().as_millis()
        );
        let chunk_scene_index = scene_chunk_scene_index.map(Arc::new);
        let trace = Arc::new(TraceCounters::default());
        let prebuild_cache_pipeline = if args.prebuild_before_show
            && args.cache_file.is_some()
            && chunk_scene_index.is_some()
            && writer_pipeline_enabled()
        {
            Some(PrebuildCachePipeline::start(
                args.cache_file.as_deref().expect("checked cache file"),
                trace.clone(),
            )?)
        } else {
            if args.prebuild_before_show && args.cache_file.is_some() {
                println!(
                    "[TRACE_WALL] stage=cache_write event=pipeline_disabled reason={}",
                    if writer_pipeline_enabled() {
                        "no_chunk_scene"
                    } else {
                        "env_disabled"
                    }
                );
            }
            None
        };
        let (build_request_tx, build_result_rx, build_workers, build_worker_count) =
            if let Some(scene_index) = chunk_scene_index.as_ref() {
                let (request_tx, result_rx, workers) = spawn_build_worker_pool(
                    scene_index.clone(),
                    Arc::new(palette_colors.clone()),
                    full_mode_materials.clone(),
                    args.build_workers,
                    trace.clone(),
                );
                (Some(request_tx), result_rx, workers, args.build_workers)
            } else {
                let (_request_tx, request_rx) = mpsc::channel::<BuildWorkerRequest>();
                drop(request_rx);
                let (_result_tx, result_rx) = mpsc::channel::<BuildWorkerResult>();
                (None, result_rx, Vec::new(), 0)
            };

        let total_scene_chunks = chunk_scene_index
            .as_ref()
            .map(|index| index.chunk_count())
            .unwrap_or(bootstrap_meshes.len());
        let mut state = Self {
            surface,
            device,
            queue,
            config,
            size,
            render_pipeline_solid,
            render_pipeline_translucent,
            camera,
            camera_buffer,
            camera_bind_group,
            lighting_bind_group,
            shadow_resources,
            atlas_bind_group,
            depth_texture,
            drag_mode: None,
            last_cursor_position: None,
            scene_bounds,
            chunk_scene_index,
            cpu_mesh_cache: ChunkMeshCpuCache::new(
                args.cpu_cache_max_chunks,
                args.cpu_cache_max_bytes,
            ),
            pending_build_queue: VecDeque::new(),
            pending_build_keys: HashSet::new(),
            in_flight_build_keys: HashSet::new(),
            max_in_flight_builds: build_worker_count.max(1) * 16,
            build_request_tx,
            build_result_rx,
            build_workers,
            build_worker_count,
            trace,
            build_budget: BuildBudget::default(),
            pending_upload_queue: VecDeque::new(),
            pending_upload_keys: HashSet::new(),
            upload_budget: UploadBudget::default(),
            current_target_keys: HashSet::new(),
            current_preload_keys: HashSet::new(),
            current_retain_keys: HashSet::new(),
            sticky_target_keys: BTreeMap::new(),
            warm_cache_keys: BTreeMap::new(),
            resident_chunks: BTreeMap::new(),
            max_resident_chunks: args.resident_max_chunks,
            max_resident_bytes: args.resident_max_bytes,
            prebuild_finished_keys: HashSet::new(),
            prebuild_empty_mesh_keys: HashSet::new(),
            next_build_batch_id: 1,
            streaming_config: ChunkStreamingConfig::default(),
            stress_camera_sweep: args.stress_limits,
            background_fill_enabled: args.prebuild_before_show,
            bootstrap_visible_first,
            prebuild_before_show: args.prebuild_before_show,
            prebuild_cache_only: args.prebuild_only,
            prebuild_completed: !args.prebuild_before_show,
            ready_file: args.ready_file.clone(),
            cache_file: args.cache_file.clone(),
            prebuild_cache_pipeline,
            prebuild_manifest_chunks: Vec::new(),
            prebuild_cache_written_keys: HashSet::new(),
            prebuild_chunk_bytes: 0,
            prebuild_writer_submitted_chunks: 0,
            prebuild_writer_completed_chunks: 0,
            total_scene_chunks,
            prebuild_last_progress_write: Instant::now(),
            prebuild_last_eta_update: Instant::now(),
            prebuild_eta_sample_time: Instant::now(),
            prebuild_eta_sample_resident: 0,
            prebuild_eta_seconds: None,
            prebuild_last_tail_log: Instant::now(),
            prebuild_last_completed_count: 0,
            prebuild_last_completed_at: Instant::now(),
            residency_dirty: true,
            frame_index: 0,
            embedded_viewport: args
                .embed_parent_hwnd
                .map(|parent_hwnd| EmbeddedViewportInfo {
                    parent_hwnd,
                    client_size: size,
                }),
            preview_mode: args.preview_mode,
            preview_spin: args.preview_spin,
            last_focus_chunk: None,
            last_target_radius_chunks: None,
            last_background_fill_log_frame: 0,
            last_runtime_diag_frame: 0,
            last_pressure_skip_log_frame: 0,
            last_camera_diag_frame: 0,
            shadow_debug: args.shadow_debug,
            shadow_debug_logged: false,
        };
        state.install_bootstrap_meshes(bootstrap_meshes);
        println!(
            "[VIEWER_CACHE] config cpu_max_chunks={} cpu_max_bytes={} resident_max_chunks={} resident_max_bytes={} stress_limits={}",
            args.cpu_cache_max_chunks,
            args.cpu_cache_max_bytes,
            args.resident_max_chunks,
            args.resident_max_bytes,
            args.stress_limits
        );
        state.log_camera_viewport_diagnostics("initial", true);
        println!(
            "[VIEWER_ASYNC] worker_pool_started worker_count={} max_in_flight_builds={}",
            state.build_worker_count, state.max_in_flight_builds
        );
        println!(
            "[TRACE_STARTUP] stage=wgpu_total elapsed_ms={}",
            wgpu_init_started_at.elapsed().as_millis()
        );
        state.sync_chunk_residency()?;
        Ok(state)
    }

    fn install_bootstrap_meshes(&mut self, bootstrap_meshes: Vec<PreparedChunkMesh>) {
        let bootstrap_count = bootstrap_meshes.len();
        for prepared in bootstrap_meshes {
            self.cpu_mesh_cache
                .insert(prepared.clone(), self.frame_index);
            if !self.prebuild_cache_only {
                let resident = self.upload_chunk_mesh(&prepared);
                self.resident_chunks.insert(resident.key, resident);
            }
            self.touch_warm_cache_key(prepared.key);
            if self.prebuild_before_show {
                self.prebuild_finished_keys.insert(prepared.key);
            }
        }
        if !self.resident_chunks.is_empty() && !self.prebuild_before_show {
            println!(
                "[VIEWER_BOOTSTRAP] bootstrap_phase_done bootstrap_chunks={} visible_first={} cpu_cache_chunks={} resident_chunks={} draw_chunks={}",
                bootstrap_count,
                self.bootstrap_visible_first,
                self.cpu_mesh_cache.len(),
                self.resident_chunks.len(),
                self.resident_chunks.len()
            );
            println!(
                "[VIEWER_STREAM] first_visible_ready bootstrap_chunks={} visible_first={} resident_chunks={} cpu_cache_chunks={}",
                bootstrap_count,
                self.bootstrap_visible_first,
                self.resident_chunks.len(),
                self.cpu_mesh_cache.len()
            );
        }
    }

    fn prebuild_is_ready(&self) -> bool {
        if self.prebuild_cache_only {
            return self.prebuild_before_show
                && !self.prebuild_completed
                && self.chunk_scene_index.is_some()
                && self.prebuild_completed_chunk_count() >= self.total_scene_chunks
                && self.pending_build_queue.is_empty()
                && self.in_flight_build_keys.is_empty();
        }
        self.prebuild_before_show
            && !self.prebuild_completed
            && self.chunk_scene_index.is_some()
            && self.prebuild_ready_chunk_count() >= self.total_scene_chunks
            && self.pending_build_queue.is_empty()
            && self.in_flight_build_keys.is_empty()
            && self.pending_upload_queue.is_empty()
    }

    fn prebuild_completed_chunk_count(&self) -> usize {
        self.prebuild_finished_keys
            .len()
            .max(
                self.resident_chunks
                    .len()
                    .saturating_add(self.prebuild_empty_mesh_keys.len()),
            )
            .min(self.total_scene_chunks)
    }

    fn prebuild_ready_chunk_count(&self) -> usize {
        if self.prebuild_cache_only {
            return self.prebuild_completed_chunk_count();
        }
        self.resident_chunks
            .len()
            .saturating_add(self.prebuild_empty_mesh_keys.len())
            .min(self.total_scene_chunks)
    }

    fn prebuild_phase(&self, ready: bool) -> &'static str {
        if ready {
            "ready"
        } else if self.prebuild_cache_only {
            "build"
        } else if !self.pending_upload_queue.is_empty()
            || self.resident_chunks.len() > self.cpu_mesh_cache.len().saturating_sub(1)
        {
            "upload"
        } else {
            "build"
        }
    }

    fn maybe_recompute_prebuild_eta(&mut self, now: Instant) {
        if now.duration_since(self.prebuild_last_eta_update) < Duration::from_secs(10)
            && self.prebuild_eta_seconds.is_some()
        {
            return;
        }
        let resident = self.prebuild_ready_chunk_count();
        let elapsed = now
            .duration_since(self.prebuild_eta_sample_time)
            .as_secs_f64();
        let advanced = resident.saturating_sub(self.prebuild_eta_sample_resident);
        if elapsed >= 1.0 && advanced > 0 && resident < self.total_scene_chunks {
            let chunks_per_second = advanced as f64 / elapsed;
            let remaining = self.total_scene_chunks.saturating_sub(resident) as f64;
            self.prebuild_eta_seconds =
                Some((remaining / chunks_per_second).ceil().max(1.0) as u64);
            self.prebuild_last_eta_update = now;
            self.prebuild_eta_sample_time = now;
            self.prebuild_eta_sample_resident = resident;
        } else if resident >= self.total_scene_chunks {
            self.prebuild_eta_seconds = Some(0);
            self.prebuild_last_eta_update = now;
            self.prebuild_eta_sample_time = now;
            self.prebuild_eta_sample_resident = resident;
        } else if now.duration_since(self.prebuild_last_eta_update) >= Duration::from_secs(10) {
            self.prebuild_eta_seconds = None;
            self.prebuild_last_eta_update = now;
            self.prebuild_eta_sample_time = now;
            self.prebuild_eta_sample_resident = resident;
        }
    }

    fn write_prebuild_progress_signal(&mut self, elapsed: Duration, ready: bool, force: bool) {
        let Some(path) = self.ready_file.clone() else {
            return;
        };
        let now = Instant::now();
        if !force
            && now.duration_since(self.prebuild_last_progress_write) < Duration::from_millis(500)
        {
            return;
        }
        self.prebuild_last_progress_write = now;
        if ready {
            self.prebuild_eta_seconds = Some(0);
        }
        self.maybe_recompute_prebuild_eta(now);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let total_chunks = self.total_scene_chunks.max(1);
        let completed_chunks = self.prebuild_ready_chunk_count();
        let (scene_size_x, scene_size_y, scene_size_z) = self
            .chunk_scene_index
            .as_ref()
            .map(|scene_index| {
                let size = &scene_index.metadata().enclosing_size;
                (size.x, size.y, size.z)
            })
            .unwrap_or((0, 0, 0));
        let built_chunks = self
            .cpu_mesh_cache
            .len()
            .saturating_add(self.prebuild_empty_mesh_keys.len())
            .min(total_chunks);
        let renderable_chunks = self.cpu_mesh_cache.len().min(total_chunks);
        let empty_mesh_chunks = self.prebuild_empty_mesh_keys.len().min(total_chunks);
        let resident_chunks = self.resident_chunks.len().min(total_chunks);
        let uploaded_chunks = completed_chunks;
        let cache_file_bytes = self
            .cache_file
            .as_deref()
            .map(|cache_path| prebuild_cache_total_bytes(cache_path, ready))
            .unwrap_or(0);
        let build_progress = built_chunks as f64 / total_chunks as f64;
        let upload_progress = if self.prebuild_cache_only {
            build_progress
        } else {
            resident_chunks as f64 / total_chunks as f64
        };
        let percent = if ready {
            100.0
        } else if self.prebuild_cache_only {
            (build_progress * 99.0).clamp(0.0, 99.0)
        } else {
            ((build_progress * 0.45 + upload_progress * 0.55) * 100.0).clamp(0.0, 99.9)
        };
        let phase = self.prebuild_phase(ready);
        let eta_value = self
            .prebuild_eta_seconds
            .map(|seconds| seconds.to_string())
            .unwrap_or_else(|| "null".to_string());
        let payload = format!(
            "{{\"ready\":{},\"total_chunks\":{},\"scene_size_x\":{},\"scene_size_y\":{},\"scene_size_z\":{},\"built_chunks\":{},\"renderable_chunks\":{},\"empty_mesh_chunks\":{},\"uploaded_chunks\":{},\"resident_chunks\":{},\"cache_file_bytes\":{},\"percent\":{:.2},\"phase\":\"{}\",\"elapsed_ms\":{},\"eta_seconds\":{}}}",
            if ready { "true" } else { "false" },
            self.total_scene_chunks,
            scene_size_x,
            scene_size_y,
            scene_size_z,
            built_chunks,
            renderable_chunks,
            empty_mesh_chunks,
            uploaded_chunks,
            resident_chunks,
            cache_file_bytes,
            percent,
            phase,
            elapsed.as_millis(),
            eta_value,
        );
        if let Err(error) = std::fs::write(&path, payload) {
            println!(
                "[NATIVE_VIEWER] progress_write_failed path={} error={}",
                path.display(),
                error
            );
            return;
        }
        println!(
            "[NATIVE_VIEWER] progress_update phase={} percent={:.2} built_chunks={} renderable_chunks={} empty_mesh_chunks={} uploaded_chunks={} resident_chunks={} total_chunks={} cache_file_bytes={} eta_seconds={} elapsed_ms={}",
            phase,
            percent,
            built_chunks,
            renderable_chunks,
            empty_mesh_chunks,
            uploaded_chunks,
            resident_chunks,
            self.total_scene_chunks,
            cache_file_bytes,
            eta_value,
            elapsed.as_millis(),
        );
        if ready {
            println!(
                "[NATIVE_VIEWER] ready_written path={} elapsed_ms={} resident_chunks={} renderable_chunks={} empty_mesh_chunks={} total_chunks={} cache_file_bytes={}",
                path.display(),
                elapsed.as_millis(),
                resident_chunks,
                renderable_chunks,
                empty_mesh_chunks,
                self.total_scene_chunks,
                cache_file_bytes
            );
        }
    }

    fn finalize_prebuild(&mut self, window: &winit::window::Window, viewer_started_at: Instant) {
        if self.prebuild_completed {
            return;
        }
        let finalize_started_at = Instant::now();
        self.prebuild_completed = true;
        println!(
            "[TRACE_WALL] stage=finalize event=start elapsed_ms={}",
            viewer_started_at.elapsed().as_millis()
        );
        println!(
            "[TRACE_WALL] stage=cache_write event=start resident_chunks={} cpu_cache_chunks={}",
            self.resident_chunks.len(),
            self.cpu_mesh_cache.len()
        );
        let cache_write_ms = self.write_prebuild_cache_file();
        println!(
            "[TRACE_WALL] stage=cache_write event=end elapsed_ms={}",
            cache_write_ms
        );
        self.write_prebuild_progress_signal(viewer_started_at.elapsed(), true, true);
        window.set_visible(true);
        let finalize_ms = finalize_started_at.elapsed().as_millis();
        let total_wall_ms = viewer_started_at.elapsed().as_millis();
        println!(
            "[PREBUILD_TIMING] finalize_ms={} cache_write_ms={} total_elapsed_ms={}",
            finalize_ms, cache_write_ms, total_wall_ms
        );
        println!(
            "[TRACE_WALL] stage=finalize event=end elapsed_ms={} total_wall_ms={}",
            finalize_ms, total_wall_ms
        );
        println!(
            "[TRACE_SUMMARY] total_wall_ms={} worker_busy_ms={} worker_wait_ms={} worker_batches={} worker_chunks={} worker_result_send_ms={} build_enqueue_ms={} build_enqueue_batches={} build_enqueue_chunks={} build_dequeue_wait_ms={} build_dequeue_batches={} build_dequeue_chunks={} writer_queue_wait_ms={} writer_busy_ms={} writer_chunks={} writer_errors={} writer_submitted_chunks={} writer_completed_chunks={} cache_file_create_ms={} max_pending_build_queue={} max_in_flight_builds={} max_pending_upload_queue={} pending_builds={} in_flight_builds={} pending_uploads={} writer_model={}",
            total_wall_ms,
            self.trace.worker_busy_ms.load(Ordering::Relaxed),
            self.trace.worker_wait_ms.load(Ordering::Relaxed),
            self.trace.worker_batches.load(Ordering::Relaxed),
            self.trace.worker_chunks.load(Ordering::Relaxed),
            self.trace.worker_result_send_ms.load(Ordering::Relaxed),
            self.trace.build_enqueue_ms.load(Ordering::Relaxed),
            self.trace.build_enqueue_batches.load(Ordering::Relaxed),
            self.trace.build_enqueue_chunks.load(Ordering::Relaxed),
            self.trace.build_dequeue_ms.load(Ordering::Relaxed),
            self.trace.build_dequeue_batches.load(Ordering::Relaxed),
            self.trace.build_dequeue_chunks.load(Ordering::Relaxed),
            self.trace.writer_wait_ms.load(Ordering::Relaxed),
            self.trace.writer_busy_ms.load(Ordering::Relaxed),
            self.trace.writer_chunks.load(Ordering::Relaxed),
            self.trace.writer_errors.load(Ordering::Relaxed),
            self.prebuild_writer_submitted_chunks,
            self.prebuild_writer_completed_chunks,
            self.trace.cache_file_create_ms.load(Ordering::Relaxed),
            self.trace.max_pending_build_queue.load(Ordering::Relaxed),
            self.trace.max_in_flight_builds.load(Ordering::Relaxed),
            self.trace.max_pending_upload_queue.load(Ordering::Relaxed),
            self.pending_build_queue.len(),
            self.in_flight_build_keys.len(),
            self.pending_upload_queue.len(),
            if writer_pipeline_enabled() {
                "pipeline"
            } else {
                "synchronous"
            }
        );
        println!(
            "[TRACE_CRITICAL_PATH] total_wall_ms={} finalize_ms={} cache_write_ms={} writer_queue_wait_ms={} writer_busy_ms_cumulative={} worker_busy_ms_cumulative={} worker_wait_ms_cumulative={} build_queue_wait_ms_cumulative={}",
            total_wall_ms,
            finalize_ms,
            cache_write_ms,
            self.trace.writer_wait_ms.load(Ordering::Relaxed),
            self.trace.writer_busy_ms.load(Ordering::Relaxed),
            self.trace.worker_busy_ms.load(Ordering::Relaxed),
            self.trace.worker_wait_ms.load(Ordering::Relaxed),
            self.trace.build_dequeue_ms.load(Ordering::Relaxed)
        );
        println!(
            "[VIEWER_BOOTSTRAP] prebuild_phase_done elapsed_ms={} total_chunks={} resident_chunks={} cpu_cache_chunks={}",
            viewer_started_at.elapsed().as_millis(),
            self.total_scene_chunks,
            self.resident_chunks.len(),
            self.cpu_mesh_cache.len()
        );
        println!(
            "[VIEWER_STREAM] first_visible_ready elapsed_ms={} total_chunks={} resident_chunks={} cpu_cache_chunks={}",
            viewer_started_at.elapsed().as_millis(),
            self.total_scene_chunks,
            self.resident_chunks.len(),
            self.cpu_mesh_cache.len()
        );
        println!(
            "[NATIVE_VIEWER] prebuild_completed elapsed_ms={} ready_file={} visible=true",
            viewer_started_at.elapsed().as_millis(),
            self.ready_file
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "none".to_string())
        );
    }

    fn write_prebuild_cache_file(&mut self) -> u128 {
        let cache_write_started_at = Instant::now();
        let Some(path) = self.cache_file.clone() else {
            return 0;
        };
        let Some(scene_index) = self.chunk_scene_index.clone() else {
            return 0;
        };
        let metadata = scene_index.metadata().clone();
        let (chunk_dir_name, chunk_dir) = if self.prebuild_cache_pipeline.is_some() {
            if let Some(pipeline) = self.prebuild_cache_pipeline.as_mut() {
                pipeline.request_tx.take();
            }
            self.drain_prebuild_cache_writer_results();
            if let Some(mut pipeline) = self.prebuild_cache_pipeline.take() {
                if let Some(worker) = pipeline.worker.take() {
                    let join_started_at = Instant::now();
                    if worker.join().is_err() {
                        println!(
                            "[PREVIEW_CACHE] cache_write_failed path={} error=writer_panic",
                            path.display()
                        );
                    }
                    println!(
                        "[TRACE_WALL] stage=cache_write event=writer_join elapsed_ms={}",
                        join_started_at.elapsed().as_millis()
                    );
                }
                while let Ok(result) = pipeline.result_rx.try_recv() {
                    self.apply_cache_writer_result(result);
                }
                (pipeline.chunk_dir_name, pipeline.chunk_dir)
            } else {
                return cache_write_started_at.elapsed().as_millis();
            }
        } else {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let manifest_name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "native_preview_cache.json".to_string());
            let chunk_dir_name = format!("{manifest_name}.chunks");
            let chunk_dir = path
                .parent()
                .map(|parent| parent.join(&chunk_dir_name))
                .unwrap_or_else(|| PathBuf::from(&chunk_dir_name));
            let create_started_at = Instant::now();
            let _ = std::fs::remove_dir_all(&chunk_dir);
            if let Err(error) = std::fs::create_dir_all(&chunk_dir) {
                println!(
                    "[PREVIEW_CACHE] cache_write_failed path={} error=create_chunk_dir:{}",
                    path.display(),
                    error
                );
                return cache_write_started_at.elapsed().as_millis();
            }
            TraceCounters::add_ms(
                &self.trace.cache_file_create_ms,
                create_started_at.elapsed(),
            );
            (chunk_dir_name, chunk_dir)
        };

        let mut total_chunk_bytes = self.prebuild_chunk_bytes;
        let missing_meshes = self
            .cpu_mesh_cache
            .entries
            .values()
            .filter(|entry| !self.prebuild_cache_written_keys.contains(&entry.mesh.key))
            .map(|entry| entry.mesh.clone())
            .collect::<Vec<_>>();
        if !missing_meshes.is_empty() {
            println!(
                "[TRACE_WALL] stage=cache_write event=write_missing_chunks count={}",
                missing_meshes.len()
            );
        }
        for mesh in missing_meshes {
            let file_name = cache_chunk_file_name(mesh.key);
            let chunk_path = chunk_dir.join(&file_name);
            let chunk_write_started_at = Instant::now();
            let chunk_bytes = match write_prepared_cache_chunk_binary(&chunk_path, &mesh) {
                Ok(bytes) => bytes,
                Err(error) => {
                    println!(
                        "[PREVIEW_CACHE] cache_write_failed path={} chunk_file={} error={}",
                        path.display(),
                        chunk_path.display(),
                        error
                    );
                    return cache_write_started_at.elapsed().as_millis();
                }
            };
            TraceCounters::add_ms(&self.trace.writer_busy_ms, chunk_write_started_at.elapsed());
            self.trace.writer_chunks.fetch_add(1, Ordering::Relaxed);
            total_chunk_bytes = total_chunk_bytes.saturating_add(chunk_bytes);
            self.prebuild_cache_written_keys.insert(mesh.key);
            self.prebuild_manifest_chunks
                .push(NativePreviewCacheManifestChunk {
                    cx: mesh.key.cx,
                    cy: mesh.key.cy,
                    cz: mesh.key.cz,
                    file: file_name,
                    vertex_count: mesh.vertex_count(),
                    index_count: mesh.index_count(),
                    bytes: chunk_bytes,
                });
        }
        self.prebuild_manifest_chunks
            .sort_by_key(|chunk| (chunk.cy, chunk.cx, chunk.cz));
        let manifest_chunks = std::mem::take(&mut self.prebuild_manifest_chunks);
        let layer_index_file = match write_native_cache_layer_index(&path, &scene_index) {
            Ok(file_name) => Some(file_name),
            Err(error) => {
                println!(
                    "[LAYER_CACHE] layer_index_failed manifest_path={} error={}",
                    path.display(),
                    error
                );
                return cache_write_started_at.elapsed().as_millis();
            }
        };

        let payload = NativePreviewCacheManifest {
            format: "native_preview_cache_manifest_v3".to_string(),
            color_chain: "shared_native_cache".to_string(),
            metadata,
            total_chunks: self.total_scene_chunks,
            renderable_chunks: manifest_chunks.len(),
            empty_mesh_chunks: self.prebuild_empty_mesh_keys.len(),
            chunk_data_dir: chunk_dir_name.clone(),
            layer_index_file,
            chunks: manifest_chunks,
        };

        match File::create(&path)
            .map(BufWriter::new)
            .and_then(|mut writer| {
                serde_json::to_writer(&mut writer, &payload)
                    .map_err(std::io::Error::other)?;
                writer.flush()?;
                Ok(())
            })
            .and_then(|_| std::fs::metadata(&path).map(|metadata| metadata.len() as usize))
            .map_err(|error| anyhow!("write cache manifest failed: {error}"))
            .map(|manifest_bytes| {
                println!(
                    "[PREVIEW_CACHE] chunked_cache_manifest path={} chunk_dir={} renderable_chunks={} empty_mesh_chunks={} total_chunks={} color_chain=shared_native_cache",
                    path.display(),
                    chunk_dir.display(),
                    payload.renderable_chunks,
                    payload.empty_mesh_chunks,
                    payload.total_chunks
                );
                println!(
                    "[PREVIEW_CACHE] chunked_cache_written manifest_path={} chunk_dir={} chunk_files={} manifest_bytes={} chunk_bytes={} total_bytes={} cache_write_ms={}",
                    path.display(),
                    chunk_dir.display(),
                    payload.chunks.len(),
                    manifest_bytes,
                    total_chunk_bytes,
                    manifest_bytes + total_chunk_bytes,
                    cache_write_started_at.elapsed().as_millis()
                );
            })
        {
            Ok(()) => {}
            Err(error) => {
                println!(
                    "[PREVIEW_CACHE] cache_write_failed path={} error={}",
                    path.display(),
                    error
                );
            }
        }
        cache_write_started_at.elapsed().as_millis()
    }

    fn log_prebuild_tail_diagnostics(&mut self, viewer_started_at: Instant, force: bool) {
        if !self.prebuild_before_show || self.prebuild_completed {
            return;
        }
        let now = Instant::now();
        if !force && now.duration_since(self.prebuild_last_tail_log) < PREBUILD_TAIL_LOG_INTERVAL {
            return;
        }
        self.prebuild_last_tail_log = now;

        let completed = self.prebuild_ready_chunk_count();
        if completed > self.prebuild_last_completed_count {
            self.prebuild_last_completed_count = completed;
            self.prebuild_last_completed_at = now;
        }
        let remaining_chunks = self.total_scene_chunks.saturating_sub(completed);
        let completed_not_applied = self
            .prebuild_finished_keys
            .iter()
            .filter(|key| {
                !self.resident_chunks.contains_key(key)
                    && !self.prebuild_empty_mesh_keys.contains(key)
            })
            .count();
        let heaviest_remaining_chunks = self
            .chunk_scene_index
            .as_ref()
            .map(|scene_index| {
                let mut remaining = scene_index
                    .chunk_entries()
                    .iter()
                    .filter(|entry| {
                        !self.resident_chunks.contains_key(&entry.key)
                            && !self.prebuild_empty_mesh_keys.contains(&entry.key)
                    })
                    .map(|entry| (entry.non_air_blocks, entry.key))
                    .collect::<Vec<_>>();
                remaining.sort_by_key(|(non_air_blocks, key)| {
                    (std::cmp::Reverse(*non_air_blocks), key.cy, key.cx, key.cz)
                });
                remaining
                    .into_iter()
                    .take(6)
                    .map(|(non_air_blocks, key)| {
                        format!("({}, {}, {}):{}", key.cx, key.cy, key.cz, non_air_blocks)
                    })
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .unwrap_or_else(|| "none".to_string());

        println!(
            "[PREBUILD_TAIL] pending_builds={} in_flight_builds={} pending_uploads={} completed_not_applied={} remaining_chunks={} heaviest_remaining_chunks={} finished_chunks={} empty_mesh_chunks={} resident_chunks={} elapsed_ms={}",
            self.pending_build_queue.len(),
            self.in_flight_build_keys.len(),
            self.pending_upload_queue.len(),
            completed_not_applied,
            remaining_chunks,
            heaviest_remaining_chunks,
            completed,
            self.prebuild_empty_mesh_keys.len(),
            self.resident_chunks.len(),
            viewer_started_at.elapsed().as_millis()
        );

        let stalled_for = now.duration_since(self.prebuild_last_completed_at);
        if remaining_chunks > 0 && stalled_for >= Duration::from_secs(PREBUILD_TAIL_STALL_SECONDS) {
            println!(
                "[PREBUILD_TAIL] tail_stall_detected stalled_seconds={} remaining_chunks={} pending_builds={} in_flight_builds={} pending_uploads={} completed_not_applied={}",
                stalled_for.as_secs(),
                remaining_chunks,
                self.pending_build_queue.len(),
                self.in_flight_build_keys.len(),
                self.pending_upload_queue.len(),
                completed_not_applied
            );
        }
    }

    fn run_prebuild_until_ready(
        &mut self,
        window: &winit::window::Window,
        viewer_started_at: Instant,
    ) -> Result<()> {
        if !self.prebuild_before_show || self.prebuild_completed {
            return Ok(());
        }
        let mut last_log = Instant::now();
        loop {
            if self.residency_dirty {
                self.sync_chunk_residency()?;
            }
            let async_stats = self.consume_completed_build_results();
            let writer_completed = self.drain_prebuild_cache_writer_results();
            if writer_completed > 0 {
                println!(
                    "[TRACE_WALL] stage=cache_write event=drain completed={} total_completed={} submitted={}",
                    writer_completed,
                    self.prebuild_writer_completed_chunks,
                    self.prebuild_writer_submitted_chunks
                );
            }
            if async_stats.completed_builds > 0
                || async_stats.dropped_build_results > 0
                || async_stats.retained_build_results > 0
                || async_stats.reused_completed_results > 0
            {
                println!(
                    "[VIEWER_ASYNC] worker_count={} completed_builds={} dropped_build_results={} retained_build_results={} reused_completed_results={} build_result_to_cache={} build_result_to_upload={} in_flight_builds={} max_in_flight_builds={}",
                    self.build_worker_count,
                    async_stats.completed_builds,
                    async_stats.dropped_build_results,
                    async_stats.retained_build_results,
                    async_stats.reused_completed_results,
                    async_stats.build_result_to_cache,
                    async_stats.build_result_to_upload,
                    self.in_flight_build_keys.len(),
                    self.max_in_flight_builds
                );
            }
            if let Some(scene_index) = self.chunk_scene_index.as_ref().cloned() {
                self.process_pending_builds(&scene_index)?;
            }
            let upload_stats = self.process_pending_uploads();
            self.prune_cache_pressure("prebuild");
            self.touch_target_resident_chunks();
            if self.prebuild_is_ready() {
                self.finalize_prebuild(window, viewer_started_at);
                break;
            }
            self.write_prebuild_progress_signal(viewer_started_at.elapsed(), false, false);
            self.log_prebuild_tail_diagnostics(viewer_started_at, false);
            if last_log.elapsed() >= Duration::from_secs(1) {
                last_log = Instant::now();
                println!(
                    "[VIEWER_BOOTSTRAP] prebuild_progress elapsed_ms={} resident_chunks={} total_chunks={} cpu_cache_chunks={} pending_builds={} in_flight_builds={} pending_uploads={} uploaded_chunks_this_round={}",
                    viewer_started_at.elapsed().as_millis(),
                    self.resident_chunks.len(),
                    self.total_scene_chunks,
                    self.cpu_mesh_cache.len(),
                    self.pending_build_queue.len(),
                    self.in_flight_build_keys.len(),
                    self.pending_upload_queue.len(),
                    upload_stats.target_usage.chunks + upload_stats.preload_usage.chunks
                );
            }
            thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }

    fn upload_chunk_mesh(&self, prepared: &PreparedChunkMesh) -> ResidentChunkMesh {
        log_chest_front_gpu_vertices("upload", &prepared.vertices, &prepared.indices);
        let vertex_count = prepared.vertices.len() as u32;
        let solid_index_count = prepared.indices.len() as u32;
        let translucent_index_count = prepared.translucent_indices.len() as u32;
        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("native_viewer_chunk_vertices"),
                contents: bytemuck::cast_slice(&prepared.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let solid_index_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("native_viewer_chunk_indices_solid"),
                    contents: bytemuck::cast_slice(&prepared.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
        let translucent_index_buffer = (!prepared.translucent_indices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("native_viewer_chunk_indices_translucent"),
                    contents: bytemuck::cast_slice(&prepared.translucent_indices),
                    usage: wgpu::BufferUsages::INDEX,
                })
        });
        ResidentChunkMesh {
            key: prepared.key,
            bounds: prepared.bounds,
            gpu: GpuChunkBuffer {
                vertex_buffer,
                solid_index_buffer,
                translucent_index_buffer,
                vertex_count,
                solid_index_count,
                translucent_index_count,
            },
            last_touched_frame: self.frame_index,
            estimated_bytes: prepared.estimated_bytes(),
        }
    }

    fn enqueue_chunk_upload_with_cost(
        &mut self,
        key: ChunkKey,
        mesh_cost: Option<(usize, usize)>,
        priority: QueuePriority,
    ) -> bool {
        if self.prebuild_cache_only {
            return false;
        }
        if self.resident_chunks.contains_key(&key) {
            return false;
        }
        if self.pending_upload_keys.contains(&key) {
            self.promote_pending_upload_priority(key, priority);
            return false;
        }
        let (vertex_count, index_count) = mesh_cost
            .or_else(|| self.cpu_mesh_cache.mesh_cost(key, self.frame_index))
            .unwrap_or((0, 0));
        self.pending_upload_queue.push_back(PendingChunkUpload {
            key,
            enqueued_frame: self.frame_index,
            vertex_count,
            index_count,
            priority,
        });
        self.pending_upload_keys.insert(key);
        TraceCounters::update_max(
            &self.trace.max_pending_upload_queue,
            self.pending_upload_queue.len(),
        );
        true
    }

    fn enqueue_chunk_build(
        &mut self,
        key: ChunkKey,
        estimated_vertex_count: usize,
        estimated_index_count: usize,
        priority: QueuePriority,
    ) -> bool {
        if self.cpu_mesh_cache.contains(key) || self.in_flight_build_keys.contains(&key) {
            return false;
        }
        if self.pending_build_keys.contains(&key) {
            self.promote_pending_build_priority(key, priority);
            return false;
        }
        self.pending_build_queue.push_back(PendingChunkBuild {
            key,
            enqueued_frame: self.frame_index,
            estimated_vertex_count,
            estimated_index_count,
            priority,
        });
        self.pending_build_keys.insert(key);
        TraceCounters::update_max(
            &self.trace.max_pending_build_queue,
            self.pending_build_queue.len(),
        );
        true
    }

    fn touch_warm_cache_key(&mut self, key: ChunkKey) {
        self.warm_cache_keys.insert(key, self.frame_index);
    }

    fn retain_warm_cache_keys(&mut self) {
        let warm_cache_frames = self.streaming_config.warm_cache_frames;
        let frame_index = self.frame_index;
        self.warm_cache_keys.retain(|_, last_seen_frame| {
            frame_index.saturating_sub(*last_seen_frame) <= warm_cache_frames
        });
    }

    fn is_warm_cache_key(&self, key: ChunkKey) -> bool {
        self.warm_cache_keys
            .get(&key)
            .map(|last_seen_frame| {
                self.frame_index.saturating_sub(*last_seen_frame)
                    <= self.streaming_config.warm_cache_frames
            })
            .unwrap_or(false)
    }

    fn promote_pending_build_priority(&mut self, key: ChunkKey, priority: QueuePriority) {
        if priority != QueuePriority::Target {
            return;
        }
        if let Some(pending) = self
            .pending_build_queue
            .iter_mut()
            .find(|pending| pending.key == key)
        {
            pending.priority = QueuePriority::Target;
        }
    }

    fn promote_pending_upload_priority(&mut self, key: ChunkKey, priority: QueuePriority) {
        if priority != QueuePriority::Target {
            return;
        }
        if let Some(pending) = self
            .pending_upload_queue
            .iter_mut()
            .find(|pending| pending.key == key)
        {
            pending.priority = QueuePriority::Target;
        }
    }

    fn current_queue_priority(&self, key: ChunkKey) -> Option<QueuePriority> {
        if self.current_target_keys.contains(&key) {
            Some(QueuePriority::Target)
        } else if self.current_preload_keys.contains(&key)
            || self.current_retain_keys.contains(&key)
            || self.is_warm_cache_key(key)
        {
            Some(QueuePriority::Preload)
        } else {
            None
        }
    }

    fn refresh_sticky_target_keys(&mut self, plan: &ChunkResidencyPlan) {
        self.sticky_target_keys.retain(|key, last_target_frame| {
            self.frame_index.saturating_sub(*last_target_frame)
                <= self.streaming_config.sticky_target_frames
                && plan.retain_keys.contains(key)
        });
        for &key in &plan.target_keys {
            self.sticky_target_keys.insert(key, self.frame_index);
        }
        for &key in self.sticky_target_keys.keys() {
            if plan.retain_keys.contains(&key) {
                self.current_target_keys.insert(key);
            }
        }
    }

    fn build_budget_split(&self) -> BudgetSplit<BuildBudget> {
        if self.prebuild_before_show {
            let full_budget = BuildBudget {
                max_chunks_per_frame: self.total_scene_chunks.max(1),
                max_vertices_per_frame: usize::MAX / 8,
                max_indices_per_frame: usize::MAX / 8,
            };
            return BudgetSplit {
                target: full_budget,
                preload: BuildBudget {
                    max_chunks_per_frame: 0,
                    max_vertices_per_frame: 0,
                    max_indices_per_frame: 0,
                },
            };
        }
        BudgetSplit {
            target: self
                .build_budget
                .saturating_sub(self.streaming_config.preload_build_soft_budget),
            preload: self.streaming_config.preload_build_soft_budget,
        }
    }

    fn upload_budget_split(&self) -> BudgetSplit<UploadBudget> {
        if self.prebuild_before_show {
            let full_budget = UploadBudget {
                max_chunks_per_frame: self.total_scene_chunks.max(1),
                max_vertices_per_frame: usize::MAX / 8,
                max_indices_per_frame: usize::MAX / 8,
            };
            return BudgetSplit {
                target: full_budget,
                preload: UploadBudget {
                    max_chunks_per_frame: 0,
                    max_vertices_per_frame: 0,
                    max_indices_per_frame: 0,
                },
            };
        }
        BudgetSplit {
            target: self
                .upload_budget
                .saturating_sub(self.streaming_config.preload_upload_soft_budget),
            preload: self.streaming_config.preload_upload_soft_budget,
        }
    }

    fn estimate_chunk_build_cost(scene_index: &ChunkSceneIndex, key: ChunkKey) -> (usize, usize) {
        let non_air_blocks = scene_index.non_air_blocks(key).unwrap_or(0) as usize;
        let estimated_vertices = non_air_blocks.saturating_mul(24).max(24);
        let estimated_indices = non_air_blocks.saturating_mul(36).max(36);
        (estimated_vertices, estimated_indices)
    }

    fn queue_chunks_for_priority(
        &mut self,
        scene_index: &ChunkSceneIndex,
        order: &[ChunkKey],
        priority: QueuePriority,
    ) -> (usize, usize, usize, usize, usize, usize, usize, usize) {
        let mut cache_hits = 0_usize;
        let mut cache_misses = 0_usize;
        let mut already_pending_builds = 0_usize;
        let mut queued_builds = 0_usize;
        let mut queued_uploads = 0_usize;
        let mut reenter_chunks = 0_usize;
        let mut cache_hit_reuploads = 0_usize;
        let mut cache_miss_rebuilds = 0_usize;

        for key in order {
            if self.resident_chunks.contains_key(key) {
                self.touch_warm_cache_key(*key);
                continue;
            }
            let is_reenter = self.is_warm_cache_key(*key);
            if is_reenter {
                reenter_chunks += 1;
            }
            if self.cpu_mesh_cache.contains(*key) {
                cache_hits += 1;
                if is_reenter {
                    cache_hit_reuploads += 1;
                }
                self.cpu_mesh_cache.touch(*key, self.frame_index);
                self.touch_warm_cache_key(*key);
                if self.enqueue_chunk_upload_with_cost(*key, None, priority) {
                    queued_uploads += 1;
                }
            } else {
                cache_misses += 1;
                if is_reenter {
                    cache_miss_rebuilds += 1;
                }
                if self.pending_build_keys.contains(key) || self.in_flight_build_keys.contains(key)
                {
                    already_pending_builds += 1;
                    self.promote_pending_build_priority(*key, priority);
                } else {
                    let (estimated_vertex_count, estimated_index_count) =
                        Self::estimate_chunk_build_cost(scene_index, *key);
                    if self.enqueue_chunk_build(
                        *key,
                        estimated_vertex_count,
                        estimated_index_count,
                        priority,
                    ) {
                        queued_builds += 1;
                    }
                }
            }
        }

        (
            cache_hits,
            cache_misses,
            already_pending_builds,
            queued_builds,
            queued_uploads,
            reenter_chunks,
            cache_hit_reuploads,
            cache_miss_rebuilds,
        )
    }

    fn prepare_target_chunk_meshes(
        &mut self,
        scene_index: &ChunkSceneIndex,
        plan: &ChunkResidencyPlan,
    ) -> QueuePreparationStats {
        let (
            target_cache_hits,
            target_cache_misses,
            target_pending_hits,
            target_queued_builds,
            target_queued_uploads,
            target_reenter_chunks,
            target_cache_hit_reuploads,
            target_cache_miss_rebuilds,
        ) = self.queue_chunks_for_priority(scene_index, &plan.upload_order, QueuePriority::Target);
        let (
            preload_cache_hits,
            preload_cache_misses,
            preload_pending_hits,
            preload_queued_builds,
            preload_queued_uploads,
            preload_reenter_chunks,
            preload_cache_hit_reuploads,
            preload_cache_miss_rebuilds,
        ) = self.queue_chunks_for_priority(
            scene_index,
            &plan.preload_order,
            QueuePriority::Preload,
        );
        QueuePreparationStats {
            target_cache_hits,
            target_cache_misses,
            target_pending_hits,
            target_queued_builds,
            target_queued_uploads,
            target_reenter_chunks,
            target_cache_hit_reuploads,
            target_cache_miss_rebuilds,
            preload_cache_hits,
            preload_cache_misses,
            preload_pending_hits,
            preload_queued_builds,
            preload_queued_uploads,
            preload_reenter_chunks,
            preload_cache_hit_reuploads,
            preload_cache_miss_rebuilds,
        }
    }

    fn process_pending_builds(
        &mut self,
        scene_index: &ChunkSceneIndex,
    ) -> Result<QueueProcessingStats> {
        let budget_split = self.build_budget_split();
        let mut stats = QueueProcessingStats::default();
        let mut target_queue = VecDeque::<PendingChunkBuild>::new();
        let mut preload_queue = VecDeque::<PendingChunkBuild>::new();
        while let Some(mut pending) = self.pending_build_queue.pop_front() {
            self.pending_build_keys.remove(&pending.key);
            if let Some(priority) = self.current_queue_priority(pending.key) {
                pending.priority = priority;
                match priority {
                    QueuePriority::Target => target_queue.push_back(pending),
                    QueuePriority::Preload => preload_queue.push_back(pending),
                }
            }
        }

        let mut next_queue = VecDeque::<PendingChunkBuild>::new();
        let mut prebuild_batch = Vec::<PendingChunkBuild>::new();
        for mut pending in target_queue.into_iter().chain(preload_queue) {
            let Some(priority) = self.current_queue_priority(pending.key) else {
                continue;
            };
            pending.priority = priority;

            if self.cpu_mesh_cache.contains(pending.key) {
                self.cpu_mesh_cache.touch(pending.key, self.frame_index);
                if self.enqueue_chunk_upload_with_cost(pending.key, None, priority) {
                    match priority {
                        QueuePriority::Target => stats.target_submitted_uploads += 1,
                        QueuePriority::Preload => stats.preload_submitted_uploads += 1,
                    }
                }
                continue;
            }
            if self.in_flight_build_keys.contains(&pending.key) {
                continue;
            }
            if self.in_flight_build_keys.len() >= self.max_in_flight_builds {
                match priority {
                    QueuePriority::Target => stats.deferred_target_chunks += 1,
                    QueuePriority::Preload => stats.deferred_preload_chunks += 1,
                }
                next_queue.push_back(pending);
                self.pending_build_keys.insert(pending.key);
                continue;
            }

            let usage = match priority {
                QueuePriority::Target => &mut stats.target_usage,
                QueuePriority::Preload => &mut stats.preload_usage,
            };
            let budget = match priority {
                QueuePriority::Target => budget_split.target,
                QueuePriority::Preload => budget_split.preload,
            };
            if !(*usage).can_fit(
                pending.estimated_vertex_count,
                pending.estimated_index_count,
                budget.max_chunks_per_frame,
                budget.max_vertices_per_frame,
                budget.max_indices_per_frame,
            ) {
                match priority {
                    QueuePriority::Target => stats.deferred_target_chunks += 1,
                    QueuePriority::Preload => stats.deferred_preload_chunks += 1,
                }
                next_queue.push_back(pending);
                self.pending_build_keys.insert(pending.key);
                continue;
            }

            if let Some(request_tx) = self.build_request_tx.clone() {
                if self.prebuild_before_show {
                    prebuild_batch.push(pending);
                    if prebuild_batch.len() >= PREBUILD_BUILD_BATCH_SIZE {
                        let submitted =
                            self.submit_build_batch(&request_tx, &prebuild_batch, priority)?;
                        for pending in prebuild_batch.drain(..) {
                            usage.consume(
                                pending.estimated_vertex_count,
                                pending.estimated_index_count,
                            );
                        }
                        match priority {
                            QueuePriority::Target => stats.target_submitted_builds += submitted,
                            QueuePriority::Preload => stats.preload_submitted_builds += submitted,
                        }
                    }
                } else {
                    let submitted = self.submit_build_batch(
                        &request_tx,
                        std::slice::from_ref(&pending),
                        priority,
                    )?;
                    usage.consume(
                        pending.estimated_vertex_count,
                        pending.estimated_index_count,
                    );
                    match priority {
                        QueuePriority::Target => stats.target_submitted_builds += submitted,
                        QueuePriority::Preload => stats.preload_submitted_builds += submitted,
                    }
                }
            }
        }
        if !prebuild_batch.is_empty()
            && let Some(request_tx) = self.build_request_tx.clone()
        {
            let priority = if prebuild_batch
                .iter()
                .any(|pending| pending.priority == QueuePriority::Target)
            {
                QueuePriority::Target
            } else {
                QueuePriority::Preload
            };
            let submitted = self.submit_build_batch(&request_tx, &prebuild_batch, priority)?;
            for pending in &prebuild_batch {
                match priority {
                    QueuePriority::Target => stats.target_usage.consume(
                        pending.estimated_vertex_count,
                        pending.estimated_index_count,
                    ),
                    QueuePriority::Preload => stats.preload_usage.consume(
                        pending.estimated_vertex_count,
                        pending.estimated_index_count,
                    ),
                }
            }
            match priority {
                QueuePriority::Target => stats.target_submitted_builds += submitted,
                QueuePriority::Preload => stats.preload_submitted_builds += submitted,
            }
        }

        self.pending_build_queue = next_queue;
        self.pending_build_keys = self
            .pending_build_queue
            .iter()
            .map(|pending| pending.key)
            .collect();
        let _ = scene_index;
        Ok(stats)
    }

    fn submit_build_batch(
        &mut self,
        request_tx: &Sender<BuildWorkerRequest>,
        pending_batch: &[PendingChunkBuild],
        priority: QueuePriority,
    ) -> Result<usize> {
        let keys = pending_batch
            .iter()
            .map(|pending| pending.key)
            .collect::<Vec<_>>();
        if keys.is_empty() {
            return Ok(0);
        }
        let batch_id = self.next_build_batch_id;
        self.next_build_batch_id = self.next_build_batch_id.saturating_add(1);
        let send_started_at = Instant::now();
        request_tx
            .send(BuildWorkerRequest {
                batch_id,
                keys: keys.clone(),
                priority,
                enqueued_at: Instant::now(),
            })
            .map_err(|error| anyhow!("failed to submit async build request: {error}"))?;
        TraceCounters::add_ms(&self.trace.build_enqueue_ms, send_started_at.elapsed());
        self.trace
            .build_enqueue_batches
            .fetch_add(1, Ordering::Relaxed);
        self.trace
            .build_enqueue_chunks
            .fetch_add(keys.len(), Ordering::Relaxed);
        for key in &keys {
            self.in_flight_build_keys.insert(*key);
        }
        TraceCounters::update_max(
            &self.trace.max_in_flight_builds,
            self.in_flight_build_keys.len(),
        );
        println!(
            "[TRACE_WALL] stage=chunk_task event=enqueue batch_id={} chunks={} priority={:?} enqueue_ms={} pending_builds={} in_flight_builds={}",
            batch_id,
            keys.len(),
            priority,
            send_started_at.elapsed().as_millis(),
            self.pending_build_queue.len(),
            self.in_flight_build_keys.len()
        );
        if self.prebuild_before_show {
            println!(
                "[PREBUILD_TAIL] batch_submitted batch_id={} chunks={} priority={:?} pending_builds={} in_flight_builds={}",
                batch_id,
                keys.len(),
                priority,
                self.pending_build_queue.len(),
                self.in_flight_build_keys.len()
            );
        }
        Ok(keys.len())
    }

    fn submit_prebuild_cache_write(
        &mut self,
        prepared: PreparedChunkMesh,
        priority: QueuePriority,
    ) -> Result<(), PreparedChunkMesh> {
        let Some(pipeline) = self.prebuild_cache_pipeline.as_ref() else {
            return Err(prepared);
        };
        let Some(request_tx) = pipeline.request_tx.as_ref() else {
            return Err(prepared);
        };
        let key = prepared.key;
        match request_tx.send(CacheWriterRequest {
            mesh: prepared,
            priority,
            submitted_at: Instant::now(),
        }) {
            Ok(()) => {
                self.prebuild_writer_submitted_chunks =
                    self.prebuild_writer_submitted_chunks.saturating_add(1);
                println!(
                    "[TRACE_WALL] stage=cache_write event=enqueue key=({}, {}, {}) submitted_chunks={} completed_chunks={}",
                    key.cx,
                    key.cy,
                    key.cz,
                    self.prebuild_writer_submitted_chunks,
                    self.prebuild_writer_completed_chunks
                );
                Ok(())
            }
            Err(error) => {
                let failed_request = error.0;
                println!(
                    "[PREVIEW_CACHE] cache_write_failed key=({}, {}, {}) error=writer_send:result_channel_closed",
                    key.cx, key.cy, key.cz
                );
                Err(failed_request.mesh)
            }
        }
    }

    fn prebuild_cache_pipeline_active(&self) -> bool {
        self.prebuild_cache_pipeline
            .as_ref()
            .and_then(|pipeline| pipeline.request_tx.as_ref())
            .is_some()
    }

    fn apply_cache_writer_result(&mut self, result: CacheWriterResult) -> bool {
        let key = result.mesh.key;
        if let Some(error) = result.error.as_ref() {
            println!(
                "[PREVIEW_CACHE] cache_write_failed key=({}, {}, {}) error={}",
                key.cx, key.cy, key.cz, error
            );
        }
        if let Some(manifest_chunk) = result.manifest_chunk
            && self.prebuild_cache_written_keys.insert(key)
        {
            self.prebuild_chunk_bytes = self.prebuild_chunk_bytes.saturating_add(result.bytes);
            self.prebuild_manifest_chunks.push(manifest_chunk);
        }
        let vertex_count = result.mesh.vertex_count();
        let index_count = result.mesh.index_count();
        self.cpu_mesh_cache.insert(result.mesh, self.frame_index);
        if self.prebuild_before_show {
            self.prebuild_finished_keys.insert(key);
        }
        self.touch_warm_cache_key(key);
        self.prebuild_writer_completed_chunks =
            self.prebuild_writer_completed_chunks.saturating_add(1);
        println!(
            "[TRACE_WALL] stage=cache_write event=complete key=({}, {}, {}) wait_ms={} write_ms={} completed_chunks={} submitted_chunks={}",
            key.cx,
            key.cy,
            key.cz,
            result.wait_ms,
            result.write_ms,
            self.prebuild_writer_completed_chunks,
            self.prebuild_writer_submitted_chunks
        );
        if !self.prebuild_cache_only {
            self.enqueue_chunk_upload_with_cost(
                key,
                Some((vertex_count, index_count)),
                result.priority,
            )
        } else {
            false
        }
    }

    fn drain_prebuild_cache_writer_results(&mut self) -> usize {
        let mut completed = 0_usize;
        loop {
            let result = match self.prebuild_cache_pipeline.as_ref() {
                Some(pipeline) => match pipeline.result_rx.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                },
                None => break,
            };
            self.apply_cache_writer_result(result);
            completed += 1;
        }
        completed
    }

    fn consume_completed_build_results(&mut self) -> AsyncBuildDrainStats {
        let mut stats = AsyncBuildDrainStats::default();
        loop {
            let result = match self.build_result_rx.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            };
            self.in_flight_build_keys.remove(&result.key);
            stats.completed_builds += 1;

            if let Some(error) = result.error {
                println!(
                    "[VIEWER_ASYNC] build_failed batch_id={} key=({}, {}, {}) priority={:?} error={}",
                    result.batch_id,
                    result.key.cx,
                    result.key.cy,
                    result.key.cz,
                    result.priority,
                    error
                );
                stats.dropped_build_results += 1;
                if self.prebuild_before_show && !self.prebuild_finished_keys.contains(&result.key) {
                    let (estimated_vertex_count, estimated_index_count) =
                        if let Some(scene_index) = self.chunk_scene_index.as_ref() {
                            Self::estimate_chunk_build_cost(scene_index, result.key)
                        } else {
                            (24, 36)
                        };
                    if self.enqueue_chunk_build(
                        result.key,
                        estimated_vertex_count,
                        estimated_index_count,
                        QueuePriority::Target,
                    ) {
                        println!(
                            "[PREBUILD_TAIL] retry_queued key=({}, {}, {}) reason=build_error",
                            result.key.cx, result.key.cy, result.key.cz
                        );
                    }
                }
                continue;
            }

            let Some(prepared) = result.mesh else {
                if self.prebuild_before_show {
                    self.prebuild_empty_mesh_keys.insert(result.key);
                    self.prebuild_finished_keys.insert(result.key);
                    stats.retained_build_results += 1;
                    stats.build_result_to_cache += 1;
                    println!(
                        "[PREBUILD_TAIL] empty_mesh_completed batch_id={} key=({}, {}, {}) finished_chunks={} total_chunks={}",
                        result.batch_id,
                        result.key.cx,
                        result.key.cy,
                        result.key.cz,
                        self.prebuild_completed_chunk_count(),
                        self.total_scene_chunks
                    );
                } else {
                    stats.dropped_build_results += 1;
                }
                continue;
            };

            let current_priority = self.current_queue_priority(prepared.key);
            let retained_by_stability = !self.current_target_keys.contains(&prepared.key)
                && !self.current_preload_keys.contains(&prepared.key)
                && (self.current_retain_keys.contains(&prepared.key)
                    || self.resident_chunks.contains_key(&prepared.key)
                    || self.is_warm_cache_key(prepared.key));
            let should_cache = current_priority.is_some() || retained_by_stability;
            if !should_cache {
                stats.dropped_build_results += 1;
                if self.prebuild_before_show && !self.prebuild_finished_keys.contains(&prepared.key)
                {
                    let mut prepared = prepared;
                    if self.prebuild_cache_pipeline_active() {
                        match self.submit_prebuild_cache_write(prepared, result.priority) {
                            Ok(()) => {
                                stats.retained_build_results += 1;
                                stats.build_result_to_cache += 1;
                                continue;
                            }
                            Err(returned) => prepared = returned,
                        }
                    }
                    let key = prepared.key;
                    self.cpu_mesh_cache.insert(prepared, self.frame_index);
                    self.prebuild_finished_keys.insert(key);
                    self.touch_warm_cache_key(key);
                    stats.retained_build_results += 1;
                    stats.build_result_to_cache += 1;
                    if self.enqueue_chunk_upload_with_cost(key, None, QueuePriority::Target) {
                        stats.build_result_to_upload += 1;
                    }
                }
                continue;
            }
            if retained_by_stability {
                stats.retained_build_results += 1;
                stats.reused_completed_results += 1;
            }

            let key = prepared.key;
            let vertex_count = prepared.vertex_count();
            let index_count = prepared.index_count();
            let mut prepared = prepared;
            if self.prebuild_before_show && self.prebuild_cache_pipeline_active() {
                match self.submit_prebuild_cache_write(prepared, result.priority) {
                    Ok(()) => {
                        stats.build_result_to_cache += 1;
                        continue;
                    }
                    Err(returned) => prepared = returned,
                }
            }
            self.cpu_mesh_cache.insert(prepared, self.frame_index);
            if self.prebuild_before_show {
                self.prebuild_finished_keys.insert(key);
            }
            self.touch_warm_cache_key(key);
            stats.build_result_to_cache += 1;

            if let Some(priority) = current_priority {
                if self.enqueue_chunk_upload_with_cost(
                    key,
                    Some((vertex_count, index_count)),
                    priority,
                ) {
                    stats.build_result_to_upload += 1;
                }
            } else if self.is_warm_cache_key(key) {
                stats.reused_completed_results += 1;
            }
        }
        stats
    }

    fn process_pending_uploads(&mut self) -> QueueProcessingStats {
        if self.prebuild_cache_only {
            self.pending_upload_queue.clear();
            self.pending_upload_keys.clear();
            return QueueProcessingStats::default();
        }
        let budget_split = self.upload_budget_split();
        let mut stats = QueueProcessingStats::default();
        let mut target_queue = VecDeque::<PendingChunkUpload>::new();
        let mut preload_queue = VecDeque::<PendingChunkUpload>::new();
        while let Some(mut pending) = self.pending_upload_queue.pop_front() {
            self.pending_upload_keys.remove(&pending.key);
            if let Some(priority) = self.current_queue_priority(pending.key) {
                pending.priority = priority;
                match priority {
                    QueuePriority::Target => target_queue.push_back(pending),
                    QueuePriority::Preload => preload_queue.push_back(pending),
                }
            }
        }

        let mut next_queue = VecDeque::<PendingChunkUpload>::new();
        for mut pending in target_queue.into_iter().chain(preload_queue) {
            let Some(priority) = self.current_queue_priority(pending.key) else {
                continue;
            };
            pending.priority = priority;

            if self.resident_chunks.contains_key(&pending.key) {
                continue;
            }
            if !self.cpu_mesh_cache.contains(pending.key) {
                continue;
            }

            let usage = match priority {
                QueuePriority::Target => &mut stats.target_usage,
                QueuePriority::Preload => &mut stats.preload_usage,
            };
            let budget = match priority {
                QueuePriority::Target => budget_split.target,
                QueuePriority::Preload => budget_split.preload,
            };
            if !(*usage).can_fit(
                pending.vertex_count,
                pending.index_count,
                budget.max_chunks_per_frame,
                budget.max_vertices_per_frame,
                budget.max_indices_per_frame,
            ) {
                let pending_key = pending.key;
                match priority {
                    QueuePriority::Target => stats.deferred_target_chunks += 1,
                    QueuePriority::Preload => stats.deferred_preload_chunks += 1,
                }
                next_queue.push_back(pending);
                self.pending_upload_keys.insert(pending_key);
                continue;
            }

            let Some(prepared) = self
                .cpu_mesh_cache
                .get_cloned(pending.key, self.frame_index)
            else {
                continue;
            };
            let resident = self.upload_chunk_mesh(&prepared);
            self.resident_chunks.insert(resident.key, resident);
            self.touch_warm_cache_key(pending.key);
            usage.consume(pending.vertex_count, pending.index_count);
        }

        self.pending_upload_queue = next_queue;
        self.pending_upload_keys = self
            .pending_upload_queue
            .iter()
            .map(|pending| pending.key)
            .collect();
        stats
    }

    fn prune_cpu_mesh_cache(&mut self) -> EvictionStats {
        if self.prebuild_before_show {
            return EvictionStats {
                candidate_count: 0,
                evicted_count: 0,
                evicted_recent_count: 0,
                before_count: self.cpu_mesh_cache.len(),
                after_count: self.cpu_mesh_cache.len(),
                before_bytes: self.cpu_mesh_cache.current_bytes,
                after_bytes: self.cpu_mesh_cache.current_bytes,
                evicted_bytes: 0,
            };
        }
        let mut protected_keys = self.current_target_keys.clone();
        protected_keys.extend(self.current_preload_keys.iter().copied());
        protected_keys.extend(self.current_retain_keys.iter().copied());
        protected_keys.extend(self.pending_build_keys.iter().copied());
        protected_keys.extend(self.in_flight_build_keys.iter().copied());
        protected_keys.extend(self.pending_upload_keys.iter().copied());
        protected_keys.extend(self.resident_chunks.keys().copied());
        protected_keys.extend(
            self.warm_cache_keys
                .iter()
                .filter_map(|(key, last_seen_frame)| {
                    (self.frame_index.saturating_sub(*last_seen_frame)
                        <= self.streaming_config.warm_cache_frames)
                        .then_some(*key)
                }),
        );
        let recent_cutoff_frame = self
            .frame_index
            .saturating_sub(self.streaming_config.warm_cache_frames);
        self.cpu_mesh_cache
            .prune(&protected_keys, recent_cutoff_frame)
    }

    fn prune_resident_chunks(&mut self, retain_keys: &HashSet<ChunkKey>) -> EvictionStats {
        if self.prebuild_before_show {
            let before_count = self.resident_chunks.len();
            let before_bytes = self
                .resident_chunks
                .values()
                .map(|resident| resident.estimated_bytes)
                .sum::<usize>();
            return EvictionStats {
                candidate_count: 0,
                evicted_count: 0,
                evicted_recent_count: 0,
                before_count,
                after_count: before_count,
                before_bytes,
                after_bytes: before_bytes,
                evicted_bytes: 0,
            };
        }
        let before_count = self.resident_chunks.len();
        let before_bytes = self
            .resident_chunks
            .values()
            .map(|resident| resident.estimated_bytes)
            .sum::<usize>();
        let incoming_upload_capacity = self
            .pending_upload_queue
            .len()
            .min(self.upload_budget.max_chunks_per_frame);
        let incoming_upload_bytes = self
            .pending_upload_queue
            .iter()
            .take(self.upload_budget.max_chunks_per_frame)
            .map(|pending| {
                pending
                    .vertex_count
                    .saturating_mul(size_of::<GpuVertex>())
                    .saturating_add(pending.index_count.saturating_mul(size_of::<u32>()))
                    .saturating_add(256)
            })
            .sum::<usize>();
        let required_after_eviction = before_count.saturating_add(incoming_upload_capacity);
        let required_evict_count = required_after_eviction.saturating_sub(self.max_resident_chunks);
        let required_after_eviction_bytes = before_bytes.saturating_add(incoming_upload_bytes);
        let required_evict_bytes =
            required_after_eviction_bytes.saturating_sub(self.max_resident_bytes);

        let mut evictable = self
            .resident_chunks
            .iter()
            .filter(|(key, _)| !retain_keys.contains(key))
            .map(|(key, resident)| (resident.last_touched_frame, resident.estimated_bytes, *key))
            .collect::<Vec<_>>();
        let candidate_count = evictable.len();

        if (required_evict_count == 0 && required_evict_bytes == 0) || candidate_count == 0 {
            return EvictionStats {
                candidate_count,
                evicted_count: 0,
                evicted_recent_count: 0,
                before_count,
                after_count: before_count,
                before_bytes,
                after_bytes: before_bytes,
                evicted_bytes: 0,
            };
        }

        evictable.sort_by_key(|(last_touched_frame, estimated_bytes, key)| {
            (*last_touched_frame, *estimated_bytes, *key)
        });

        let mut evicted_count = 0_usize;
        let mut evicted_bytes = 0_usize;
        let mut current_bytes = before_bytes;
        for (_, _, key) in evictable {
            if evicted_count >= required_evict_count && current_bytes <= self.max_resident_bytes {
                break;
            }
            if let Some(removed) = self.resident_chunks.remove(&key) {
                evicted_count += 1;
                evicted_bytes = evicted_bytes.saturating_add(removed.estimated_bytes);
                current_bytes = current_bytes.saturating_sub(removed.estimated_bytes);
            }
        }

        let after_count = self.resident_chunks.len();
        EvictionStats {
            candidate_count,
            evicted_count,
            evicted_recent_count: 0,
            before_count,
            after_count,
            before_bytes,
            after_bytes: current_bytes,
            evicted_bytes,
        }
    }

    fn touch_target_resident_chunks(&mut self) {
        let keys = self.current_target_keys.iter().copied().collect::<Vec<_>>();
        for key in keys {
            if let Some(resident) = self.resident_chunks.get_mut(&key) {
                resident.last_touched_frame = self.frame_index;
            }
            self.touch_warm_cache_key(key);
        }
        self.retain_warm_cache_keys();
    }

    fn prune_cache_pressure(&mut self, reason: &str) {
        if self.uses_full_scene_cache_runtime() {
            if self.frame_index < 5
                || self
                    .frame_index
                    .saturating_sub(self.last_pressure_skip_log_frame)
                    >= 120
            {
                self.last_pressure_skip_log_frame = self.frame_index;
                println!(
                    "[VIEWER_STREAM] pressure_relief_skipped reason={} runtime_mode=full_scene_cache resident_chunks={} cpu_cache_chunks={}",
                    reason,
                    self.resident_chunks.len(),
                    self.cpu_mesh_cache.len()
                );
            }
            return;
        }
        let cpu_eviction = self.prune_cpu_mesh_cache();
        let retain_keys = self.current_retain_keys.clone();
        let resident_eviction = self.prune_resident_chunks(&retain_keys);
        if cpu_eviction.evicted_count == 0 && resident_eviction.evicted_count == 0 {
            return;
        }
        println!(
            "[VIEWER_CACHE] pressure_relief reason={} cpu_lru_candidates={} cpu_evicted={} cpu_cache_evicted_recent={} cpu_cache_before={} cpu_cache_after={} cpu_cache_bytes_before={} cpu_cache_bytes_after={} cpu_bytes_evicted={} resident_lru_candidates={} resident_evicted={} resident_before={} resident_after={} resident_bytes_before={} resident_bytes_after={} resident_bytes_evicted={} cpu_max_chunks={} cpu_max_bytes={} resident_max_chunks={} resident_max_bytes={}",
            reason,
            cpu_eviction.candidate_count,
            cpu_eviction.evicted_count,
            cpu_eviction.evicted_recent_count,
            cpu_eviction.before_count,
            cpu_eviction.after_count,
            cpu_eviction.before_bytes,
            cpu_eviction.after_bytes,
            cpu_eviction.evicted_bytes,
            resident_eviction.candidate_count,
            resident_eviction.evicted_count,
            resident_eviction.before_count,
            resident_eviction.after_count,
            resident_eviction.before_bytes,
            resident_eviction.after_bytes,
            resident_eviction.evicted_bytes,
            self.cpu_mesh_cache.max_chunks,
            self.cpu_mesh_cache.max_bytes,
            self.max_resident_chunks,
            self.max_resident_bytes
        );
        if cpu_eviction.evicted_recent_count > 0 {
            println!(
                "[VIEWER_CACHE] cpu_cache_evicted_recent reason={} evicted_recent={}",
                reason, cpu_eviction.evicted_recent_count
            );
        }
        println!(
            "[VIEWER_STREAM] pressure_relief reason={} cpu_cache_chunks={} resident_chunks={} pending_builds={} pending_uploads={}",
            reason,
            self.cpu_mesh_cache.len(),
            self.resident_chunks.len(),
            self.pending_build_queue.len(),
            self.pending_upload_queue.len()
        );
    }

    fn sync_chunk_residency(&mut self) -> Result<()> {
        let Some(scene_index) = self.chunk_scene_index.as_ref().cloned() else {
            self.residency_dirty = false;
            return Ok(());
        };

        let plan = ChunkResidencyPlan::build(
            &scene_index,
            &self.camera,
            &self.streaming_config,
            self.frame_index,
            self.background_fill_enabled,
            self.prebuild_before_show,
        );
        let indexed_chunks = scene_index.chunk_count();
        self.current_target_keys = plan.target_keys.clone();
        self.current_preload_keys = plan.preload_keys.clone();
        self.current_retain_keys = plan.retain_keys.clone();
        self.refresh_sticky_target_keys(&plan);

        for key in &self.current_target_keys {
            if let Some(resident) = self.resident_chunks.get_mut(key) {
                resident.last_touched_frame = self.frame_index;
            }
        }

        let prep_stats = self.prepare_target_chunk_meshes(&scene_index, &plan);

        let retain_keys = self.current_retain_keys.clone();
        let resident_eviction = self.prune_resident_chunks(&retain_keys);
        let cpu_eviction = self.prune_cpu_mesh_cache();

        let (draw_vertices, draw_bounds_min, draw_bounds_max, draw_radius_sum, draw_center_avg) =
            self.current_draw_stats();

        let should_log = prep_stats.target_cache_hits > 0
            || prep_stats.target_cache_misses > 0
            || prep_stats.target_pending_hits > 0
            || prep_stats.target_queued_builds > 0
            || prep_stats.target_queued_uploads > 0
            || prep_stats.preload_cache_hits > 0
            || prep_stats.preload_cache_misses > 0
            || prep_stats.preload_pending_hits > 0
            || prep_stats.preload_queued_builds > 0
            || prep_stats.preload_queued_uploads > 0
            || resident_eviction.evicted_count > 0
            || cpu_eviction.evicted_count > 0
            || self.last_focus_chunk != Some(plan.focus_chunk)
            || self.last_target_radius_chunks != Some(plan.target_radius_chunks)
            || self.frame_index == 0;
        if should_log {
            let total_cache_hits = prep_stats.target_cache_hits + prep_stats.preload_cache_hits;
            let total_cache_misses =
                prep_stats.target_cache_misses + prep_stats.preload_cache_misses;
            let cpu_cache_hit_rate = if total_cache_hits + total_cache_misses == 0 {
                1.0
            } else {
                total_cache_hits as f32 / (total_cache_hits + total_cache_misses) as f32
            };
            let total_reenter_chunks =
                prep_stats.target_reenter_chunks + prep_stats.preload_reenter_chunks;
            let total_cache_hit_reuploads =
                prep_stats.target_cache_hit_reuploads + prep_stats.preload_cache_hit_reuploads;
            let total_cache_miss_rebuilds =
                prep_stats.target_cache_miss_rebuilds + prep_stats.preload_cache_miss_rebuilds;
            println!(
                "[VIEWER_CACHE] focus_chunk=({}, {}, {}) cache_hits={} cache_misses={} pending_build_hits={} queued_builds={} queued_uploads={} reenter_chunks={} cache_hit_reupload={} cache_miss_rebuild={} cpu_cache_hit_rate={:.3} cpu_lru_candidates={} cpu_evicted={} cpu_cache_evicted_recent={} cpu_cache_before={} cpu_cache_after={} cpu_cache_bytes_before={} cpu_cache_bytes_after={} cpu_bytes_evicted={} resident_lru_candidates={} resident_evicted={} resident_before={} resident_after={} resident_bytes_before={} resident_bytes_after={} resident_bytes_evicted={} cpu_max_chunks={} cpu_max_bytes={} resident_max_chunks={} resident_max_bytes={}",
                plan.focus_chunk.cx,
                plan.focus_chunk.cy,
                plan.focus_chunk.cz,
                total_cache_hits,
                total_cache_misses,
                prep_stats.target_pending_hits + prep_stats.preload_pending_hits,
                prep_stats.target_queued_builds + prep_stats.preload_queued_builds,
                prep_stats.target_queued_uploads + prep_stats.preload_queued_uploads,
                total_reenter_chunks,
                total_cache_hit_reuploads,
                total_cache_miss_rebuilds,
                cpu_cache_hit_rate,
                cpu_eviction.candidate_count,
                cpu_eviction.evicted_count,
                cpu_eviction.evicted_recent_count,
                cpu_eviction.before_count,
                cpu_eviction.after_count,
                cpu_eviction.before_bytes,
                cpu_eviction.after_bytes,
                cpu_eviction.evicted_bytes,
                resident_eviction.candidate_count,
                resident_eviction.evicted_count,
                resident_eviction.before_count,
                resident_eviction.after_count,
                resident_eviction.before_bytes,
                resident_eviction.after_bytes,
                resident_eviction.evicted_bytes,
                self.cpu_mesh_cache.max_chunks,
                self.cpu_mesh_cache.max_bytes,
                self.max_resident_chunks,
                self.max_resident_bytes
            );
            println!(
                "[VIEWER_BUILD] focus_chunk=({}, {}, {}) target_build_miss_chunks={} target_queued_builds={} target_pending_build_hits={} target_reenter_chunks={} target_cache_hit_reupload={} target_cache_miss_rebuild={} preload_build_miss_chunks={} preload_queued_builds={} preload_pending_build_hits={} preload_reenter_chunks={} preload_cache_hit_reupload={} preload_cache_miss_rebuild={} pending_builds={} in_flight_builds={} cpu_cache_chunks={} pending_uploads={}",
                plan.focus_chunk.cx,
                plan.focus_chunk.cy,
                plan.focus_chunk.cz,
                prep_stats.target_cache_misses,
                prep_stats.target_queued_builds,
                prep_stats.target_pending_hits,
                prep_stats.target_reenter_chunks,
                prep_stats.target_cache_hit_reuploads,
                prep_stats.target_cache_miss_rebuilds,
                prep_stats.preload_cache_misses,
                prep_stats.preload_queued_builds,
                prep_stats.preload_pending_hits,
                prep_stats.preload_reenter_chunks,
                prep_stats.preload_cache_hit_reuploads,
                prep_stats.preload_cache_miss_rebuilds,
                self.pending_build_queue.len(),
                self.in_flight_build_keys.len(),
                self.cpu_mesh_cache.len(),
                self.pending_upload_queue.len()
            );
            println!(
                "[VIEWER_STREAM] indexed_chunks={} focus_chunk=({}, {}, {}) target_radius_chunks={} retain_radius_chunks={} preload_radius_chunks={} coarse_target_chunks={} target_chunks={} frustum_target_chunks={} background_visible_chunks={} retain_chunks={} preload_chunks={} reenter_chunks={} queued_builds={} pending_builds={} queued_uploads={} pending_uploads={} resident_lru_candidates={} resident_evicted={} draw_chunks={} draw_vertices={} cpu_cache_chunks={} resident_chunks={} draw_bounds_min=({:.1}, {:.1}, {:.1}) draw_bounds_max=({:.1}, {:.1}, {:.1}) draw_center_avg=({:.1}, {:.1}, {:.1}) draw_radius_sum={:.1}",
                indexed_chunks,
                plan.focus_chunk.cx,
                plan.focus_chunk.cy,
                plan.focus_chunk.cz,
                plan.target_radius_chunks,
                plan.retain_radius_chunks,
                plan.preload_radius_chunks,
                plan.coarse_target_chunks,
                plan.target_chunks,
                plan.visible_target_chunks,
                plan.background_visible_chunks,
                plan.retain_chunks,
                plan.preload_chunks,
                total_reenter_chunks,
                prep_stats.target_queued_builds + prep_stats.preload_queued_builds,
                self.pending_build_queue.len(),
                prep_stats.target_queued_uploads + prep_stats.preload_queued_uploads,
                self.pending_upload_queue.len(),
                resident_eviction.candidate_count,
                resident_eviction.evicted_count,
                self.resident_chunks.len(),
                draw_vertices,
                self.cpu_mesh_cache.len(),
                self.resident_chunks.len(),
                draw_bounds_min.x,
                draw_bounds_min.y,
                draw_bounds_min.z,
                draw_bounds_max.x,
                draw_bounds_max.y,
                draw_bounds_max.z,
                draw_center_avg.x,
                draw_center_avg.y,
                draw_center_avg.z,
                draw_radius_sum
            );
            println!(
                "[VIEWER_PRELOAD] focus_chunk=({}, {}, {}) preload_chunks={} preload_forward_selected={} preload_radius_chunks={} preload_build_budget_chunks={} preload_build_budget_vertices={} preload_build_budget_indices={} preload_upload_budget_chunks={} preload_upload_budget_vertices={} preload_upload_budget_indices={} queued_builds={} queued_uploads={} pending_builds={} in_flight_builds={} pending_uploads={}",
                plan.focus_chunk.cx,
                plan.focus_chunk.cy,
                plan.focus_chunk.cz,
                plan.preload_chunks,
                plan.preload_forward_selected,
                plan.preload_radius_chunks,
                self.streaming_config
                    .preload_build_soft_budget
                    .max_chunks_per_frame,
                self.streaming_config
                    .preload_build_soft_budget
                    .max_vertices_per_frame,
                self.streaming_config
                    .preload_build_soft_budget
                    .max_indices_per_frame,
                self.streaming_config
                    .preload_upload_soft_budget
                    .max_chunks_per_frame,
                self.streaming_config
                    .preload_upload_soft_budget
                    .max_vertices_per_frame,
                self.streaming_config
                    .preload_upload_soft_budget
                    .max_indices_per_frame,
                prep_stats.preload_queued_builds,
                prep_stats.preload_queued_uploads,
                self.pending_build_queue.len(),
                self.in_flight_build_keys.len(),
                self.pending_upload_queue.len()
            );
            println!(
                "[VIEWER_CACHE] cpu_cache_hit_rate focus_chunk=({}, {}, {}) hit_rate={:.3} cache_hits={} cache_misses={}",
                plan.focus_chunk.cx,
                plan.focus_chunk.cy,
                plan.focus_chunk.cz,
                cpu_cache_hit_rate,
                total_cache_hits,
                total_cache_misses
            );
            if total_cache_hit_reuploads > 0 {
                println!(
                    "[VIEWER_CACHE] cache_hit_reupload focus_chunk=({}, {}, {}) reenter_chunks={} cache_hit_reupload={}",
                    plan.focus_chunk.cx,
                    plan.focus_chunk.cy,
                    plan.focus_chunk.cz,
                    total_reenter_chunks,
                    total_cache_hit_reuploads
                );
            }
            if total_cache_miss_rebuilds > 0 {
                println!(
                    "[VIEWER_CACHE] cache_miss_rebuild focus_chunk=({}, {}, {}) reenter_chunks={} cache_miss_rebuild={}",
                    plan.focus_chunk.cx,
                    plan.focus_chunk.cy,
                    plan.focus_chunk.cz,
                    total_reenter_chunks,
                    total_cache_miss_rebuilds
                );
            }
            if total_reenter_chunks > 0 {
                println!(
                    "[VIEWER_STREAM] reenter_chunks focus_chunk=({}, {}, {}) reenter_chunks={} cache_hit_reupload={} cache_miss_rebuild={}",
                    plan.focus_chunk.cx,
                    plan.focus_chunk.cy,
                    plan.focus_chunk.cz,
                    total_reenter_chunks,
                    total_cache_hit_reuploads,
                    total_cache_miss_rebuilds
                );
            }
        }

        self.last_focus_chunk = Some(plan.focus_chunk);
        self.last_target_radius_chunks = Some(plan.target_radius_chunks);
        self.residency_dirty = false;
        Ok(())
    }

    fn current_draw_stats(&self) -> (u64, Vec3, Vec3, f32, Vec3) {
        if self.resident_chunks.is_empty() {
            return (0, Vec3::ZERO, Vec3::ZERO, 0.0, Vec3::ZERO);
        }

        let mut draw_vertices = 0_u64;
        let mut bounds_min = Vec3::splat(f32::INFINITY);
        let mut bounds_max = Vec3::splat(f32::NEG_INFINITY);
        let mut radius_sum = 0.0_f32;
        let mut center_sum = Vec3::ZERO;
        for resident in self.resident_chunks.values() {
            draw_vertices += u64::from(resident.gpu.vertex_count);
            bounds_min = bounds_min.min(resident.bounds.min);
            bounds_max = bounds_max.max(resident.bounds.max);
            radius_sum += resident.bounds.radius;
            center_sum += resident.bounds.center;
        }

        (
            draw_vertices,
            bounds_min,
            bounds_max,
            radius_sum,
            center_sum / self.resident_chunks.len() as f32,
        )
    }

    fn mark_residency_dirty(&mut self) {
        self.residency_dirty = true;
    }

    fn uses_full_scene_cache_runtime(&self) -> bool {
        self.chunk_scene_index.is_none() && !self.prebuild_cache_only
    }

    fn projection_center_source(&self) -> &'static str {
        "orbit_target(scene_bounds_center)"
    }

    fn launch_mode_label(&self) -> &'static str {
        if self.embedded_viewport.is_some() {
            "embedded"
        } else {
            "popup"
        }
    }

    fn log_camera_viewport_diagnostics(&mut self, event: &str, force: bool) {
        if !force
            && self.frame_index >= 5
            && self.frame_index.saturating_sub(self.last_camera_diag_frame) < 120
        {
            return;
        }
        self.last_camera_diag_frame = self.frame_index;
        let eye = self.camera.eye();
        println!(
            "[VIEWER_CAMERA] launch_mode={} event={} viewport_w={} viewport_h={} aspect={:.5} camera_target=({:.2},{:.2},{:.2}) camera_pos=({:.2},{:.2},{:.2}) orbit_center=({:.2},{:.2},{:.2}) projection_center_source={}",
            self.launch_mode_label(),
            event,
            self.config.width,
            self.config.height,
            self.camera.aspect,
            self.camera.target.x,
            self.camera.target.y,
            self.camera.target.z,
            eye.x,
            eye.y,
            eye.z,
            self.scene_bounds.center.x,
            self.scene_bounds.center.y,
            self.scene_bounds.center.z,
            self.projection_center_source(),
        );
    }

    fn log_runtime_visibility_diagnostics(&mut self) {
        if self.frame_index < 5
            || self
                .frame_index
                .saturating_sub(self.last_runtime_diag_frame)
                >= 120
        {
            self.last_runtime_diag_frame = self.frame_index;
            let total = self
                .chunk_scene_index
                .as_ref()
                .map(|scene_index| scene_index.chunk_count())
                .unwrap_or_else(|| self.bootstrap_visible_first.max(self.cpu_mesh_cache.len()));
            let resident_keys = self.resident_chunks.keys().copied().collect::<HashSet<_>>();
            let drawable_keys = self
                .resident_chunks
                .iter()
                .filter_map(|(key, chunk)| {
                    (chunk.gpu.solid_index_count > 0 || chunk.gpu.translucent_index_count > 0)
                        .then_some(*key)
                })
                .collect::<HashSet<_>>();
            let pending_upload_keys = self
                .pending_upload_queue
                .iter()
                .map(|pending| pending.key)
                .collect::<HashSet<_>>();
            let pending_build_keys = self
                .pending_build_queue
                .iter()
                .map(|pending| pending.key)
                .collect::<HashSet<_>>();
            let uploaded = self.resident_chunks.len();
            let resident = self.resident_chunks.len();
            let drawable = drawable_keys.len();
            let full_scene_cache_runtime = self.uses_full_scene_cache_runtime();
            let visible = if full_scene_cache_runtime {
                drawable
            } else {
                self.current_target_keys
                    .intersection(&drawable_keys)
                    .count()
            };
            let culled = if full_scene_cache_runtime {
                0
            } else {
                total.saturating_sub(self.current_target_keys.len())
            };
            let missing_total = total.saturating_sub(drawable);
            let missing_not_uploaded = if full_scene_cache_runtime {
                total.saturating_sub(uploaded)
            } else {
                self.current_target_keys.difference(&resident_keys).count()
            };
            let missing_pending_upload = if full_scene_cache_runtime {
                pending_upload_keys.len()
            } else {
                self.current_target_keys
                    .intersection(&pending_upload_keys)
                    .count()
            };
            let missing_pending_build = if full_scene_cache_runtime {
                pending_build_keys.len()
            } else {
                self.current_target_keys
                    .intersection(&pending_build_keys)
                    .count()
            };
            let missing_empty_mesh = self.prebuild_empty_mesh_keys.len();
            let missing_culled = culled.saturating_sub(missing_empty_mesh);
            let missing_outside_visible = if full_scene_cache_runtime {
                0
            } else {
                total
                    .saturating_sub(self.current_target_keys.len())
                    .saturating_sub(missing_empty_mesh)
            };
            println!(
                "[VIEWER_RUNTIME] frame={} total={} uploaded={} resident={} drawable={} visible={} culled={} missing_total={} missing_not_uploaded={} missing_pending_upload={} missing_pending_build={} missing_empty_mesh={} missing_culled={} missing_outside_visible_or_cull={} pending_uploads={} pending_builds={} cpu_cache_chunks={} runtime_mode={} renderer_path=native_wgpu",
                self.frame_index,
                total,
                uploaded,
                resident,
                drawable,
                visible,
                culled,
                missing_total,
                missing_not_uploaded,
                missing_pending_upload,
                missing_pending_build,
                missing_empty_mesh,
                missing_culled,
                missing_outside_visible,
                self.pending_upload_queue.len(),
                self.pending_build_queue.len(),
                self.cpu_mesh_cache.len(),
                if full_scene_cache_runtime {
                    "full_scene_cache"
                } else {
                    "streaming_scene_index"
                }
            );
        }
    }

    fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            self.size = new_size;
            return;
        }
        self.size = new_size;
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.camera.update_aspect(new_size.width, new_size.height);
        self.surface.configure(&self.device, &self.config);
        self.depth_texture = DepthTexture::create(&self.device, &self.config);
        self.write_camera();
        self.mark_residency_dirty();
        self.log_camera_viewport_diagnostics("resize", true);
    }

    fn write_camera(&mut self) {
        let camera_uniform =
            CameraUniform::from_matrix_and_eye(self.camera.view_proj_matrix(), self.camera.eye());
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));
    }

    fn process_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        let next = Vec2::new(position.x as f32, position.y as f32);
        if let Some(last) = self.last_cursor_position {
            let delta = next - last;
            match self.drag_mode {
                Some(DragMode::Rotate) => {
                    self.camera.orbit(delta);
                    self.write_camera();
                    self.mark_residency_dirty();
                }
                Some(DragMode::Pan) => {
                    self.camera.pan(delta);
                    self.write_camera();
                    self.mark_residency_dirty();
                }
                None => {}
            }
        }
        self.last_cursor_position = Some(next);
    }

    fn process_mouse_input(&mut self, state: ElementState, button: MouseButton) {
        if self.preview_mode {
            self.drag_mode = None;
            return;
        }
        match (state, button) {
            (ElementState::Pressed, MouseButton::Left) => self.drag_mode = Some(DragMode::Rotate),
            (ElementState::Pressed, MouseButton::Right)
            | (ElementState::Pressed, MouseButton::Middle) => self.drag_mode = Some(DragMode::Pan),
            (ElementState::Released, MouseButton::Left)
            | (ElementState::Released, MouseButton::Right)
            | (ElementState::Released, MouseButton::Middle) => self.drag_mode = None,
            _ => {}
        }
    }

    fn apply_stress_camera_sweep(&mut self) {
        if !self.stress_camera_sweep {
            return;
        }

        let phase = self.frame_index as f32 * 0.045;
        let horizontal_radius = (self.scene_bounds.radius * 0.65).max(96.0);
        let vertical_radius = (self.scene_bounds.radius * 0.12).max(16.0);
        let next_target = self.scene_bounds.center
            + Vec3::new(
                phase.cos() * horizontal_radius,
                (phase * 0.5).sin() * vertical_radius,
                (phase * 0.8).sin() * horizontal_radius,
            );
        let next_yaw = phase + 0.8;
        let next_pitch = 0.35 + (phase * 0.35).sin() * 0.25;
        let next_distance =
            OrbitCamera::fit_distance(self.scene_bounds).min(self.camera.max_distance);
        let changed = self.camera.target.distance(next_target) > 1.0
            || (self.camera.yaw - next_yaw).abs() > 0.01
            || (self.camera.pitch - next_pitch).abs() > 0.01
            || (self.camera.distance - next_distance).abs() > 1.0;
        if changed {
            self.camera.target = next_target;
            self.camera.yaw = next_yaw;
            self.camera.pitch = next_pitch.clamp(-1.2, 1.2);
            self.camera.distance = next_distance;
            self.write_camera();
            self.mark_residency_dirty();
        }
    }

    fn apply_preview_camera_motion(&mut self) {
        if !self.preview_mode {
            return;
        }
        let next_yaw = if self.preview_spin {
            self.camera.yaw + 0.0015
        } else {
            FRAC_PI_4
        };
        let next_target = self.scene_bounds.center;
        let next_pitch = 0.45;
        let next_distance =
            OrbitCamera::fit_distance(self.scene_bounds).min(self.camera.max_distance);
        self.camera.target = next_target;
        self.camera.yaw = next_yaw;
        self.camera.pitch = next_pitch;
        self.camera.distance = next_distance;
        self.write_camera();
        self.mark_residency_dirty();
    }

    fn process_scroll(&mut self, delta: MouseScrollDelta) {
        if self.preview_mode {
            return;
        }
        let amount = match delta {
            MouseScrollDelta::LineDelta(_, y) => y,
            MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 48.0,
        };
        self.camera.zoom(amount);
        self.write_camera();
        self.mark_residency_dirty();
    }

    fn reset_camera(&mut self) {
        self.camera.reset(
            self.scene_bounds,
            self.config.width.max(1),
            self.config.height.max(1),
        );
        self.write_camera();
        self.mark_residency_dirty();
    }

    #[cfg(target_os = "windows")]
    fn sync_embedded_viewport_if_needed(&mut self, window: &winit::window::Window) {
        let Some(embedded) = self.embedded_viewport.as_mut() else {
            return;
        };
        match sync_embedded_window_to_parent(window, embedded.parent_hwnd) {
            Ok(client_size) => {
                if embedded.client_size != client_size {
                    embedded.client_size = client_size;
                    self.resize(client_size);
                }
            }
            Err(error) => {
                println!(
                    "[VIEWER_CAMERA] launch_mode=embedded event=sync_failed error={}",
                    error
                );
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn sync_embedded_viewport_if_needed(&mut self, _window: &winit::window::Window) {}

    fn render(
        &mut self,
        window: &winit::window::Window,
        viewer_started_at: Instant,
    ) -> Result<(), wgpu::SurfaceError> {
        self.sync_embedded_viewport_if_needed(window);
        if self.config.width == 0 || self.config.height == 0 {
            return Ok(());
        }

        self.frame_index = self.frame_index.saturating_add(1);
        if !self.prebuild_before_show
            && !self.background_fill_enabled
            && self.frame_index >= self.streaming_config.background_fill_start_frame
        {
            self.background_fill_enabled = true;
            self.mark_residency_dirty();
            println!(
                "[VIEWER_BOOTSTRAP] bootstrap_phase_done frame={} bootstrap_chunks={} visible_first={} resident_chunks={}",
                self.frame_index,
                self.resident_chunks.len(),
                self.bootstrap_visible_first,
                self.resident_chunks.len()
            );
        }
        if self.preview_mode {
            self.apply_preview_camera_motion();
        } else {
            self.apply_stress_camera_sweep();
        }
        if self.residency_dirty
            && let Err(error) = self.sync_chunk_residency()
        {
            println!("[VIEWER_STREAM] sync_failed error={error}");
        }
        let async_stats = self.consume_completed_build_results();
        let writer_completed = self.drain_prebuild_cache_writer_results();
        if writer_completed > 0 {
            println!(
                "[TRACE_WALL] stage=cache_write event=drain completed={} total_completed={} submitted={}",
                writer_completed,
                self.prebuild_writer_completed_chunks,
                self.prebuild_writer_submitted_chunks
            );
        }
        if async_stats.completed_builds > 0
            || async_stats.dropped_build_results > 0
            || async_stats.retained_build_results > 0
            || async_stats.reused_completed_results > 0
        {
            println!(
                "[VIEWER_ASYNC] worker_count={} completed_builds={} dropped_build_results={} retained_build_results={} reused_completed_results={} build_result_to_cache={} build_result_to_upload={} in_flight_builds={} max_in_flight_builds={}",
                self.build_worker_count,
                async_stats.completed_builds,
                async_stats.dropped_build_results,
                async_stats.retained_build_results,
                async_stats.reused_completed_results,
                async_stats.build_result_to_cache,
                async_stats.build_result_to_upload,
                self.in_flight_build_keys.len(),
                self.max_in_flight_builds
            );
        }
        if let Some(scene_index) = self.chunk_scene_index.as_ref().cloned() {
            match self.process_pending_builds(&scene_index) {
                Ok(build_stats) => {
                    let build_budget_split = self.build_budget_split();
                    let total_build_chunks =
                        build_stats.target_usage.chunks + build_stats.preload_usage.chunks;
                    let total_deferred_chunks =
                        build_stats.deferred_target_chunks + build_stats.deferred_preload_chunks;
                    let total_submitted_builds =
                        build_stats.target_submitted_builds + build_stats.preload_submitted_builds;
                    let total_submitted_uploads = build_stats.target_submitted_uploads
                        + build_stats.preload_submitted_uploads;
                    if total_build_chunks > 0
                        || total_submitted_builds > 0
                        || total_submitted_uploads > 0
                        || total_deferred_chunks > 0
                    {
                        let oldest_pending_age_frames = self
                            .pending_build_queue
                            .front()
                            .map(|pending| self.frame_index.saturating_sub(pending.enqueued_frame))
                            .unwrap_or(0);
                        println!(
                            "[VIEWER_BUILD] frame_builds target_submitted_builds={} target_build_budget_chunks={} target_build_budget_vertices={} target_build_budget_indices={} preload_submitted_builds={} preload_build_budget_chunks={} preload_build_budget_vertices={} preload_build_budget_indices={} target_submitted_uploads={} preload_submitted_uploads={} target_deferred_chunks={} preload_deferred_chunks={} pending_builds={} in_flight_builds={} oldest_pending_age_frames={} cpu_cache_chunks={} pending_uploads={}",
                            build_stats.target_submitted_builds,
                            build_budget_split.target.max_chunks_per_frame,
                            build_budget_split.target.max_vertices_per_frame,
                            build_budget_split.target.max_indices_per_frame,
                            build_stats.preload_submitted_builds,
                            build_budget_split.preload.max_chunks_per_frame,
                            build_budget_split.preload.max_vertices_per_frame,
                            build_budget_split.preload.max_indices_per_frame,
                            build_stats.target_submitted_uploads,
                            build_stats.preload_submitted_uploads,
                            build_stats.deferred_target_chunks,
                            build_stats.deferred_preload_chunks,
                            self.pending_build_queue.len(),
                            self.in_flight_build_keys.len(),
                            oldest_pending_age_frames,
                            self.cpu_mesh_cache.len(),
                            self.pending_upload_queue.len()
                        );
                        println!(
                            "[VIEWER_PRELOAD] frame_builds preload_budget_chunks={} preload_budget_vertices={} preload_budget_indices={} preload_used_chunks={} preload_used_vertices={} preload_used_indices={} preload_deferred_chunks={} target_used_chunks={} target_used_vertices={} target_used_indices={}",
                            self.streaming_config
                                .preload_build_soft_budget
                                .max_chunks_per_frame,
                            self.streaming_config
                                .preload_build_soft_budget
                                .max_vertices_per_frame,
                            self.streaming_config
                                .preload_build_soft_budget
                                .max_indices_per_frame,
                            build_stats.preload_usage.chunks,
                            build_stats.preload_usage.vertices,
                            build_stats.preload_usage.indices,
                            build_stats.deferred_preload_chunks,
                            build_stats.target_usage.chunks,
                            build_stats.target_usage.vertices,
                            build_stats.target_usage.indices
                        );
                        println!(
                            "[VIEWER_ASYNC] worker_count={} submitted_builds={} in_flight_builds={} max_in_flight_builds={} pending_builds={} submitted_uploads={}",
                            self.build_worker_count,
                            total_submitted_builds,
                            self.in_flight_build_keys.len(),
                            self.max_in_flight_builds,
                            self.pending_build_queue.len(),
                            total_submitted_uploads
                        );
                    }
                }
                Err(error) => {
                    println!("[VIEWER_BUILD] build_failed error={error}");
                }
            }
        }
        self.prune_cache_pressure("post_build");
        let upload_stats = self.process_pending_uploads();
        let total_upload_chunks =
            upload_stats.target_usage.chunks + upload_stats.preload_usage.chunks;
        let total_deferred_uploads =
            upload_stats.deferred_target_chunks + upload_stats.deferred_preload_chunks;
        if total_upload_chunks > 0 || total_deferred_uploads > 0 {
            let oldest_pending_age_frames = self
                .pending_upload_queue
                .front()
                .map(|pending| self.frame_index.saturating_sub(pending.enqueued_frame))
                .unwrap_or(0);
            println!(
                "[VIEWER_STREAM] frame_uploads target_upload_chunks={} target_upload_vertices={} target_upload_indices={} preload_upload_chunks={} preload_upload_vertices={} preload_upload_indices={} preload_budget_chunks={} preload_budget_vertices={} preload_budget_indices={} target_deferred_chunks={} preload_deferred_chunks={} pending_uploads={} oldest_pending_age_frames={} cpu_cache_chunks={} resident_chunks={}",
                upload_stats.target_usage.chunks,
                upload_stats.target_usage.vertices,
                upload_stats.target_usage.indices,
                upload_stats.preload_usage.chunks,
                upload_stats.preload_usage.vertices,
                upload_stats.preload_usage.indices,
                self.streaming_config
                    .preload_upload_soft_budget
                    .max_chunks_per_frame,
                self.streaming_config
                    .preload_upload_soft_budget
                    .max_vertices_per_frame,
                self.streaming_config
                    .preload_upload_soft_budget
                    .max_indices_per_frame,
                upload_stats.deferred_target_chunks,
                upload_stats.deferred_preload_chunks,
                self.pending_upload_queue.len(),
                oldest_pending_age_frames,
                self.cpu_mesh_cache.len(),
                self.resident_chunks.len()
            );
            println!(
                "[VIEWER_PRELOAD] frame_uploads preload_budget_chunks={} preload_budget_vertices={} preload_budget_indices={} preload_used_chunks={} preload_used_vertices={} preload_used_indices={} preload_deferred_chunks={} target_used_chunks={} target_used_vertices={} target_used_indices={}",
                self.streaming_config
                    .preload_upload_soft_budget
                    .max_chunks_per_frame,
                self.streaming_config
                    .preload_upload_soft_budget
                    .max_vertices_per_frame,
                self.streaming_config
                    .preload_upload_soft_budget
                    .max_indices_per_frame,
                upload_stats.preload_usage.chunks,
                upload_stats.preload_usage.vertices,
                upload_stats.preload_usage.indices,
                upload_stats.deferred_preload_chunks,
                upload_stats.target_usage.chunks,
                upload_stats.target_usage.vertices,
                upload_stats.target_usage.indices
            );
        }
        self.prune_cache_pressure("post_upload");

        self.touch_target_resident_chunks();
        if self.prebuild_is_ready() {
            self.finalize_prebuild(window, viewer_started_at);
        }
        if !self.prebuild_before_show
            && self.background_fill_enabled
            && self
                .frame_index
                .saturating_sub(self.last_background_fill_log_frame)
                >= self.streaming_config.background_fill_log_interval_frames
        {
            self.last_background_fill_log_frame = self.frame_index;
            println!(
                "[VIEWER_STREAM] background_fill_progress frame={} target_chunks={} retain_chunks={} resident_chunks={} cpu_cache_chunks={} pending_builds={} in_flight_builds={} pending_uploads={}",
                self.frame_index,
                self.current_target_keys.len(),
                self.current_retain_keys.len(),
                self.resident_chunks.len(),
                self.cpu_mesh_cache.len(),
                self.pending_build_queue.len(),
                self.in_flight_build_keys.len(),
                self.pending_upload_queue.len()
            );
        }
        self.log_runtime_visibility_diagnostics();

        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("native_viewer_encoder"),
            });

        let shadow_plan = ShadowDepthPassPlan::from_config(self.shadow_resources.config);
        let mut shadow_draw_calls = 0_u64;
        let mut shadow_index_count = 0_u64;
        if shadow_plan.enabled {
            let mut shadow_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("native_viewer_basic_shadow_depth_pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_resources.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            shadow_pass.set_pipeline(&self.shadow_resources.depth_pipeline);
            shadow_pass.set_bind_group(0, &self.shadow_resources.camera_bind_group, &[]);
            for resident in self.resident_chunks.values() {
                shadow_pass.set_vertex_buffer(0, resident.gpu.vertex_buffer.slice(..));
                shadow_pass.set_index_buffer(
                    resident.gpu.solid_index_buffer.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                shadow_pass.draw_indexed(0..resident.gpu.solid_index_count, 0, 0..1);
                shadow_draw_calls += 1;
                shadow_index_count += u64::from(resident.gpu.solid_index_count);
            }
        }
        let should_log_shadow_debug = self.shadow_debug
            && (!self.shadow_debug_logged || self.frame_index.is_multiple_of(120));
        let shadow_debug_readback = if should_log_shadow_debug && shadow_plan.enabled {
            let resolution = self.shadow_resources.config.resolution.max(1);
            let bytes_per_row = resolution * std::mem::size_of::<f32>() as u32;
            let byte_len = bytes_per_row as u64 * u64::from(resolution);
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("native_viewer_basic_shadow_debug_readback"),
                size: byte_len,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::ImageCopyTexture {
                    texture: &self.shadow_resources.depth_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::DepthOnly,
                },
                wgpu::ImageCopyBuffer {
                    buffer: &buffer,
                    layout: wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(bytes_per_row),
                        rows_per_image: Some(resolution),
                    },
                },
                wgpu::Extent3d {
                    width: resolution,
                    height: resolution,
                    depth_or_array_layers: 1,
                },
            );
            Some((buffer, byte_len as usize))
        } else {
            None
        };
        if should_log_shadow_debug {
            self.shadow_debug_logged = true;
            println!(
                "[LBA_SHADOW_DEBUG] stage=depth_pass frame={} enabled={} executed={} draw_calls={} index_count={} instance_count={} resident_chunks={} shadow_map_resolution={} effective_pcf_samples={} pcf_radius={:.3} bias={:.5} strength={:.3} shader_shadow_enabled={} shadow_debug_view={} shadow_force_test={}",
                self.frame_index,
                self.shadow_resources.config.enabled,
                shadow_plan.enabled,
                shadow_draw_calls,
                shadow_index_count,
                shadow_draw_calls,
                self.resident_chunks.len(),
                self.shadow_resources.config.resolution,
                self.shadow_resources.config.pcf.label(),
                self.shadow_resources.config.pcf.radius,
                self.shadow_resources.config.bias,
                self.shadow_resources.config.strength,
                if self.shadow_resources.config.enabled {
                    1.0
                } else {
                    0.0
                },
                self.shadow_resources.config.debug_view.label(),
                self.shadow_resources.config.force_test,
            );
        }

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("native_viewer_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.08,
                            g: 0.09,
                            b: 0.11,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_texture.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            render_pass.set_bind_group(1, &self.atlas_bind_group, &[]);
            render_pass.set_bind_group(2, &self.lighting_bind_group, &[]);
            render_pass.set_bind_group(3, &self.shadow_resources.bind_group, &[]);
            render_pass.set_pipeline(&self.render_pipeline_solid);
            for resident in self.resident_chunks.values() {
                render_pass.set_vertex_buffer(0, resident.gpu.vertex_buffer.slice(..));
                render_pass.set_index_buffer(
                    resident.gpu.solid_index_buffer.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                render_pass.draw_indexed(0..resident.gpu.solid_index_count, 0, 0..1);
            }
            render_pass.set_pipeline(&self.render_pipeline_translucent);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            render_pass.set_bind_group(2, &self.lighting_bind_group, &[]);
            render_pass.set_bind_group(3, &self.shadow_resources.bind_group, &[]);
            for resident in self.resident_chunks.values() {
                let Some(translucent_index_buffer) = resident.gpu.translucent_index_buffer.as_ref()
                else {
                    continue;
                };
                render_pass.set_vertex_buffer(0, resident.gpu.vertex_buffer.slice(..));
                render_pass.set_index_buffer(
                    translucent_index_buffer.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                render_pass.draw_indexed(0..resident.gpu.translucent_index_count, 0, 0..1);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        output.present();
        if let Some((buffer, byte_len)) = shadow_debug_readback {
            if let Some(depth_values) = read_shadow_depth_values(&self.device, &buffer, byte_len) {
                let shadow_depth_stats = collect_shadow_depth_stats(&depth_values);
                println!(
                    "[LBA_SHADOW_DEBUG] stage=shadow_depth_stats written_depth=[{}] clear_depth_pixels={} total_pixels={}",
                    shadow_depth_stats.format(),
                    depth_values
                        .iter()
                        .filter(|value| **value >= 0.999_999)
                        .count(),
                    depth_values.len(),
                );
                collect_shadow_compare_stats(
                    &self.resident_chunks,
                    &self.cpu_mesh_cache,
                    &depth_values,
                    self.shadow_resources.config,
                )
                .log(self.shadow_resources.config);
            } else {
                println!("[LBA_SHADOW_DEBUG] stage=shadow_depth_stats readback_failed=true");
            }
        }
        Ok(())
    }
}

impl Drop for ViewerState {
    fn drop(&mut self) {
        self.build_request_tx.take();
        for (worker_index, worker) in self.build_workers.drain(..).enumerate() {
            if worker.join().is_err() {
                println!(
                    "[VIEWER_ASYNC] worker_join_failed worker_id={} reason=panic_during_shutdown",
                    worker_index
                );
            }
        }
    }
}

fn parse_args() -> Result<Option<ViewerArgs>> {
    let mut input: Option<PathBuf> = None;
    let mut chunk_size = 32_u32;
    let mut probe_scene = false;
    let mut prebuild_only = false;
    let mut auto_exit_seconds = None;
    let mut prebuild_before_show = false;
    let mut cache_input = None;
    let mut ready_file = None;
    let mut cache_file = None;
    let mut preview_output = None;
    let mut build_workers = default_build_worker_count();
    let mut cpu_cache_budget = DEFAULT_CPU_CACHE_BUDGET;
    let mut resident_budget = DEFAULT_RESIDENT_BUDGET;
    let mut stress_limits = false;
    let mut embed_parent_hwnd = None;
    let mut preview_mode = false;
    let mut preview_spin = false;
    let mut basic_lighting = std::env::var("LBA_VIEWER_BASIC_LIGHTING")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on"
            )
        })
        .unwrap_or(false);
    let mut shadow_debug = env_bool("LBA_SHADOW_DEBUG") || shadow_smoke_check_enabled();
    let shadow_debug_view_env = std::env::var("LBA_SHADOW_DEBUG_VIEW")
        .ok()
        .map(|value| ShadowDebugViewMode::parse(&value));
    let mut shadow_debug_view = ShadowDebugViewMode::Off;
    let shadow_force_test = std::env::var("LBA_SHADOW_FORCE_TEST")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on"
            )
        })
        .unwrap_or(false);
    let mut basic_shadows = std::env::var("LBA_VIEWER_BASIC_SHADOWS")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on"
            )
        })
        .unwrap_or(false);
    let mut display_mode = std::env::var("LBA_VIEWER_DISPLAY_MODE")
        .ok()
        .map(|value| ViewerDisplayMode::parse(&value))
        .unwrap_or(ViewerDisplayMode::Standard);
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "usage: litematica_native_viewer [file] [--chunk-size=N] [--probe-scene] [--prebuild-only] [--auto-exit-seconds=N] [--prebuild-before-show] [--ready-file=PATH] [--cache-file=PATH] [--preview-output=PATH] [--build-workers=N] [--stress-limits] [--preview-mode] [--preview-spin] [--display-mode=normal|fast_experimental|full] [--basic-lighting] [--basic-shadows] [--shadow-debug] [--shadow-debug-view] [--cpu-cache-max-chunks=N] [--cpu-cache-max-bytes=SIZE] [--resident-max-chunks=N] [--resident-max-bytes=SIZE] [--embed-parent-hwnd=HWND]"
                );
                println!(
                    "controls: left-drag rotate, right-drag pan, mouse-wheel zoom, R reset, Esc exit"
                );
                return Ok(None);
            }
            "--probe-scene" => probe_scene = true,
            "--prebuild-only" => prebuild_only = true,
            "--prebuild-before-show" => prebuild_before_show = true,
            "--preview-mode" => preview_mode = true,
            "--preview-spin" => {
                preview_mode = true;
                preview_spin = true;
            }
            "--basic-lighting" => basic_lighting = true,
            "--basic-shadows" => basic_shadows = true,
            "--shadow-debug" => shadow_debug = true,
            "--shadow-debug-view" => {
                shadow_debug = true;
                shadow_debug_view = shadow_debug_view_env.unwrap_or(ShadowDebugViewMode::Factor);
            }
            "--stress-limits" => {
                stress_limits = true;
                cpu_cache_budget = STRESS_CPU_CACHE_BUDGET;
                resident_budget = STRESS_RESIDENT_BUDGET;
            }
            _ if arg.starts_with("--chunk-size=") => {
                chunk_size = arg.trim_start_matches("--chunk-size=").parse()?;
            }
            _ if arg.starts_with("--auto-exit-seconds=") => {
                auto_exit_seconds = Some(
                    arg.trim_start_matches("--auto-exit-seconds=")
                        .parse::<u64>()?
                        .max(1),
                );
            }
            _ if arg.starts_with("--ready-file=") => {
                ready_file = Some(PathBuf::from(arg.trim_start_matches("--ready-file=")));
            }
            _ if arg.starts_with("--cache-input=") => {
                cache_input = Some(PathBuf::from(arg.trim_start_matches("--cache-input=")));
            }
            _ if arg.starts_with("--cache-file=") => {
                cache_file = Some(PathBuf::from(arg.trim_start_matches("--cache-file=")));
            }
            _ if arg.starts_with("--display-mode=") => {
                display_mode = ViewerDisplayMode::parse(arg.trim_start_matches("--display-mode="));
            }
            _ if arg.starts_with("--preview-output=") => {
                preview_output = Some(PathBuf::from(arg.trim_start_matches("--preview-output=")));
            }
            _ if arg.starts_with("--build-workers=") => {
                build_workers = arg
                    .trim_start_matches("--build-workers=")
                    .parse::<usize>()?
                    .max(1);
            }
            _ if arg.starts_with("--cpu-cache-max-chunks=") => {
                cpu_cache_budget.max_chunks =
                    arg.trim_start_matches("--cpu-cache-max-chunks=").parse()?;
            }
            _ if arg.starts_with("--cpu-cache-max-bytes=") => {
                cpu_cache_budget.max_bytes =
                    parse_byte_size(arg.trim_start_matches("--cpu-cache-max-bytes="))?;
            }
            _ if arg.starts_with("--resident-max-chunks=") => {
                resident_budget.max_chunks =
                    arg.trim_start_matches("--resident-max-chunks=").parse()?;
            }
            _ if arg.starts_with("--resident-max-bytes=") => {
                resident_budget.max_bytes =
                    parse_byte_size(arg.trim_start_matches("--resident-max-bytes="))?;
            }
            _ if arg.starts_with("--embed-parent-hwnd=") => {
                embed_parent_hwnd = Some(
                    arg.trim_start_matches("--embed-parent-hwnd=")
                        .parse::<isize>()?,
                );
            }
            _ if arg.starts_with('-') => bail!("unknown argument: {arg}"),
            _ => {
                if input.is_some() {
                    bail!("multiple input files provided");
                }
                input = Some(PathBuf::from(arg));
            }
        }
    }
    if basic_shadows && !basic_lighting {
        println!(
            "[NATIVE_VIEWER] basic_shadows_requested=true basic_lighting=false shadow_effect=disabled"
        );
        basic_shadows = false;
    }
    Ok(Some(ViewerArgs {
        input,
        chunk_size,
        probe_scene,
        prebuild_only,
        auto_exit_seconds,
        prebuild_before_show,
        cache_input,
        ready_file,
        cache_file,
        preview_output,
        build_workers,
        cpu_cache_max_chunks: cpu_cache_budget.max_chunks,
        cpu_cache_max_bytes: cpu_cache_budget.max_bytes,
        resident_max_chunks: resident_budget.max_chunks,
        resident_max_bytes: resident_budget.max_bytes,
        stress_limits,
        embed_parent_hwnd,
        preview_mode,
        preview_spin,
        display_mode,
        basic_lighting,
        basic_shadows,
        shadow_debug,
        shadow_debug_view,
        shadow_force_test,
    }))
}

fn env_bool(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on"
            )
        })
        .unwrap_or(false)
}

fn resolve_palette_color(
    block_id: &str,
    properties: Option<&BTreeMap<String, String>>,
    display_mode: ViewerDisplayMode,
) -> ResolvedBlockColor {
    if display_mode == ViewerDisplayMode::Full {
        let stripped_state = block_id.split('[').next().unwrap_or(block_id);
        return ResolvedBlockColor {
            rgb: [1.0, 1.0, 1.0],
            normalized_key: stripped_state
                .split(':')
                .next_back()
                .unwrap_or(stripped_state)
                .to_string(),
            matched_key: Some("runtime_v2_texture_atlas".to_string()),
            used_default: false,
        };
    }
    let stripped_state = block_id.split('[').next().unwrap_or(block_id);
    let normalized_key = stripped_state
        .split(':')
        .next_back()
        .unwrap_or(stripped_state)
        .to_string();
    let state_key = properties.map(|props| block_state_color_key(stripped_state, props));
    let cache = block_color_cache();
    let mut candidates = Vec::<&str>::new();
    if let Some(state_key) = state_key.as_deref() {
        candidates.push(state_key);
    }
    candidates.push(stripped_state);
    candidates.push(normalized_key.as_str());
    let matched_key = candidates
        .into_iter()
        .find(|candidate| cache.contains_key(*candidate))
        .map(str::to_string);
    let used_default = matched_key.is_none();
    let rgb = matched_key
        .as_deref()
        .and_then(|candidate| cache.get(candidate))
        .copied()
        .unwrap_or(DEFAULT_BLOCK_COLOR);
    ResolvedBlockColor {
        rgb,
        normalized_key,
        matched_key,
        used_default,
    }
}

fn block_state_color_key(block_id: &str, properties: &BTreeMap<String, String>) -> String {
    if properties.is_empty() {
        return block_id.to_string();
    }
    let pairs = properties
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{block_id}[{pairs}]")
}

fn block_color_cache() -> &'static HashMap<String, [f32; 3]> {
    BLOCK_COLOR_CACHE.get_or_init(|| {
        let Some(root) = locate_workspace_root() else {
            println!(
                "[VIEWER_COLOR] color_source=missing_workspace_root preview_chain=desktop_ui_cache native_chain=default_gray"
            );
            return HashMap::new();
        };
        let cache_path = root
            .join("data")
            .join("blockColorCache.json");
        let payload = match std::fs::read_to_string(&cache_path) {
            Ok(payload) => payload,
            Err(error) => {
                println!(
                    "[VIEWER_COLOR] color_source=missing_cache path={} error={} native_chain=default_gray",
                    cache_path.display(),
                    error
                );
                return HashMap::new();
            }
        };
        let cache = serde_json::from_str::<HashMap<String, [f32; 3]>>(&payload).unwrap_or_else(|error| {
            println!(
                "[VIEWER_COLOR] color_source=invalid_cache path={} error={} native_chain=default_gray",
                cache_path.display(),
                error
            );
            HashMap::new()
        });
        println!(
            "[VIEWER_COLOR] color_source=data/blockColorCache.json cache_entries={} path={}",
            cache.len(),
            cache_path.display()
        );
        cache
    })
}

fn locate_workspace_root() -> Option<PathBuf> {
    fn is_workspace_root(path: &Path) -> bool {
        path.join("block").exists()
            && path.join("item").exists()
    }

    if let Ok(current_dir) = std::env::current_dir() {
        for ancestor in current_dir.ancestors() {
            if is_workspace_root(ancestor) {
                return Some(ancestor.to_path_buf());
            }
        }
    }

    if let Ok(current_exe) = std::env::current_exe() {
        for ancestor in current_exe.ancestors() {
            if is_workspace_root(ancestor) {
                return Some(ancestor.to_path_buf());
            }
        }
    }

    None
}

fn load_scene(args: &ViewerArgs) -> Result<ViewerScene> {
    if let Some(path) = args.cache_input.as_deref() {
        match ViewerScene::from_prebuild_cache(path, args.chunk_size) {
            Ok(scene) => return Ok(scene),
            Err(error) => {
                if args.display_mode == ViewerDisplayMode::Full {
                    anyhow::bail!(
                        "[VIEWER_CACHE] load_failed cache_input={} error={} fallback=disabled_for_full_mode_v2",
                        path.display(),
                        error
                    );
                }
                println!(
                    "[VIEWER_CACHE] load_failed cache_input={} error={} fallback=placeholder",
                    path.display(),
                    error
                );
                return Ok(ViewerScene::placeholder());
            }
        }
    }
    match args.input.as_deref() {
        Some(path) => match ViewerScene::from_litematic(path, args.chunk_size, args.display_mode) {
            Ok(scene) => Ok(scene),
            Err(error) => {
                if args.display_mode == ViewerDisplayMode::Full {
                    anyhow::bail!(
                        "[VIEWER_SCENE] load_failed file={} error={} fallback=disabled_for_full_mode_v2",
                        path.display(),
                        error
                    );
                }
                println!(
                    "[VIEWER_SCENE] load_failed file={} error={} fallback=placeholder",
                    path.display(),
                    error
                );
                Ok(ViewerScene::placeholder())
            }
        },
        None => {
            println!("[VIEWER_SCENE] no_input_provided using_placeholder=true");
            Ok(ViewerScene::placeholder())
        }
    }
}

fn render_offscreen_preview(
    scene: &ViewerScene,
    output: &Path,
    lighting: LightingConfig,
) -> Result<()> {
    let basic_lighting = lighting.basic();
    let debug_labels = std::env::var_os("LBA_FULL_MODE_V2_PISTON_DEBUG").is_some()
        || std::env::var_os("LBA_FULL_MODE_V2_CHEST_DEBUG").is_some()
        || std::env::var_os("LBA_FULL_MODE_V2_OBSERVER_DEBUG").is_some();
    let (width, height) = if debug_labels {
        (1600_u32, 1000_u32)
    } else {
        (320_u32, 200_u32)
    };
    let mut camera = OrbitCamera::from_bounds(scene.bounds, width, height);
    camera.yaw = FRAC_PI_4;
    camera.pitch = 0.45;
    camera.distance = (OrbitCamera::fit_distance(scene.bounds) * 1.08).min(camera.max_distance);
    let view_proj = camera.view_proj_matrix();
    let mut image = RgbImage::from_pixel(width, height, Rgb([22, 24, 29]));
    let mut opaque_depth = vec![f32::INFINITY; (width * height) as usize];
    let mut translucent_triangles = Vec::<PreviewTriangle>::new();
    let mut chest_debug_stats = (std::env::var_os("LBA_FULL_MODE_V2_CHEST_DEBUG").is_some())
        .then(ChestDebugPreviewStats::default);
    let mut observer_debug_stats = (std::env::var_os("LBA_FULL_MODE_V2_OBSERVER_DEBUG").is_some())
        .then(ObserverDebugPreviewStats::default);
    for mesh in &scene.bootstrap_meshes {
        for tri in mesh.indices.chunks_exact(3) {
            if let Some(triangle) =
                project_preview_triangle(mesh.vertices.as_slice(), tri, view_proj, width, height)
            {
                raster_preview_triangle(
                    &mut image,
                    &mut opaque_depth,
                    width,
                    height,
                    &triangle,
                    scene.texture_atlas.as_ref(),
                    basic_lighting,
                    PreviewRasterMode::Opaque,
                    chest_debug_stats.as_mut(),
                    observer_debug_stats.as_mut(),
                );
            }
        }
        for tri in mesh.translucent_indices.chunks_exact(3) {
            if let Some(triangle) =
                project_preview_triangle(mesh.vertices.as_slice(), tri, view_proj, width, height)
            {
                translucent_triangles.push(triangle);
            }
        }
    }
    translucent_triangles.sort_by(|left, right| {
        right
            .average_depth()
            .partial_cmp(&left.average_depth())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for triangle in &translucent_triangles {
        raster_preview_triangle(
            &mut image,
            &mut opaque_depth,
            width,
            height,
            triangle,
            scene.texture_atlas.as_ref(),
            basic_lighting,
            PreviewRasterMode::Translucent,
            chest_debug_stats.as_mut(),
            observer_debug_stats.as_mut(),
        );
    }
    if let Some(stats) = chest_debug_stats.as_ref() {
        stats.log_summary();
    }
    if let Some(stats) = observer_debug_stats.as_ref() {
        stats.log_summary();
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create preview output dir failed: {}", parent.display()))?;
    }
    image
        .save(output)
        .with_context(|| format!("write preview image failed: {}", output.display()))?;
    println!(
        "[PREVIEW_RENDER] offscreen_ready output={} width={} height={} chunks={} center=({:.2},{:.2},{:.2}) radius={:.2}",
        output.display(),
        width,
        height,
        scene.bootstrap_meshes.len(),
        scene.bounds.center.x,
        scene.bounds.center.y,
        scene.bounds.center.z,
        scene.bounds.radius,
    );
    Ok(())
}

fn project_preview_vertex(
    view_proj: Mat4,
    position: [f32; 3],
    width: u32,
    height: u32,
) -> Option<[f32; 3]> {
    let clip = view_proj * Vec3::from_array(position).extend(1.0);
    if clip.w.abs() <= f32::EPSILON {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if ndc.z < -1.0 || ndc.z > 1.0 {
        return None;
    }
    Some([
        (ndc.x * 0.5 + 0.5) * (width as f32 - 1.0),
        (1.0 - (ndc.y * 0.5 + 0.5)) * (height as f32 - 1.0),
        ndc.z,
    ])
}

#[derive(Clone, Copy)]
struct PreviewProjectedVertex {
    position: [f32; 3],
    color: [f32; 4],
    uv: [f32; 2],
    use_texture: f32,
    normal: [f32; 3],
}

#[derive(Clone)]
struct PreviewTriangle {
    vertices: [PreviewProjectedVertex; 3],
    chest_debug: Option<ChestDebugTriangleInfo>,
    observer_debug: Option<ObserverDebugTriangleInfo>,
}

impl PreviewTriangle {
    fn average_depth(&self) -> f32 {
        (self.vertices[0].position[2] + self.vertices[1].position[2] + self.vertices[2].position[2])
            / 3.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PreviewRasterMode {
    Opaque,
    Translucent,
}

fn project_preview_triangle(
    vertices: &[GpuVertex],
    tri: &[u32],
    view_proj: Mat4,
    width: u32,
    height: u32,
) -> Option<PreviewTriangle> {
    let Some(v0) = vertices.get(*tri.first()? as usize) else {
        return None;
    };
    let Some(v1) = vertices.get(*tri.get(1)? as usize) else {
        return None;
    };
    let Some(v2) = vertices.get(*tri.get(2)? as usize) else {
        return None;
    };
    let chest_debug = classify_chest_debug_triangle([v0, v1, v2]);
    let observer_debug = classify_observer_debug_triangle([v0, v1, v2]);
    log_chest_front_triangle("preview_input", [v0, v1, v2], chest_debug.as_ref());
    let p0 = project_preview_vertex(view_proj, v0.position, width, height)?;
    let p1 = project_preview_vertex(view_proj, v1.position, width, height)?;
    let p2 = project_preview_vertex(view_proj, v2.position, width, height)?;
    Some(PreviewTriangle {
        vertices: [
            PreviewProjectedVertex {
                position: p0,
                color: v0.color,
                uv: v0.uv,
                use_texture: v0.use_texture,
                normal: v0.normal,
            },
            PreviewProjectedVertex {
                position: p1,
                color: v1.color,
                uv: v1.uv,
                use_texture: v1.use_texture,
                normal: v1.normal,
            },
            PreviewProjectedVertex {
                position: p2,
                color: v2.color,
                uv: v2.uv,
                use_texture: v2.use_texture,
                normal: v2.normal,
            },
        ],
        chest_debug,
        observer_debug,
    })
}

fn raster_preview_triangle(
    image: &mut RgbImage,
    depth: &mut [f32],
    width: u32,
    height: u32,
    triangle: &PreviewTriangle,
    atlas: Option<&RgbaImage>,
    lighting: BasicLightingConfig,
    mode: PreviewRasterMode,
    chest_debug_stats: Option<&mut ChestDebugPreviewStats>,
    observer_debug_stats: Option<&mut ObserverDebugPreviewStats>,
) {
    let p0 = triangle.vertices[0].position;
    let p1 = triangle.vertices[1].position;
    let p2 = triangle.vertices[2].position;
    let area = edge_preview(p0, p1, p2[0], p2[1]);
    if area.abs() <= f32::EPSILON {
        return;
    }
    let mut chest_debug_stats = chest_debug_stats;
    let mut observer_debug_stats = observer_debug_stats;
    if let (Some(stats), Some(debug)) = (
        chest_debug_stats.as_deref_mut(),
        triangle.chest_debug.as_ref(),
    ) {
        stats.note_triangle(debug, mode);
    }
    if let (Some(stats), Some(debug)) = (
        observer_debug_stats.as_deref_mut(),
        triangle.observer_debug.as_ref(),
    ) {
        stats.note_triangle(debug, mode);
    }
    let min_x = p0[0].min(p1[0]).min(p2[0]).floor().max(0.0) as u32;
    let max_x = p0[0].max(p1[0]).max(p2[0]).ceil().min(width as f32 - 1.0) as u32;
    let min_y = p0[1].min(p1[1]).min(p2[1]).floor().max(0.0) as u32;
    let max_y = p0[1].max(p1[1]).max(p2[1]).ceil().min(height as f32 - 1.0) as u32;
    let sign = area.signum();
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 = edge_preview(p1, p2, px, py) * sign;
            let w1 = edge_preview(p2, p0, px, py) * sign;
            let w2 = edge_preview(p0, p1, px, py) * sign;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            if let (Some(stats), Some(debug)) = (
                chest_debug_stats.as_deref_mut(),
                triangle.chest_debug.as_ref(),
            ) {
                stats.note_candidate(debug);
            }
            if let (Some(stats), Some(debug)) = (
                observer_debug_stats.as_deref_mut(),
                triangle.observer_debug.as_ref(),
            ) {
                stats.note_candidate(debug);
            }
            let inv_area = 1.0 / area.abs();
            let b0 = w0 * inv_area;
            let b1 = w1 * inv_area;
            let b2 = w2 * inv_area;
            let z = p0[2] * b0 + p1[2] * b1 + p2[2] * b2;
            let index = (y * width + x) as usize;
            if z >= depth[index] {
                continue;
            }
            let mut color = [
                triangle.vertices[0].color[0] * b0
                    + triangle.vertices[1].color[0] * b1
                    + triangle.vertices[2].color[0] * b2,
                triangle.vertices[0].color[1] * b0
                    + triangle.vertices[1].color[1] * b1
                    + triangle.vertices[2].color[1] * b2,
                triangle.vertices[0].color[2] * b0
                    + triangle.vertices[1].color[2] * b1
                    + triangle.vertices[2].color[2] * b2,
                triangle.vertices[0].color[3] * b0
                    + triangle.vertices[1].color[3] * b1
                    + triangle.vertices[2].color[3] * b2,
            ];
            let use_texture = triangle.vertices[0].use_texture * b0
                + triangle.vertices[1].use_texture * b1
                + triangle.vertices[2].use_texture * b2;
            let normal = normalize_lighting_direction([
                triangle.vertices[0].normal[0] * b0
                    + triangle.vertices[1].normal[0] * b1
                    + triangle.vertices[2].normal[0] * b2,
                triangle.vertices[0].normal[1] * b0
                    + triangle.vertices[1].normal[1] * b1
                    + triangle.vertices[2].normal[1] * b2,
                triangle.vertices[0].normal[2] * b0
                    + triangle.vertices[1].normal[2] * b1
                    + triangle.vertices[2].normal[2] * b2,
            ]);
            if use_texture > 0.5 {
                let uv = [
                    triangle.vertices[0].uv[0] * b0
                        + triangle.vertices[1].uv[0] * b1
                        + triangle.vertices[2].uv[0] * b2,
                    triangle.vertices[0].uv[1] * b0
                        + triangle.vertices[1].uv[1] * b1
                        + triangle.vertices[2].uv[1] * b2,
                ];
                let sampled = sample_preview_atlas(atlas, uv);
                if sampled[3] <= 0.01 {
                    continue;
                }
                color = [sampled[0], sampled[1], sampled[2], color[3] * sampled[3]];
            }
            let lit_rgb = apply_basic_lighting([color[0], color[1], color[2]], normal, lighting);
            color[0] = lit_rgb[0];
            color[1] = lit_rgb[1];
            color[2] = lit_rgb[2];
            if color[3] <= 0.01 {
                continue;
            }
            if let (Some(stats), Some(debug)) = (
                chest_debug_stats.as_deref_mut(),
                triangle.chest_debug.as_ref(),
            ) {
                stats.note_drawn(debug);
            }
            if let (Some(stats), Some(debug)) = (
                observer_debug_stats.as_deref_mut(),
                triangle.observer_debug.as_ref(),
            ) {
                stats.note_drawn(debug);
            }
            if mode == PreviewRasterMode::Opaque {
                if let (Some(stats), Some(debug)) = (
                    chest_debug_stats.as_deref_mut(),
                    triangle.chest_debug.as_ref(),
                ) {
                    stats.note_depth_result(debug, z < depth[index]);
                }
                if let (Some(stats), Some(debug)) = (
                    observer_debug_stats.as_deref_mut(),
                    triangle.observer_debug.as_ref(),
                ) {
                    stats.note_depth_result(debug, z < depth[index]);
                }
                depth[index] = z;
                image.put_pixel(
                    x,
                    y,
                    Rgb([
                        (color[0].clamp(0.0, 1.0) * 255.0) as u8,
                        (color[1].clamp(0.0, 1.0) * 255.0) as u8,
                        (color[2].clamp(0.0, 1.0) * 255.0) as u8,
                    ]),
                );
                continue;
            }
            if let (Some(stats), Some(debug)) = (
                chest_debug_stats.as_deref_mut(),
                triangle.chest_debug.as_ref(),
            ) {
                stats.note_depth_result(debug, true);
            }
            if let (Some(stats), Some(debug)) = (
                observer_debug_stats.as_deref_mut(),
                triangle.observer_debug.as_ref(),
            ) {
                stats.note_depth_result(debug, true);
            }
            let dst = image.get_pixel(x, y).0;
            let alpha = color[3].clamp(0.0, 1.0);
            let blended = [
                color[0] * alpha + (dst[0] as f32 / 255.0) * (1.0 - alpha),
                color[1] * alpha + (dst[1] as f32 / 255.0) * (1.0 - alpha),
                color[2] * alpha + (dst[2] as f32 / 255.0) * (1.0 - alpha),
            ];
            image.put_pixel(
                x,
                y,
                Rgb([
                    (blended[0].clamp(0.0, 1.0) * 255.0) as u8,
                    (blended[1].clamp(0.0, 1.0) * 255.0) as u8,
                    (blended[2].clamp(0.0, 1.0) * 255.0) as u8,
                ]),
            );
        }
    }
}

fn sample_preview_atlas(atlas: Option<&RgbaImage>, uv: [f32; 2]) -> [f32; 4] {
    let Some(atlas) = atlas else {
        return [1.0, 1.0, 1.0, 1.0];
    };
    let width = atlas.width().max(1);
    let height = atlas.height().max(1);
    let x = (uv[0].clamp(0.0, 0.999_999) * width as f32).floor() as u32;
    let y = (uv[1].clamp(0.0, 0.999_999) * height as f32).floor() as u32;
    let pixel = atlas.get_pixel(x.min(width - 1), y.min(height - 1)).0;
    [
        pixel[0] as f32 / 255.0,
        pixel[1] as f32 / 255.0,
        pixel[2] as f32 / 255.0,
        pixel[3] as f32 / 255.0,
    ]
}

#[derive(Clone)]
struct ChestDebugTriangleInfo {
    block_label: &'static str,
    face_role: &'static str,
    pair_half: &'static str,
    world_face: &'static str,
    candidate_label: &'static str,
    group_key: String,
    world_bounds: String,
    uv_bounds: String,
}

impl ChestDebugTriangleInfo {
    fn target_label(&self) -> String {
        format!("{}-{}", self.face_role, self.pair_half)
    }
}

#[derive(Default)]
struct ChestDebugPreviewStats {
    groups: HashMap<String, ChestDebugPreviewGroupStats>,
}

#[derive(Clone)]
struct ChestDebugPreviewGroupStats {
    block_label: &'static str,
    face_role: &'static str,
    pair_half: &'static str,
    world_face: &'static str,
    candidate_label: &'static str,
    world_bounds: String,
    uv_bounds: String,
    opaque_triangles: usize,
    translucent_triangles: usize,
    candidate_pixels: usize,
    drawn_pixels: usize,
    visible_pixels: usize,
}

impl ChestDebugPreviewStats {
    fn note_triangle(&mut self, info: &ChestDebugTriangleInfo, mode: PreviewRasterMode) {
        let entry = self
            .groups
            .entry(info.group_key.clone())
            .or_insert_with(|| ChestDebugPreviewGroupStats {
                block_label: info.block_label,
                face_role: info.face_role,
                pair_half: info.pair_half,
                world_face: info.world_face,
                candidate_label: info.candidate_label,
                world_bounds: info.world_bounds.clone(),
                uv_bounds: info.uv_bounds.clone(),
                opaque_triangles: 0,
                translucent_triangles: 0,
                candidate_pixels: 0,
                drawn_pixels: 0,
                visible_pixels: 0,
            });
        match mode {
            PreviewRasterMode::Opaque => entry.opaque_triangles += 1,
            PreviewRasterMode::Translucent => entry.translucent_triangles += 1,
        }
    }

    fn note_candidate(&mut self, info: &ChestDebugTriangleInfo) {
        if let Some(entry) = self.groups.get_mut(&info.group_key) {
            entry.candidate_pixels += 1;
        }
    }

    fn note_drawn(&mut self, info: &ChestDebugTriangleInfo) {
        if let Some(entry) = self.groups.get_mut(&info.group_key) {
            entry.drawn_pixels += 1;
        }
    }

    fn note_depth_result(&mut self, info: &ChestDebugTriangleInfo, visible: bool) {
        if visible && let Some(entry) = self.groups.get_mut(&info.group_key) {
            entry.visible_pixels += 1;
        }
    }

    fn log_summary(&self) {
        let mut groups = self.groups.values().cloned().collect::<Vec<_>>();
        groups.sort_by(|left, right| {
            right
                .visible_pixels
                .cmp(&left.visible_pixels)
                .then(right.drawn_pixels.cmp(&left.drawn_pixels))
                .then(left.block_label.cmp(right.block_label))
        });
        for group in groups {
            println!(
                "[LBA_FULL_MODE_V2_CHEST_PREVIEW] block={} target={} candidate={} world_face={} opaque_triangles={} translucent_triangles={} candidate_pixels={} drawn_pixels={} visible_pixels={} world_bounds={} uv_bounds={}",
                group.block_label,
                format!("{}-{}", group.face_role, group.pair_half),
                group.candidate_label,
                group.world_face,
                group.opaque_triangles,
                group.translucent_triangles,
                group.candidate_pixels,
                group.drawn_pixels,
                group.visible_pixels,
                group.world_bounds,
                group.uv_bounds,
            );
        }
    }
}

#[derive(Clone)]
struct ObserverDebugTriangleInfo {
    block_label: &'static str,
    face_number: &'static str,
    world_face: &'static str,
    group_key: String,
    world_bounds: String,
    uv_bounds: String,
}

#[derive(Default)]
struct ObserverDebugPreviewStats {
    groups: HashMap<String, ObserverDebugPreviewGroupStats>,
}

#[derive(Clone)]
struct ObserverDebugPreviewGroupStats {
    block_label: &'static str,
    face_number: &'static str,
    world_face: &'static str,
    world_bounds: String,
    uv_bounds: String,
    opaque_triangles: usize,
    translucent_triangles: usize,
    candidate_pixels: usize,
    drawn_pixels: usize,
    visible_pixels: usize,
}

impl ObserverDebugPreviewStats {
    fn note_triangle(&mut self, info: &ObserverDebugTriangleInfo, mode: PreviewRasterMode) {
        let entry = self
            .groups
            .entry(info.group_key.clone())
            .or_insert_with(|| ObserverDebugPreviewGroupStats {
                block_label: info.block_label,
                face_number: info.face_number,
                world_face: info.world_face,
                world_bounds: info.world_bounds.clone(),
                uv_bounds: info.uv_bounds.clone(),
                opaque_triangles: 0,
                translucent_triangles: 0,
                candidate_pixels: 0,
                drawn_pixels: 0,
                visible_pixels: 0,
            });
        match mode {
            PreviewRasterMode::Opaque => entry.opaque_triangles += 1,
            PreviewRasterMode::Translucent => entry.translucent_triangles += 1,
        }
    }

    fn note_candidate(&mut self, info: &ObserverDebugTriangleInfo) {
        if let Some(entry) = self.groups.get_mut(&info.group_key) {
            entry.candidate_pixels += 1;
        }
    }

    fn note_drawn(&mut self, info: &ObserverDebugTriangleInfo) {
        if let Some(entry) = self.groups.get_mut(&info.group_key) {
            entry.drawn_pixels += 1;
        }
    }

    fn note_depth_result(&mut self, info: &ObserverDebugTriangleInfo, visible: bool) {
        if visible && let Some(entry) = self.groups.get_mut(&info.group_key) {
            entry.visible_pixels += 1;
        }
    }

    fn log_summary(&self) {
        let mut groups = self.groups.values().cloned().collect::<Vec<_>>();
        groups.sort_by(|left, right| {
            left.block_label
                .cmp(right.block_label)
                .then(left.face_number.cmp(right.face_number))
        });
        for group in groups {
            println!(
                "[LBA_FULL_MODE_V2_OBSERVER_PREVIEW] block={} face={} world_face={} opaque_triangles={} translucent_triangles={} candidate_pixels={} drawn_pixels={} visible_pixels={} world_bounds={} uv_bounds={}",
                group.block_label,
                group.face_number,
                group.world_face,
                group.opaque_triangles,
                group.translucent_triangles,
                group.candidate_pixels,
                group.drawn_pixels,
                group.visible_pixels,
                group.world_bounds,
                group.uv_bounds,
            );
        }
    }
}

fn classify_chest_debug_triangle(vertices: [&GpuVertex; 3]) -> Option<ChestDebugTriangleInfo> {
    std::env::var_os("LBA_FULL_MODE_V2_CHEST_DEBUG")?;
    let positions = vertices.map(|vertex| vertex.position);
    let centroid = [
        (positions[0][0] + positions[1][0] + positions[2][0]) / 3.0,
        (positions[0][1] + positions[1][1] + positions[2][1]) / 3.0,
        (positions[0][2] + positions[1][2] + positions[2][2]) / 3.0,
    ];
    let ab = [
        positions[1][0] - positions[0][0],
        positions[1][1] - positions[0][1],
        positions[1][2] - positions[0][2],
    ];
    let ac = [
        positions[2][0] - positions[0][0],
        positions[2][1] - positions[0][1],
        positions[2][2] - positions[0][2],
    ];
    let normal = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    let within_block = |origin: [f32; 3]| {
        centroid[0] >= origin[0] - 0.01
            && centroid[0] <= origin[0] + 1.01
            && centroid[1] >= origin[1] - 0.01
            && centroid[1] <= origin[1] + 1.01
            && centroid[2] >= origin[2] - 0.01
            && centroid[2] <= origin[2] + 1.01
    };
    let world_face = if normal[0] > 0.0001 && normal[0].abs() >= normal[2].abs() {
        Some("east")
    } else if normal[0] < -0.0001 && normal[0].abs() >= normal[2].abs() {
        Some("west")
    } else {
        None
    }?;
    let (block_label, face_role, pair_half) = match world_face {
        "east" if within_block([47.0, 1.0, 114.0]) => ("C49-6", "F", "R"),
        "west" if within_block([47.0, 1.0, 114.0]) => ("C49-6", "B", "L"),
        "east" if within_block([47.0, 1.0, 115.0]) => ("C55-6", "F", "L"),
        "west" if within_block([47.0, 1.0, 115.0]) => ("C55-6", "B", "R"),
        "west" if within_block([33.0, 1.0, 115.0]) => ("C54-5", "F", "L"),
        "east" if within_block([33.0, 1.0, 115.0]) => ("C54-5", "B", "R"),
        "west" if within_block([33.0, 1.0, 116.0]) => ("C56-5", "F", "R"),
        "east" if within_block([33.0, 1.0, 116.0]) => ("C56-5", "B", "L"),
        _ => return None,
    };
    let candidate_label = classify_chest_debug_candidate(face_role, positions);
    Some(ChestDebugTriangleInfo {
        block_label,
        face_role,
        pair_half,
        world_face,
        candidate_label,
        group_key: format!(
            "{}|{}-{}|{}|{}|{}|{}",
            block_label,
            face_role,
            pair_half,
            candidate_label,
            world_face,
            quantized_world_bounds(positions),
            quantized_uv_bounds(vertices),
        ),
        world_bounds: quantized_world_bounds(positions),
        uv_bounds: quantized_uv_bounds(vertices),
    })
}

fn classify_observer_debug_triangle(
    vertices: [&GpuVertex; 3],
) -> Option<ObserverDebugTriangleInfo> {
    std::env::var_os("LBA_FULL_MODE_V2_OBSERVER_DEBUG")?;
    let positions = vertices.map(|vertex| vertex.position);
    let centroid = [
        (positions[0][0] + positions[1][0] + positions[2][0]) / 3.0,
        (positions[0][1] + positions[1][1] + positions[2][1]) / 3.0,
        (positions[0][2] + positions[1][2] + positions[2][2]) / 3.0,
    ];
    let Some(block_label) = observer_debug_block_label(centroid) else {
        return None;
    };
    let world_face = dominant_world_face_from_positions(positions)?;
    let face_number = match world_face {
        "up" => "1",
        "down" => "2",
        "north" => "3",
        "south" => "4",
        "west" => "5",
        "east" => "6",
        _ => return None,
    };
    if !matches!(
        (block_label, face_number),
        ("O05", "1") | ("O06", "1") | ("O11", "1") | ("O12", "1")
    ) {
        return None;
    }
    Some(ObserverDebugTriangleInfo {
        block_label,
        face_number,
        world_face,
        group_key: format!("{block_label}-{face_number}"),
        world_bounds: quantized_world_bounds(positions),
        uv_bounds: quantized_uv_bounds(vertices),
    })
}

fn observer_debug_block_label(centroid: [f32; 3]) -> Option<&'static str> {
    if (centroid[1] - 2.025).abs() > 0.15 {
        return None;
    }
    const TARGETS: [(&str, f32, f32); 4] = [
        ("O05", 54.0, 5.0),
        ("O06", 66.0, 5.0),
        ("O11", 54.0, 15.0),
        ("O12", 66.0, 15.0),
    ];
    TARGETS
        .into_iter()
        .find(|(_, x, z)| (centroid[0] - *x).abs() <= 1.2 && (centroid[2] - *z).abs() <= 1.2)
        .map(|(label, _, _)| label)
}

fn dominant_world_face_from_positions(positions: [[f32; 3]; 3]) -> Option<&'static str> {
    let ab = [
        positions[1][0] - positions[0][0],
        positions[1][1] - positions[0][1],
        positions[1][2] - positions[0][2],
    ];
    let ac = [
        positions[2][0] - positions[0][0],
        positions[2][1] - positions[0][1],
        positions[2][2] - positions[0][2],
    ];
    let normal = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    let abs = [normal[0].abs(), normal[1].abs(), normal[2].abs()];
    if abs[0] <= f32::EPSILON && abs[1] <= f32::EPSILON && abs[2] <= f32::EPSILON {
        return None;
    }
    Some(if abs[1] >= abs[0] && abs[1] >= abs[2] {
        if normal[1] >= 0.0 { "up" } else { "down" }
    } else if abs[0] >= abs[2] {
        if normal[0] >= 0.0 { "east" } else { "west" }
    } else if normal[2] >= 0.0 {
        "south"
    } else {
        "north"
    })
}

fn log_chest_front_triangle(
    stage: &str,
    vertices: [&GpuVertex; 3],
    debug: Option<&ChestDebugTriangleInfo>,
) {
    let Some(debug) = debug else {
        return;
    };
    println!(
        "[LBA_FULL_MODE_V2_CHEST_NATIVE] stage={} block={} target={} candidate={} world_face={} positions={} uvs={} world_bounds={} uv_bounds={} use_texture={:.1}/{:.1}/{:.1}",
        stage,
        debug.block_label,
        debug.target_label(),
        debug.candidate_label,
        debug.world_face,
        format_gpu_positions(vertices),
        format_gpu_uvs(vertices),
        debug.world_bounds,
        debug.uv_bounds,
        vertices[0].use_texture,
        vertices[1].use_texture,
        vertices[2].use_texture,
    );
}

fn log_chest_front_gpu_vertices(stage: &str, vertices: &[GpuVertex], indices: &[u32]) {
    if std::env::var_os("LBA_FULL_MODE_V2_CHEST_DEBUG").is_none() {
        return;
    }
    for tri in indices.chunks_exact(3) {
        let Some(v0) = vertices.get(tri[0] as usize) else {
            continue;
        };
        let Some(v1) = vertices.get(tri[1] as usize) else {
            continue;
        };
        let Some(v2) = vertices.get(tri[2] as usize) else {
            continue;
        };
        let debug = classify_chest_debug_triangle([v0, v1, v2]);
        log_chest_front_triangle(stage, [v0, v1, v2], debug.as_ref());
    }
}

fn classify_chest_debug_candidate(face_role: &str, positions: [[f32; 3]; 3]) -> &'static str {
    let min_y = positions.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
    let max_y = positions
        .iter()
        .map(|p| p[1])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_z = positions.iter().map(|p| p[2]).fold(f32::INFINITY, f32::min);
    let max_z = positions
        .iter()
        .map(|p| p[2])
        .fold(f32::NEG_INFINITY, f32::max);
    let y_span = max_y - min_y;
    let z_span = max_z - min_z;
    match (face_role, y_span, z_span, min_y) {
        ("F", y, z, min) if y >= 0.5 && z >= 0.8 && min < 1.1 => "base_front_candidate_A",
        ("B", y, z, min) if y >= 0.5 && z >= 0.8 && min < 1.1 => "base_back_candidate_A",
        ("F", y, z, min) if y >= 0.2 && z >= 0.8 && min >= 1.5 => "lid_front_candidate_B",
        ("B", y, z, min) if y >= 0.2 && z >= 0.8 && min >= 1.5 => "lid_back_candidate_B",
        ("F", y, z, _) if y <= 0.3 && z <= 0.1 => "lock_front_candidate_C",
        ("B", y, z, _) if y <= 0.3 && z <= 0.1 => "lock_back_candidate_C",
        ("F", ..) => "other_front_candidate_Z",
        _ => "other_back_candidate_Z",
    }
}

fn format_gpu_positions(vertices: [&GpuVertex; 3]) -> String {
    format!(
        "[{:?},{:?},{:?}]",
        vertices[0].position, vertices[1].position, vertices[2].position
    )
}

fn format_gpu_uvs(vertices: [&GpuVertex; 3]) -> String {
    format!(
        "[{:?},{:?},{:?}]",
        vertices[0].uv, vertices[1].uv, vertices[2].uv
    )
}

fn quantized_world_bounds(positions: [[f32; 3]; 3]) -> String {
    let min_x = positions.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
    let max_x = positions
        .iter()
        .map(|p| p[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = positions.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
    let max_y = positions
        .iter()
        .map(|p| p[1])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_z = positions.iter().map(|p| p[2]).fold(f32::INFINITY, f32::min);
    let max_z = positions
        .iter()
        .map(|p| p[2])
        .fold(f32::NEG_INFINITY, f32::max);
    format!(
        "[{:.4}..{:.4},{:.4}..{:.4},{:.4}..{:.4}]",
        min_x, max_x, min_y, max_y, min_z, max_z
    )
}

fn quantized_uv_bounds(vertices: [&GpuVertex; 3]) -> String {
    let min_u = vertices
        .iter()
        .map(|vertex| vertex.uv[0])
        .fold(f32::INFINITY, f32::min);
    let max_u = vertices
        .iter()
        .map(|vertex| vertex.uv[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_v = vertices
        .iter()
        .map(|vertex| vertex.uv[1])
        .fold(f32::INFINITY, f32::min);
    let max_v = vertices
        .iter()
        .map(|vertex| vertex.uv[1])
        .fold(f32::NEG_INFINITY, f32::max);
    format!("[{:.6}..{:.6},{:.6}..{:.6}]", min_u, max_u, min_v, max_v)
}

fn edge_preview(a: [f32; 3], b: [f32; 3], x: f32, y: f32) -> f32 {
    (x - a[0]) * (b[1] - a[1]) - (y - a[1]) * (b[0] - a[0])
}

fn write_headless_progress_signal(
    args: &ViewerArgs,
    scene_index: &ChunkSceneIndex,
    total_chunks: usize,
    built_chunks: usize,
    renderable_chunks: usize,
    empty_mesh_chunks: usize,
    cache_file_bytes: usize,
    elapsed: Duration,
    ready: bool,
) {
    let Some(path) = args.ready_file.as_ref() else {
        return;
    };
    let metadata = scene_index.metadata();
    let percent = if ready {
        100.0
    } else if total_chunks == 0 {
        99.9
    } else {
        ((built_chunks as f64 / total_chunks as f64) * 100.0).clamp(0.0, 99.9)
    };
    let payload = format!(
        "{{\"ready\":{},\"total_chunks\":{},\"scene_size_x\":{},\"scene_size_y\":{},\"scene_size_z\":{},\"built_chunks\":{},\"renderable_chunks\":{},\"empty_mesh_chunks\":{},\"uploaded_chunks\":{},\"resident_chunks\":{},\"cache_file_bytes\":{},\"percent\":{:.2},\"phase\":\"{}\",\"elapsed_ms\":{},\"eta_seconds\":{}}}",
        if ready { "true" } else { "false" },
        total_chunks,
        metadata.enclosing_size.x,
        metadata.enclosing_size.y,
        metadata.enclosing_size.z,
        built_chunks,
        renderable_chunks,
        empty_mesh_chunks,
        built_chunks,
        built_chunks,
        cache_file_bytes,
        percent,
        if ready { "ready" } else { "build" },
        elapsed.as_millis(),
        if ready { "0" } else { "null" },
    );
    if let Err(error) = std::fs::write(path, payload) {
        println!(
            "[NATIVE_VIEWER] progress_write_failed path={} error={}",
            path.display(),
            error
        );
        return;
    }
    if ready {
        println!(
            "[NATIVE_VIEWER] ready_written path={} elapsed_ms={} resident_chunks={} renderable_chunks={} empty_mesh_chunks={} total_chunks={} cache_file_bytes={}",
            path.display(),
            elapsed.as_millis(),
            built_chunks,
            renderable_chunks,
            empty_mesh_chunks,
            total_chunks,
            cache_file_bytes
        );
    }
}

fn write_native_cache_layer_index(path: &Path, scene_index: &ChunkSceneIndex) -> Result<String> {
    let started_at = Instant::now();
    let manifest_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "native_preview_cache.json".to_string());
    let file_name = format!("{manifest_name}.layers.json");
    let layer_path = path
        .parent()
        .map(|parent| parent.join(&file_name))
        .unwrap_or_else(|| PathBuf::from(&file_name));
    if let Some(parent) = layer_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let output = scene_index.build_layer_index_output()?;
    let layer_count = output.visual.layers.len();
    let block_count = output
        .visual
        .layers
        .iter()
        .map(|layer| layer.blocks.len())
        .sum::<usize>();
    File::create(&layer_path)
        .map(BufWriter::new)
        .and_then(|mut writer| {
            serde_json::to_writer(&mut writer, &output).map_err(std::io::Error::other)?;
            writer.flush()?;
            Ok(())
        })
        .with_context(|| format!("write layer cache failed: {}", layer_path.display()))?;
    let bytes = std::fs::metadata(&layer_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    println!(
        "[LAYER_CACHE] layer_index_written manifest_path={} path={} layers={} blocks={} bytes={} elapsed_ms={}",
        path.display(),
        layer_path.display(),
        layer_count,
        block_count,
        bytes,
        started_at.elapsed().as_millis()
    );
    Ok(file_name)
}

fn write_native_cache_manifest(
    path: &Path,
    chunk_dir: &Path,
    chunk_dir_name: String,
    metadata: MetadataOutput,
    total_chunks: usize,
    empty_mesh_chunks: usize,
    mut manifest_chunks: Vec<NativePreviewCacheManifestChunk>,
    total_chunk_bytes: usize,
    started_at: Instant,
    layer_index_file: Option<String>,
) -> Result<usize> {
    manifest_chunks.sort_by_key(|chunk| (chunk.cy, chunk.cx, chunk.cz));
    let payload = NativePreviewCacheManifest {
        format: "native_preview_cache_manifest_v3".to_string(),
        color_chain: "shared_native_cache".to_string(),
        metadata,
        total_chunks,
        renderable_chunks: manifest_chunks.len(),
        empty_mesh_chunks,
        chunk_data_dir: chunk_dir_name,
        layer_index_file,
        chunks: manifest_chunks,
    };
    let manifest_bytes = File::create(path)
        .map(BufWriter::new)
        .and_then(|mut writer| {
            serde_json::to_writer(&mut writer, &payload).map_err(std::io::Error::other)?;
            writer.flush()?;
            Ok(())
        })
        .and_then(|_| std::fs::metadata(path).map(|metadata| metadata.len() as usize))
        .with_context(|| format!("write cache manifest failed: {}", path.display()))?;
    println!(
        "[PREVIEW_CACHE] chunked_cache_manifest path={} chunk_dir={} renderable_chunks={} empty_mesh_chunks={} total_chunks={} color_chain=shared_native_cache",
        path.display(),
        chunk_dir.display(),
        payload.renderable_chunks,
        payload.empty_mesh_chunks,
        payload.total_chunks
    );
    println!(
        "[PREVIEW_CACHE] chunked_cache_written manifest_path={} chunk_dir={} chunk_files={} manifest_bytes={} chunk_bytes={} total_bytes={} cache_write_ms={}",
        path.display(),
        chunk_dir.display(),
        payload.chunks.len(),
        manifest_bytes,
        total_chunk_bytes,
        manifest_bytes + total_chunk_bytes,
        started_at.elapsed().as_millis()
    );
    Ok(manifest_bytes + total_chunk_bytes)
}

fn run_headless_prebuild(args: &ViewerArgs, total_trace_started_at: Instant) -> Result<()> {
    let Some(input) = args.input.as_deref() else {
        bail!("headless prebuild requires an input .litematic file");
    };
    let Some(cache_file) = args.cache_file.as_deref() else {
        bail!("headless prebuild requires --cache-file");
    };
    let prebuild_started_at = Instant::now();
    println!("[TRACE_STARTUP] mode=headless_prebuild reason=skip_window_wgpu_bootstrap");
    println!(
        "[VIEWER_SCENE] loading_litematic file={} chunk_size={}",
        input.display(),
        args.chunk_size
    );
    let scene_index = Arc::new(ChunkSceneIndex::load(input, args.chunk_size)?);
    let palette_colors = Arc::new(resolve_scene_palette_colors(
        &scene_index,
        args.display_mode,
    ));
    let bounds = SceneBounds::from_metadata(scene_index.metadata());
    println!(
        "[VIEWER_CHUNK] index_ready total_chunks={} chunk_size={} scene_radius={:.2}",
        scene_index.chunk_count(),
        args.chunk_size,
        bounds.radius
    );
    println!("[TRACE_STARTUP] stage=wgpu_total elapsed_ms=0 mode=headless_prebuild");

    let trace = Arc::new(TraceCounters::default());
    let mut pipeline = if writer_pipeline_enabled() {
        Some(PrebuildCachePipeline::start(cache_file, trace.clone())?)
    } else {
        println!("[TRACE_WALL] stage=cache_write event=pipeline_disabled reason=env_disabled");
        None
    };
    let (request_tx, result_rx, workers) = spawn_build_worker_pool(
        scene_index.clone(),
        palette_colors,
        None,
        args.build_workers,
        trace.clone(),
    );

    let entries = scene_index.chunk_entries().to_vec();
    let total_chunks = entries.len();
    let batch_size = if total_chunks > 512 {
        32_usize
    } else {
        16_usize
    };
    println!(
        "[TRACE_WALL] stage=chunk_task event=headless_batch_plan total_chunks={} batch_size={} worker_count={}",
        total_chunks, batch_size, args.build_workers
    );
    let mut submitted_batches = 0_usize;
    for chunk in entries.chunks(batch_size) {
        let keys = chunk.iter().map(|entry| entry.key).collect::<Vec<_>>();
        request_tx.send(BuildWorkerRequest {
            batch_id: submitted_batches as u64 + 1,
            keys: keys.clone(),
            priority: QueuePriority::Target,
            enqueued_at: Instant::now(),
        })?;
        trace.build_enqueue_batches.fetch_add(1, Ordering::Relaxed);
        trace
            .build_enqueue_chunks
            .fetch_add(keys.len(), Ordering::Relaxed);
        TraceCounters::update_max(
            &trace.max_in_flight_builds,
            (submitted_batches + 1) * batch_size,
        );
        submitted_batches += 1;
    }
    TraceCounters::update_max(&trace.max_pending_build_queue, total_chunks);
    drop(request_tx);

    let mut completed_chunks = 0_usize;
    let mut renderable_chunks = 0_usize;
    let mut empty_mesh_chunks = 0_usize;
    let mut manifest_chunks = Vec::<NativePreviewCacheManifestChunk>::new();
    let mut total_chunk_bytes = 0_usize;
    let mut writer_submitted_chunks = 0_usize;
    let mut writer_completed_chunks = 0_usize;
    let mut cache_written_keys = HashSet::<ChunkKey>::new();
    let mut cpu_meshes = Vec::<PreparedChunkMesh>::new();
    let mut last_progress = Instant::now();

    while completed_chunks < total_chunks {
        match result_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => {
                if let Some(error) = result.error {
                    bail!(
                        "headless build failed for chunk ({}, {}, {}): {}",
                        result.key.cx,
                        result.key.cy,
                        result.key.cz,
                        error
                    );
                }
                completed_chunks += 1;
                if let Some(mesh) = result.mesh {
                    renderable_chunks += 1;
                    if let Some(pipeline) = pipeline.as_ref() {
                        if let Some(request_tx) = pipeline.request_tx.as_ref() {
                            request_tx.send(CacheWriterRequest {
                                mesh,
                                priority: QueuePriority::Target,
                                submitted_at: Instant::now(),
                            })?;
                            writer_submitted_chunks += 1;
                        }
                    } else {
                        cpu_meshes.push(mesh);
                    }
                } else {
                    empty_mesh_chunks += 1;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if let Some(pipeline) = pipeline.as_ref() {
            while let Ok(result) = pipeline.result_rx.try_recv() {
                if let Some(error) = result.error {
                    bail!("headless cache write failed: {error}");
                }
                if let Some(chunk) = result.manifest_chunk {
                    cache_written_keys.insert(result.mesh.key);
                    total_chunk_bytes = total_chunk_bytes.saturating_add(result.bytes);
                    manifest_chunks.push(chunk);
                    writer_completed_chunks += 1;
                }
            }
        }
        if last_progress.elapsed() >= Duration::from_millis(500) {
            last_progress = Instant::now();
            write_headless_progress_signal(
                args,
                &scene_index,
                total_chunks,
                completed_chunks,
                renderable_chunks,
                empty_mesh_chunks,
                total_chunk_bytes,
                prebuild_started_at.elapsed(),
                false,
            );
        }
    }
    for worker in workers {
        let _ = worker.join();
    }

    let finalize_started_at = Instant::now();
    let cache_write_started_at = Instant::now();
    let (chunk_dir_name, chunk_dir) = if let Some(mut pipeline) = pipeline.take() {
        if let Some(request_tx) = pipeline.request_tx.take() {
            drop(request_tx);
        }
        if let Some(worker) = pipeline.worker.take() {
            let _ = worker.join();
        }
        while let Ok(result) = pipeline.result_rx.try_recv() {
            if let Some(error) = result.error {
                bail!("headless cache write failed: {error}");
            }
            if let Some(chunk) = result.manifest_chunk
                && cache_written_keys.insert(result.mesh.key)
            {
                total_chunk_bytes = total_chunk_bytes.saturating_add(result.bytes);
                manifest_chunks.push(chunk);
                writer_completed_chunks += 1;
            }
        }
        (pipeline.chunk_dir_name, pipeline.chunk_dir)
    } else {
        if let Some(parent) = cache_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let manifest_name = cache_file
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "native_preview_cache.json".to_string());
        let chunk_dir_name = format!("{manifest_name}.chunks");
        let chunk_dir = cache_file
            .parent()
            .map(|parent| parent.join(&chunk_dir_name))
            .unwrap_or_else(|| PathBuf::from(&chunk_dir_name));
        let _ = std::fs::remove_dir_all(&chunk_dir);
        std::fs::create_dir_all(&chunk_dir)?;
        for mesh in &cpu_meshes {
            let file_name = cache_chunk_file_name(mesh.key);
            let chunk_path = chunk_dir.join(&file_name);
            let chunk_started_at = Instant::now();
            let bytes = write_prepared_cache_chunk_binary(&chunk_path, mesh)?;
            TraceCounters::add_ms(&trace.writer_busy_ms, chunk_started_at.elapsed());
            trace.writer_chunks.fetch_add(1, Ordering::Relaxed);
            total_chunk_bytes = total_chunk_bytes.saturating_add(bytes);
            manifest_chunks.push(NativePreviewCacheManifestChunk {
                cx: mesh.key.cx,
                cy: mesh.key.cy,
                cz: mesh.key.cz,
                file: file_name,
                vertex_count: mesh.vertex_count(),
                index_count: mesh.index_count(),
                bytes,
            });
        }
        (chunk_dir_name, chunk_dir)
    };
    let layer_index_file = Some(write_native_cache_layer_index(cache_file, &scene_index)?);
    let total_bytes = write_native_cache_manifest(
        cache_file,
        &chunk_dir,
        chunk_dir_name,
        scene_index.metadata().clone(),
        total_chunks,
        empty_mesh_chunks,
        manifest_chunks,
        total_chunk_bytes,
        cache_write_started_at,
        layer_index_file,
    )?;
    let cache_write_ms = cache_write_started_at.elapsed().as_millis();
    write_headless_progress_signal(
        args,
        &scene_index,
        total_chunks,
        total_chunks,
        renderable_chunks,
        empty_mesh_chunks,
        total_bytes,
        prebuild_started_at.elapsed(),
        true,
    );
    let finalize_ms = finalize_started_at.elapsed().as_millis();
    let total_wall_ms = prebuild_started_at.elapsed().as_millis();
    println!(
        "[PREBUILD_TIMING] finalize_ms={} cache_write_ms={} total_elapsed_ms={}",
        finalize_ms, cache_write_ms, total_wall_ms
    );
    println!(
        "[TRACE_SUMMARY] total_wall_ms={} worker_busy_ms={} worker_wait_ms={} worker_batches={} worker_chunks={} worker_result_send_ms={} build_enqueue_ms={} build_enqueue_batches={} build_enqueue_chunks={} build_dequeue_wait_ms={} build_dequeue_batches={} build_dequeue_chunks={} writer_queue_wait_ms={} writer_busy_ms={} writer_chunks={} writer_errors={} writer_submitted_chunks={} writer_completed_chunks={} cache_file_create_ms={} max_pending_build_queue={} max_in_flight_builds={} max_pending_upload_queue=0 pending_builds=0 in_flight_builds=0 pending_uploads=0 writer_model={}",
        total_wall_ms,
        trace.worker_busy_ms.load(Ordering::Relaxed),
        trace.worker_wait_ms.load(Ordering::Relaxed),
        trace.worker_batches.load(Ordering::Relaxed),
        trace.worker_chunks.load(Ordering::Relaxed),
        trace.worker_result_send_ms.load(Ordering::Relaxed),
        trace.build_enqueue_ms.load(Ordering::Relaxed),
        trace.build_enqueue_batches.load(Ordering::Relaxed),
        trace.build_enqueue_chunks.load(Ordering::Relaxed),
        trace.build_dequeue_ms.load(Ordering::Relaxed),
        trace.build_dequeue_batches.load(Ordering::Relaxed),
        trace.build_dequeue_chunks.load(Ordering::Relaxed),
        trace.writer_wait_ms.load(Ordering::Relaxed),
        trace.writer_busy_ms.load(Ordering::Relaxed),
        trace.writer_chunks.load(Ordering::Relaxed),
        trace.writer_errors.load(Ordering::Relaxed),
        writer_submitted_chunks,
        writer_completed_chunks,
        trace.cache_file_create_ms.load(Ordering::Relaxed),
        total_chunks,
        trace.max_in_flight_builds.load(Ordering::Relaxed),
        if writer_pipeline_enabled() {
            "headless_pipeline"
        } else {
            "headless_synchronous"
        }
    );
    println!(
        "[TRACE_CRITICAL_PATH] total_wall_ms={} finalize_ms={} cache_write_ms={} writer_queue_wait_ms={} writer_busy_ms_cumulative={} worker_busy_ms_cumulative={} worker_wait_ms_cumulative={} build_queue_wait_ms_cumulative={}",
        total_wall_ms,
        finalize_ms,
        cache_write_ms,
        trace.writer_wait_ms.load(Ordering::Relaxed),
        trace.writer_busy_ms.load(Ordering::Relaxed),
        trace.worker_busy_ms.load(Ordering::Relaxed),
        trace.worker_wait_ms.load(Ordering::Relaxed),
        trace.build_dequeue_ms.load(Ordering::Relaxed)
    );
    println!(
        "[TRACE_WALL] stage=total event=end elapsed_ms={}",
        total_trace_started_at.elapsed().as_millis()
    );
    println!("[NATIVE_VIEWER] prebuild_only=true exit_after_prebuild=true");
    Ok(())
}

fn probe_scene(scene: &ViewerScene) -> Result<()> {
    match scene.chunk_scene_index.as_ref() {
        Some(scene_index) => {
            let camera = OrbitCamera::from_bounds(scene.bounds, 1280, 800);
            let plan = ChunkResidencyPlan::build(
                scene_index,
                &camera,
                &ChunkStreamingConfig::default(),
                0,
                false,
                false,
            );
            let probe_keys = plan
                .upload_order
                .iter()
                .copied()
                .take(8)
                .collect::<Vec<_>>();
            let probe_meshes = if let Some(materials) = scene.full_mode_materials.as_ref() {
                scene_index.build_textured_chunk_meshes(&probe_keys, materials)?
            } else {
                scene_index.build_chunk_meshes(&probe_keys)?
            };
            let probe_vertex_count = probe_meshes
                .iter()
                .map(|chunk| {
                    chunk
                        .vertices
                        .len()
                        .saturating_add(chunk.textured_vertices.len())
                })
                .sum::<usize>();
            let probe_index_count = probe_meshes
                .iter()
                .map(|chunk| {
                    chunk
                        .indices
                        .len()
                        .saturating_add(chunk.translucent_indices.len())
                })
                .sum::<usize>();
            println!(
                "[NATIVE_VIEWER] probe_scene_ready total_chunks={} probe_chunks={} probe_vertices={} probe_indices={} radius={:.2}",
                scene_index.chunk_count(),
                probe_keys.len(),
                probe_vertex_count,
                probe_index_count,
                scene.bounds.radius
            );
        }
        None => {
            println!(
                "[NATIVE_VIEWER] probe_scene_ready total_chunks={} probe_chunks={} probe_vertices={} probe_indices={} radius={:.2}",
                scene.bootstrap_meshes.len(),
                scene.bootstrap_meshes.len(),
                scene
                    .bootstrap_meshes
                    .iter()
                    .map(|mesh| mesh.vertices.len())
                    .sum::<usize>(),
                scene
                    .bootstrap_meshes
                    .iter()
                    .map(|mesh| mesh.indices.len())
                    .sum::<usize>(),
                scene.bounds.radius
            );
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn window_hwnd(window: &winit::window::Window) -> Result<isize> {
    match window.window_handle()?.as_raw() {
        RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get()),
        other => bail!("embed_parent_hwnd requires Win32 window handle, got {other:?}"),
    }
}

#[cfg(target_os = "windows")]
fn embedded_parent_client_size(parent_hwnd: isize) -> Result<PhysicalSize<u32>> {
    let mut rect = WinRect::default();
    let ok = unsafe { GetClientRect(parent_hwnd, &mut rect) };
    if ok == 0 {
        bail!("GetClientRect failed for parent_hwnd={parent_hwnd}");
    }
    let width = (rect.right - rect.left).max(1) as u32;
    let height = (rect.bottom - rect.top).max(1) as u32;
    Ok(PhysicalSize::new(width, height))
}

#[cfg(target_os = "windows")]
fn sync_embedded_window_to_parent(
    window: &winit::window::Window,
    parent_hwnd: isize,
) -> Result<PhysicalSize<u32>> {
    let hwnd = window_hwnd(window)?;
    let client_size = embedded_parent_client_size(parent_hwnd)?;
    let swp_no_zorder = 0x0004_u32;
    let swp_no_activate = 0x0010_u32;
    let swp_frame_changed = 0x0020_u32;
    let ok = unsafe {
        SetWindowPos(
            hwnd,
            0,
            0,
            0,
            client_size.width as i32,
            client_size.height as i32,
            swp_no_zorder | swp_no_activate | swp_frame_changed,
        )
    };
    if ok == 0 {
        bail!(
            "SetWindowPos failed for hwnd={} parent_hwnd={} target={}x{}",
            hwnd,
            parent_hwnd,
            client_size.width,
            client_size.height
        );
    }
    Ok(client_size)
}

#[cfg(target_os = "windows")]
fn attach_window_to_parent(window: &winit::window::Window, parent_hwnd: isize) -> Result<()> {
    let hwnd = window_hwnd(window)?;
    let gwl_style = -16_i32;
    let ws_child = 0x40000000_isize;
    let ws_popup = 0x80000000_isize;
    let ws_caption = 0x00C00000_isize;
    let ws_thickframe = 0x00040000_isize;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, gwl_style);
        let style = (style | ws_child) & !ws_popup & !ws_caption & !ws_thickframe;
        SetWindowLongPtrW(hwnd, gwl_style, style);
        SetParent(hwnd, parent_hwnd);
        ShowWindow(hwnd, 5);
    }
    let client_size = sync_embedded_window_to_parent(window, parent_hwnd)?;
    println!(
        "[NATIVE_VIEWER] launch_mode=embedded external_window_created=false reparent=true hwnd={} parent_hwnd={} viewport_w={} viewport_h={}",
        hwnd, parent_hwnd, client_size.width, client_size.height,
    );
    Ok(())
}

pub fn run() -> Result<()> {
    let Some(args) = parse_args()? else {
        return Ok(());
    };
    println!(
        "[NATIVE_VIEWER] startup file={} chunk_size={} probe_scene={} prebuild_only={} auto_exit_seconds={} prebuild_before_show={} cache_input={} ready_file={} cache_file={} preview_output={} build_workers={} cpu_cache_max_chunks={} cpu_cache_max_bytes={} resident_max_chunks={} resident_max_bytes={} stress_limits={} preview_mode={} preview_spin={} display_mode={} basic_lighting={} basic_shadows={} shadow_debug={} shadow_debug_view={} shadow_force_test={} launch_mode={} embed_parent_hwnd={}",
        args.input
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<placeholder>".to_string()),
        args.chunk_size,
        args.probe_scene,
        args.prebuild_only,
        args.auto_exit_seconds
            .map(|seconds| seconds.to_string())
            .unwrap_or_else(|| "none".to_string()),
        args.prebuild_before_show,
        args.cache_input
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "none".to_string()),
        args.ready_file
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "none".to_string()),
        args.cache_file
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "none".to_string()),
        args.preview_output
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "none".to_string()),
        args.build_workers,
        args.cpu_cache_max_chunks,
        args.cpu_cache_max_bytes,
        args.resident_max_chunks,
        args.resident_max_bytes,
        args.stress_limits,
        args.preview_mode,
        args.preview_spin,
        args.display_mode.label(),
        args.basic_lighting,
        args.basic_shadows,
        args.shadow_debug,
        args.shadow_debug_view.label(),
        args.shadow_force_test,
        if args.embed_parent_hwnd.is_some() {
            "embedded"
        } else {
            "popup"
        },
        args.embed_parent_hwnd
            .map(|hwnd| hwnd.to_string())
            .unwrap_or_else(|| "none".to_string())
    );
    let total_trace_started_at = Instant::now();
    println!("[TRACE_WALL] stage=total event=start");
    if args.prebuild_only
        && args.prebuild_before_show
        && args.cache_input.is_none()
        && args.input.is_some()
        && args.cache_file.is_some()
        && headless_prebuild_enabled()
    {
        return run_headless_prebuild(&args, total_trace_started_at);
    }
    let scene = load_scene(&args)?;
    if let Some(output) = args.preview_output.as_deref() {
        return render_offscreen_preview(
            &scene,
            output,
            LightingConfig::from_args(
                args.basic_lighting,
                args.basic_shadows,
                args.shadow_debug_view,
                args.shadow_force_test,
                scene.bounds,
            ),
        );
    }
    if args.probe_scene {
        probe_scene(&scene)?;
        return Ok(());
    }

    let title = format!("Litematica Native Viewer - {}", scene.label);
    let event_loop = EventLoop::new()?;
    let viewer_started_at = Instant::now();
    let auto_exit_after = args.auto_exit_seconds.map(Duration::from_secs);
    let window = Arc::new(
        WindowBuilder::new()
            .with_title(title)
            .with_inner_size(PhysicalSize::new(1280_u32, 800_u32))
            .with_decorations(args.embed_parent_hwnd.is_none())
            .with_visible(!args.prebuild_before_show && args.embed_parent_hwnd.is_none())
            .build(&event_loop)
            .context("failed to create native viewer window")?,
    );
    #[cfg(target_os = "windows")]
    if let Some(parent_hwnd) = args.embed_parent_hwnd {
        attach_window_to_parent(window.as_ref(), parent_hwnd)?;
    } else {
        println!("[NATIVE_VIEWER] launch_mode=popup external_window_created=true reparent=false");
    }
    #[cfg(not(target_os = "windows"))]
    {
        if args.embed_parent_hwnd.is_some() {
            println!(
                "[NATIVE_VIEWER] launch_mode=embedded external_window_created=true reparent=false reason=unsupported_platform"
            );
        } else {
            println!(
                "[NATIVE_VIEWER] launch_mode=popup external_window_created=true reparent=false"
            );
        }
    }

    let mut state = pollster::block_on(ViewerState::new(window.clone(), scene, &args))?;
    if args.prebuild_before_show {
        state.run_prebuild_until_ready(window.as_ref(), viewer_started_at)?;
    }
    if args.prebuild_only {
        println!(
            "[TRACE_WALL] stage=total event=end elapsed_ms={}",
            total_trace_started_at.elapsed().as_millis()
        );
        println!("[NATIVE_VIEWER] prebuild_only=true exit_after_prebuild=true");
        return Ok(());
    }
    println!(
        "[NATIVE_VIEWER] controls left-drag=rotate right-drag=pan wheel=zoom R=reset Esc=exit"
    );

    event_loop.run(move |event, event_loop_window_target| {
        event_loop_window_target.set_control_flow(ControlFlow::Poll);
        match event {
            Event::WindowEvent { event, window_id } if window_id == window.id() => match event {
                WindowEvent::CloseRequested => event_loop_window_target.exit(),
                WindowEvent::Resized(new_size) => state.resize(new_size),
                WindowEvent::ScaleFactorChanged { .. } => state.resize(window.inner_size()),
                WindowEvent::MouseInput {
                    state: element_state,
                    button,
                    ..
                } => {
                    state.process_mouse_input(element_state, button);
                }
                WindowEvent::CursorMoved { position, .. } => {
                    state.process_cursor_moved(position);
                }
                WindowEvent::CursorLeft { .. } => {
                    state.last_cursor_position = None;
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    state.process_scroll(delta);
                }
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed =>
                {
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::Escape) => event_loop_window_target.exit(),
                        PhysicalKey::Code(KeyCode::KeyR) => state.reset_camera(),
                        _ => {}
                    }
                }
                WindowEvent::RedrawRequested => {
                    match state.render(window.as_ref(), viewer_started_at) {
                        Ok(()) => {}
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            state.resize(state.size)
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            println!("[WGPU_INIT] surface_out_of_memory");
                            event_loop_window_target.exit();
                        }
                        Err(wgpu::SurfaceError::Timeout) => {
                            println!("[WGPU_INIT] surface_timeout");
                        }
                    }
                }
                _ => {}
            },
            Event::AboutToWait => {
                if let Some(auto_exit_after) = auto_exit_after
                    && viewer_started_at.elapsed() >= auto_exit_after
                {
                    println!(
                        "[NATIVE_VIEWER] auto_exit elapsed_seconds={} reason=auto_exit_seconds",
                        auto_exit_after.as_secs()
                    );
                    event_loop_window_target.exit();
                    return;
                }
                window.request_redraw();
            }
            _ => {}
        }
    })?;

    Ok(())
}
