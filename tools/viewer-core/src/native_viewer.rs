use std::borrow::Cow;
use std::collections::{HashMap,BTreeMap,HashSet,VecDeque};
use std::f32::consts::FRAC_PI_4;
use std::fs::File;
use std::io::{BufWriter,Read,Write};
use std::mem::size_of;
use std::path::{Path,PathBuf};
use std::sync::atomic::{AtomicBool,AtomicIsize,AtomicU64,AtomicUsize,Ordering};
use std::sync::mpsc::{self,Receiver,Sender,TryRecvError};
use std::sync::{Arc,Mutex,OnceLock};
use std::thread::{self,JoinHandle};
use std::time::{Duration,Instant};
use anyhow::{Context,Result,anyhow,bail,ensure};
use bytemuck::{Pod,Zeroable};
use glam::{Mat4,Vec2,Vec3};
use image::{Rgb,RgbImage,Rgba,RgbaImage};
use serde::{Serialize,Deserialize};
use serde_json::Value as JsonValue;
use sha2::{Digest,Sha256};
use viewer_launch_contract::{VIEWER_LAUNCH_ACK_PREFIX,VIEWER_LAUNCH_ACK_SCHEMA,VIEWER_LAUNCH_SCHEMA,ViewerLaunchAck,ViewerLaunchContract,ViewerSessionCommand,ViewerSessionCommandKind,ViewerSessionState,ViewerTask};
use wgpu::util::DeviceExt;
use winit::dpi::{PhysicalPosition,PhysicalSize};
use winit::event::{ElementState,Event,MouseButton,MouseScrollDelta,WindowEvent};
use winit::event_loop::{ControlFlow,EventLoop};
use winit::keyboard::{KeyCode,PhysicalKey};
#[cfg(target_os="windows")]
use winit::raw_window_handle::{HasWindowHandle,RawWindowHandle};
#[cfg(target_os="windows")]
use winit::platform::windows::WindowBuilderExtWindows;
use winit::window::WindowBuilder;
use crate::native_viewport::NativeViewportWindow;
#[cfg(target_os="windows")]
#[path="native_child.rs"]
mod native_child;
#[cfg(target_os="linux")]
#[path="native_child_x11.rs"]
mod native_child;
#[cfg(target_os="windows")]
#[path="first_block_timing.rs"]
mod first_block_timing;
use crate::backend_provenance::BackendProvenance;
use crate::build_mode::{ViewerBuildMode,compact_cache_v2_enabled,install_viewer_build_mode,writer_pipeline_enabled};
use crate::mesh::{ChunkKey,ChunkSceneIndex};
use crate::model::{CompactSurfaceOutput,MeshChunkOutput,MetadataOutput,TexturedVertexOutput};
use crate::process_io::read_process_file;
use crate::viewer_session::ViewerSessionWatcher;
use crate::runtime_paths;
const QUICK_MESH_CACHE_FORMAT: &str="native_fast_preview_cache_v1";
#[derive(Clone, Debug)]
struct ViewerShutdownRecord {
    reason: String,
    detail: String,
    exit_code: i32,
}

static VIEWER_SHUTDOWN_RECORD: OnceLock<Mutex<Option<ViewerShutdownRecord>>> = OnceLock::new();
static VIEWER_SHUTDOWN_EMITTED: AtomicBool = AtomicBool::new(false);
static VIEWER_SHUTDOWN_HWND: AtomicIsize = AtomicIsize::new(0);
static VIEWER_SHUTDOWN_GENERATION: AtomicU64 = AtomicU64::new(0);



fn viewer_shutdown_record() -> &'static Mutex<Option<ViewerShutdownRecord>> {
    VIEWER_SHUTDOWN_RECORD.get_or_init(|| Mutex::new(None))
}

fn register_viewer_shutdown_hwnd(hwnd: isize) {
    VIEWER_SHUTDOWN_HWND.store(hwnd, Ordering::SeqCst);
}

fn register_viewer_shutdown_generation(generation: u64) {
    VIEWER_SHUTDOWN_GENERATION.store(generation, Ordering::SeqCst);
}

fn record_viewer_shutdown(reason: &str, exit_code: i32, detail: impl Into<String>) {
    let next = ViewerShutdownRecord {
        reason: reason.to_owned(),
        detail: detail.into(),
        exit_code,
    };
    let mut record = viewer_shutdown_record()
        .lock()
        .expect("Viewer shutdown record mutex");
    if record
        .as_ref()
        .is_none_or(|current| current.exit_code == 0 && next.exit_code != 0)
    {
        *record = Some(next);
    }
}

fn emit_viewer_shutdown() {
    if VIEWER_SHUTDOWN_EMITTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let record = viewer_shutdown_record()
        .lock()
        .expect("Viewer shutdown record mutex")
        .clone()
        .unwrap_or_else(|| ViewerShutdownRecord {
            reason: "unknown".to_owned(),
            detail: "no shutdown reason was recorded".to_owned(),
            exit_code: 1,
        });
    let detail = serde_json::to_string(&record.detail)
        .unwrap_or_else(|_| "\"failed to serialize shutdown detail\"".to_owned());
    eprintln!(
        "[VIEWER_SHUTDOWN] reason={} pid={} hwnd={} generation={} exit_code={} detail={}",
        record.reason,
        std::process::id(),
        VIEWER_SHUTDOWN_HWND.load(Ordering::SeqCst),
        VIEWER_SHUTDOWN_GENERATION.load(Ordering::SeqCst),
        record.exit_code,
        detail,
    );
    let _ = std::io::stderr().flush();
}

pub fn install_viewer_shutdown_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        record_viewer_shutdown("panic", 101, panic_info.to_string());
        emit_viewer_shutdown();
        previous(panic_info);
    }));
}

pub fn finish_viewer_main(reason: &str, exit_code: i32, detail: impl Into<String>) {
    record_viewer_shutdown(reason, exit_code, detail);
    emit_viewer_shutdown();
}

/// Geometry cache version for textured full-mode chunk meshes. **Bump this
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
    vanilla_lighting: f32,
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
        // 原版 26.2 光照模式：顶点烘焙亮度（方向 shade x 4 顶点 AO，
        // mesh 阶段按 BlockModelLighter 算好存进顶点色 r），无
        // shadow mapping/SSAO/tone（原版没有这些），直接相乘输出。
        if (lighting.vanilla_lighting > 0.5) {
            return vec4<f32>(sampled.rgb * input.color.r, color.a);
        }
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
    vanilla_lighting: f32,
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
            vanilla_lighting: if vanilla_lighting_enabled() { 1.0 } else { 0.0 },
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

#[derive(Clone)]
struct ViewerArgs {
    launch_request_id: String,
    launch_contract_sha256: String,
    input: Option<PathBuf>,
    /// Frozen product geometry-lifetime contract received by the isolated
    /// Build Worker. This is intentionally unrelated to legacy
    /// `ViewerBuildMode`, which only selects an internal mesh implementation.
    chunk_size: u32,
    probe_scene: bool,
    prebuild_only: bool,
    build_cache_and_warm: bool,
    prepare_viewer_only: bool,
    persistent_popup_presentation: bool,
    initial_visible: bool,
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
    quick_host: Option<viewer_launch_contract::QuickPreviewHostWindow>,
    quick_gpu_context_only: bool,
    graphics_backend: viewer_launch_contract::GraphicsApiPreference,
    actual_graphics_backend: viewer_launch_contract::GraphicsApiPreference,
    quick_resource_report: Option<PathBuf>,
    quick_resource_decision: Option<PathBuf>,
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
}

impl ViewerDisplayMode {


    fn label(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::FastExperimental => "fast_experimental",
        }
    }
}

fn display_mode_for_launch_task(_: &ViewerTask) -> ViewerDisplayMode { ViewerDisplayMode::FastExperimental }

fn build_mode_for_launch_task(_: &ViewerTask) -> ViewerBuildMode { ViewerBuildMode::FastExperimental }


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
const NATIVE_CACHE_CHUNK_BINARY_MAGIC_REVISION5: &[u8; 8] = b"LPNC5\0\0\0";
static BLOCK_COLOR_CACHE: OnceLock<HashMap<String, [f32; 3]>> = OnceLock::new();

#[derive(Debug, Default)]
struct TraceCounters {
    worker_busy_ms: AtomicU64,
    worker_wait_ms: AtomicU64,
    worker_batches: AtomicUsize,
    worker_chunks: AtomicUsize,
    gpu_buffer_upload_requests: AtomicUsize,
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
    fn ShowWindow(hwnd: isize, n_cmd_show: i32) -> i32;
    fn IsWindowVisible(hwnd: isize) -> i32;
    fn PostMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
    fn SetWindowLongPtrW(hwnd: isize, n_index: i32, dw_new_long: isize) -> isize;
    fn GetWindowLongPtrW(hwnd: isize, n_index: i32) -> isize;
    fn GetWindowRect(hwnd: isize, lp_rect: *mut WinRect) -> i32;
    fn SetWindowPos(
        hwnd: isize,
        hwnd_insert_after: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
    fn SetFocus(hwnd: isize) -> isize;
    fn GetFocus() -> isize;
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

    texture_atlas: Option<RgbaImage>,
    /// Precomputed debug block-number labels (empty unless LBA_BLOCK_LABELS_DEBUG).
    label_anchors: Vec<LabelAnchor>,
}

impl ViewerScene {
    fn gpu_context_only() -> Self {
        Self { label: "GPU context".into(), bounds: SceneBounds { center: Vec3::ZERO, radius: 1.0 },
            palette_colors: Vec::new(), chunk_scene_index: None, bootstrap_meshes: Vec::new(),
            bootstrap_visible_first: 0, texture_atlas: None, label_anchors: Vec::new() }
    }

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

            texture_atlas: None,
            label_anchors: Vec::new(),
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
        let texture_atlas = None;
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
        let bootstrap_meshes = chunk_scene_index
                .build_chunk_meshes(&bootstrap_keys)?
                .into_iter()
                .filter_map(|chunk| {
                    PreparedChunkMesh::from_mesh_chunk(
                        chunk,
                        palette_colors.as_slice(),
                        chunk_scene_index.chunk_size(),
                    )
                })
                .collect::<Vec<_>>();
        println!(
            "[VIEWER_BOOTSTRAP] bootstrap_chunks={} visible_first={} target_chunks={} retain_chunks={}",
            bootstrap_meshes.len(),
            bootstrap_keys.len(),
            bootstrap_plan.visible_target_chunks,
            bootstrap_plan.retain_chunks
        );
        let label_anchors = if block_labels_enabled() {
            compute_case_label_anchors(&chunk_scene_index)
        } else {
            Vec::new()
        };
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

            texture_atlas,
            label_anchors,
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
    let default_hits = resolved_palette_colors
        .iter()
        .filter(|entry| entry.used_default)
        .count();
    let fallback_hits = resolved_palette_colors
        .iter()
        .filter(|entry| {
            entry
                .matched_key
                .as_deref()
                .is_some_and(|key| key.starts_with("fallback_material:"))
                && !entry.used_default
        })
        .count();
    let cache_hits = resolved_palette_colors
        .len()
        .saturating_sub(fallback_hits)
        .saturating_sub(default_hits);
    let palette_hits = resolved_palette_colors.len().saturating_sub(default_hits);
    let cached_entries = block_color_cache().len();
    println!("[VIEWER_COLOR] vertex_color_enabled=true");
    println!("[VIEWER_COLOR] shading_mode=vertex_color_flat");
    let color_source = "data/blockColorCache.json+material_fallback";
    println!(
        "[VIEWER_COLOR] display_mode={} color_source={} preview_chain={} native_chain={} palette_entries={} cache_entries={} cache_hits={} fallback_material_hits={} default_gray_hits={} palette_hits={} palette_misses={}",
        display_mode.label(),
        color_source,
        "block_color_cache_or_material_fallback",
        "palette_vertex_color",
        palette_colors.len(),
        cached_entries,
        cache_hits,
        fallback_hits,
        default_hits,
        palette_hits,
        default_hits
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
    fn gpu_upload_mesh(&self) -> Cow<'_, Self> {
        if self.compact_surfaces.is_empty() {
            return Cow::Borrowed(self);
        }
        // The cache reader expands compact faces, but a live fast-mode build
        // reaches the GPU without a disk round trip. Use the very same face
        // expansion here; retain the compact CPU/cache representation.
        let mut expanded = self.clone();
        expanded.vertices.reserve(self.compact_surfaces.len() * 4);
        expanded.indices.reserve(self.compact_surfaces.len() * 6);
        for surface in std::mem::take(&mut expanded.compact_surfaces) {
            append_compact_quad(
                &mut expanded.vertices,
                &mut expanded.indices,
                usize::from(surface.face_index),
                surface.min,
                surface.max,
                surface.color,
            )
            .expect("mesher compact surfaces must use one of the six cube faces");
        }
        Cow::Owned(expanded)
    }

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
            let mut vertices = mesh_chunk
                .textured_vertices
                .into_iter()
                .map(textured_vertex_from_output)
                .collect::<Vec<_>>();
            assign_textured_quad_normals(&mut vertices);
            vertices
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

/// 原版 26.2 光照默认开启；LBA_VANILLA_LIGHTING_OFF=1 回退自研
/// hemisphere/shadow/SSAO 光照。
fn vanilla_lighting_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("LBA_VANILLA_LIGHTING_OFF")
            .map(|value| {
                let normalized = value.trim().to_ascii_lowercase();
                normalized.is_empty() || normalized == "0" || normalized == "false"
            })
            .unwrap_or(true)
    })
}

fn textured_vertex_from_output(vertex: TexturedVertexOutput) -> GpuVertex {
    GpuVertex {
        position: vertex.position,
        // 顶点色 r=g=b= 烘焙亮度（原版方向 shade x AO），vanilla 光照
        // 模式在 shader 里与纹理相乘；自研光照模式忽略（读法线路径）。
        color: [vertex.light, vertex.light, vertex.light, 1.0],
        uv: vertex.uv,
        use_texture: 1.0,
        normal: default_vertex_normal(),
        emissive_tag: 0.0,
    }
}

/// Legacy textured quads are emitted as sequential groups of 4 vertices
/// (see `emit_textured_quad` in mesh.rs). They carry no per-face normal, so the
/// lighting pipeline previously treated every face as up-facing and could not
/// shade side/down faces differently. Derive a flat geometric normal per quad
/// from its first three vertices so the existing directional/side shading reads
/// correctly (e.g. the thin step of a pressure plate on a matching floor).
fn assign_textured_quad_normals(vertices: &mut [GpuVertex]) {
    for quad in vertices.chunks_mut(4) {
        if quad.len() < 3 {
            continue;
        }
        let a = Vec3::from(quad[0].position);
        let b = Vec3::from(quad[1].position);
        let c = Vec3::from(quad[2].position);
        let normal = (b - a).cross(c - a);
        let normal = if normal.length_squared() > 1e-12 {
            normal.normalize()
        } else {
            Vec3::from(default_vertex_normal())
        };
        let normal = normal.to_array();
        for vertex in quad.iter_mut() {
            vertex.normal = normal;
        }
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

fn prepared_mesh_uses_full_texture_data(mesh: &PreparedChunkMesh) -> bool {
    mesh.vertices.iter().any(|vertex| {
        vertex.use_texture > 0.5
            || vertex.uv != [0.0, 0.0]
            || vertex.emissive_tag != 0.0
            || vertex.color != rgb_to_rgba(vertex_rgb(vertex))
    })
}

fn write_prepared_cache_chunk_binary(
    path: &Path,
    mesh: &PreparedChunkMesh,
) -> std::io::Result<usize> {
    if prepared_mesh_uses_full_texture_data(mesh) {
        return write_prepared_cache_chunk_binary_v5(path, mesh);
    }
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

fn write_prepared_cache_chunk_binary_v5(
    path: &Path,
    mesh: &PreparedChunkMesh,
) -> std::io::Result<usize> {
    let mut writer = BufWriter::new(File::create(path)?);
    writer.write_all(NATIVE_CACHE_CHUNK_BINARY_MAGIC_REVISION5)?;
    writer.write_all(&mesh.key.cx.to_le_bytes())?;
    writer.write_all(&mesh.key.cy.to_le_bytes())?;
    writer.write_all(&mesh.key.cz.to_le_bytes())?;
    writer.write_all(&(mesh.vertices.len() as u32).to_le_bytes())?;
    writer.write_all(&(mesh.indices.len() as u32).to_le_bytes())?;
    writer.write_all(&(mesh.translucent_indices.len() as u32).to_le_bytes())?;
    for vertex in &mesh.vertices {
        for value in vertex.position {
            writer.write_all(&value.to_le_bytes())?;
        }
        for value in vertex.color {
            writer.write_all(&value.to_le_bytes())?;
        }
        for value in vertex.uv {
            writer.write_all(&value.to_le_bytes())?;
        }
        writer.write_all(&vertex.use_texture.to_le_bytes())?;
        for value in vertex.normal {
            writer.write_all(&value.to_le_bytes())?;
        }
        writer.write_all(&vertex.emissive_tag.to_le_bytes())?;
    }
    for index in &mesh.indices {
        writer.write_all(&index.to_le_bytes())?;
    }
    for index in &mesh.translucent_indices {
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
    let is_revision5 = magic == NATIVE_CACHE_CHUNK_BINARY_MAGIC_REVISION5;
    if !is_v2 && !is_v3 && !is_v4 && !is_revision5 {
        bail!("invalid binary cache chunk magic: {}", path.display());
    }
    let mut offset = NATIVE_CACHE_CHUNK_BINARY_MAGIC_V2.len();
    let cx = read_i32_le(&bytes, &mut offset)?;
    let cy = read_i32_le(&bytes, &mut offset)?;
    let cz = read_i32_le(&bytes, &mut offset)?;
    if is_revision5 {
        let vertex_count = read_u32_le(&bytes, &mut offset)? as usize;
        let index_count = read_u32_le(&bytes, &mut offset)? as usize;
        let translucent_index_count = read_u32_le(&bytes, &mut offset)? as usize;
        let mut vertices = Vec::with_capacity(vertex_count);
        for _ in 0..vertex_count {
            let position = [
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
            ];
            let color = [
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
            ];
            let uv = [
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
            ];
            let use_texture = read_f32_le(&bytes, &mut offset)?;
            let normal = [
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
                read_f32_le(&bytes, &mut offset)?,
            ];
            let emissive_tag = read_f32_le(&bytes, &mut offset)?;
            vertices.push(GpuVertex {
                position,
                color,
                uv,
                use_texture,
                normal,
                emissive_tag,
            });
        }
        let mut indices = Vec::with_capacity(index_count);
        for _ in 0..index_count {
            indices.push(read_u32_le(&bytes, &mut offset)?);
        }
        let mut translucent_indices = Vec::with_capacity(translucent_index_count);
        for _ in 0..translucent_index_count {
            translucent_indices.push(read_u32_le(&bytes, &mut offset)?);
        }
        let key = ChunkKey::new(cx, cy, cz);
        return Ok(PreparedChunkMesh {
            key,
            bounds: ChunkBounds::from_key(key, chunk_size),
            vertices,
            indices,
            translucent_indices,
            compact_surfaces: Vec::new(),
        });
    }
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

    worker_count: usize,
    trace: Arc<TraceCounters>,
    cancellation: Option<Arc<AtomicBool>>,
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

        let trace = trace.clone();
        let cancellation = cancellation.clone();
        let worker = thread::spawn(move || {
            let execution = std::panic::catch_unwind(move || {
                loop {
                    if cancellation.as_ref().is_some_and(|flag| flag.load(Ordering::Acquire)) { break; }
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

                    if cancellation.as_ref().is_some_and(|flag| flag.load(Ordering::Acquire)) { break; }
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
                    let build_result = scene_index.build_chunk_meshes(&requested_keys);
                    if cancellation.as_ref().is_some_and(|flag| flag.load(Ordering::Acquire)) { break; }
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
    retain_complete_source: bool,
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
    gpu_evicted_chunks: usize,
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
            retain_complete_source: false,
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
        if self.retain_complete_source {
            return EvictionStats { candidate_count: 0, evicted_count: 0,
                evicted_recent_count: 0, before_count, after_count: before_count,
                before_bytes, after_bytes: before_bytes, evicted_bytes: 0 };
        }
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

#[cfg(test)]
#[path = "quick_streaming_tests.rs"]
mod quick_streaming_tests;

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
            && max_vertices > 0
            && max_indices > 0
            // Chunk work is indivisible. An empty, enabled frame budget must
            // admit one oversized chunk or it is deferred forever (especially
            // in the smaller background preload budget). Later chunks still
            // obey the normal frame limits; no geometry is truncated.
            && (self.chunks == 0
                || (self.vertices.saturating_add(vertex_count) <= max_vertices
                    && self.indices.saturating_add(index_count) <= max_indices))
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

fn complete_projection_upload_order(keys: impl Iterator<Item = ChunkKey>, focus: ChunkKey) -> Vec<ChunkKey> {
    let mut order = keys.collect::<Vec<_>>();
    order.sort_by_key(|key| {
        let dx = i128::from(key.cx) - i128::from(focus.cx);
        let dy = i128::from(key.cy) - i128::from(focus.cy);
        let dz = i128::from(key.cz) - i128::from(focus.cz);
        (dx * dx + dy * dy + dz * dz, key.cy, key.cx, key.cz)
    });
    order
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
            let upload_order = complete_projection_upload_order(
                scene_index.chunk_entries().iter().map(|entry| entry.key), focus_chunk,
            );
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
    depth_pipeline: Arc<wgpu::RenderPipeline>,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    bind_group: wgpu::BindGroup,
}

fn create_shadow_map_resources(
    device: &wgpu::Device,
    config: ShadowMapConfig,
    shadow_bind_group_layout: &wgpu::BindGroupLayout,
    camera_bind_group_layout: &wgpu::BindGroupLayout,
    depth_shader: Option<&wgpu::ShaderModule>,
    existing_pipeline: Option<Arc<wgpu::RenderPipeline>>,
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
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
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
    let depth_pipeline = existing_pipeline.unwrap_or_else(|| Arc::new(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("native_viewer_shadow_depth_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: depth_shader.expect("initial shadow pipeline requires its existing shader"),
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
    })));
    ShadowMapResources {
        config,
        depth_texture,
        depth_view,
        depth_pipeline,
        camera_buffer,
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

/// Build the solid + translucent render pipelines used by both the interactive
/// window path (`ViewerState::new`) and the headless GPU offscreen preview.
/// Centralizing this guarantees the offscreen capture rasterizes with the exact
/// same state as the on-screen GPU pipeline — most importantly `cull_mode=Back`
/// and `front_face=Ccw`, which is the whole reason the offscreen path exists
/// (CPU preview does not back-face cull and therefore hides winding bugs).
fn create_viewer_render_pipelines(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    color_format: wgpu::TextureFormat,
) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
    let solid = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("native_viewer_pipeline_solid"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: "vs_main",
            buffers: &[GpuVertex::desc()],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
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
    let translucent = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("native_viewer_pipeline_translucent"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: "vs_main",
            buffers: &[GpuVertex::desc()],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
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
    (solid, translucent)
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

fn unpack_frame_pixels(mapped: &[u8], width: u32, height: u32, stride: u32, bgra: bool) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for row in mapped.chunks(stride as usize).take(height as usize) { pixels.extend_from_slice(&row[..width as usize * 4]); }
    if bgra { for pixel in pixels.chunks_exact_mut(4) { pixel.swap(0, 2); } }
    pixels
}

#[cfg(test)]
mod frame_capture_tests {
    use super::unpack_frame_pixels;
    #[test]
    fn actual_frame_readback_removes_padding_and_converts_bgra_without_flipping_rows() {
        let mapped = [1,2,3,255,4,5,6,128,99,99,99,99,7,8,9,255,10,11,12,0,99,99,99,99];
        assert_eq!(unpack_frame_pixels(&mapped, 2, 2, 12, true), [3,2,1,255,6,5,4,128,9,8,7,255,12,11,10,0]);
    }
}

fn save_gpu_frame(device: &wgpu::Device, buffer: &wgpu::Buffer, path: &Path, width: u32, height: u32, stride: u32, format: wgpu::TextureFormat) -> Result<()> {
    let slice = buffer.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| { let _ = tx.send(result); });
    device.poll(wgpu::Maintain::Wait);
    rx.recv().context("receive actual frame readback")?.context("map actual frame readback")?;
    let view = slice.get_mapped_range();
    let pixels = unpack_frame_pixels(&view, width, height, stride, matches!(format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb));
    drop(view); buffer.unmap();
    let temporary = path.with_extension("png.tmp");
    image::save_buffer_with_format(&temporary, &pixels, width, height, image::ColorType::Rgba8, image::ImageFormat::Png)?;
    std::fs::rename(&temporary, path)?;
    Ok(())
}

fn quick_surface_alpha_mode(embedded: bool, supported: &[wgpu::CompositeAlphaMode]) -> Result<wgpu::CompositeAlphaMode> {
    if embedded {
        ensure!(supported.contains(&wgpu::CompositeAlphaMode::PreMultiplied),
            "Selected DX12 composition surface does not support premultiplied alpha");
        Ok(wgpu::CompositeAlphaMode::PreMultiplied)
    } else {
        supported.first().copied().context("GPU surface exposes no alpha mode")
    }
}

fn quick_clear_color(alpha_mode: wgpu::CompositeAlphaMode) -> wgpu::Color {
    if alpha_mode == wgpu::CompositeAlphaMode::PreMultiplied {
        wgpu::Color::TRANSPARENT
    } else {
        wgpu::Color { r: 0.08, g: 0.09, b: 0.11, a: 1.0 }
    }
}

fn read_quick_resource_decision(path: &Path, request: &str) -> Result<Option<viewer_launch_contract::QuickResourceDecision>> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(metadata.is_file() && metadata.len() <= 4096, "Quick resource decision must be a bounded JSON file");
    let decision: viewer_launch_contract::QuickResourceDecision = serde_json::from_slice(&std::fs::read(path)?)?;
    decision.validate(request).map_err(|error| anyhow!(error))?;
    Ok(Some(decision))
}

fn quick_gpu_geometry_upper(mesh: &PreparedChunkMesh) -> usize {
    let mesh = mesh.gpu_upload_mesh();
    let aligned = |bytes: usize| if bytes == 0 { 0 } else { bytes.saturating_add(65_535) / 65_536 * 65_536 };
    aligned(mesh.vertices.len().saturating_mul(size_of::<GpuVertex>()))
        .saturating_add(aligned(mesh.indices.len().saturating_mul(size_of::<u32>())))
        .saturating_add(aligned(mesh.translucent_indices.len().saturating_mul(size_of::<u32>())))
}

fn quick_preload_allowed(pressure: &str, warning_confirmed: bool) -> bool {
    pressure == "normal" || (pressure == "warning" && warning_confirmed)
}

fn quick_resource_action(
    requested_preload: bool, pressure: &str, complete_estimate: bool,
    decision: Option<viewer_launch_contract::QuickResourceDecision>,
) -> Result<viewer_launch_contract::QuickResourceAction> {
    use viewer_launch_contract::QuickResourceAction as Action;
    let warning_confirmed = decision.as_ref().is_some_and(|choice| choice.warning_confirmed);
    let action = decision.map(|choice| choice.action).unwrap_or(if requested_preload { Action::Preload } else { Action::Realtime });
    ensure!(action != Action::Cancel, "Quick preview was cancelled before GPU allocation");
    ensure!(action != Action::Preload || (complete_estimate && quick_preload_allowed(pressure, warning_confirmed)),
        "快速模式尚未取得有效完整投影显存需求及预加载准入，请明确选择实时加载或取消。");
    Ok(action)
}

fn quick_upload_should_queue(deferred_preloads: &mut HashSet<ChunkKey>, key: ChunkKey, priority: QueuePriority) -> bool {
    if priority == QueuePriority::Target { deferred_preloads.remove(&key); return true; }
    !deferred_preloads.contains(&key)
}

fn quick_upload_refresh_demand(deferred_preloads: &mut HashSet<ChunkKey>, retry_at: &mut Option<Instant>, changed: bool) {
    if changed { deferred_preloads.clear(); *retry_at = None; }
}

fn prepare_quick_gpu_resources(
    adapter: &wgpu::Adapter, index: Option<&ChunkSceneIndex>, _atlas: Option<&RgbaImage>,
    size: PhysicalSize<u32>, bounds: SceneBounds, args: &mut ViewerArgs,
    retained_shadow_resolution: Option<u32>,
) -> Result<()> {
    if args.quick_host.is_none() { return Ok(()); }
    use crate::quick_resource_preflight::{QuickResourceInput, estimate_quick_resources, query_selected_adapter_budget};
    use viewer_launch_contract::QuickResourceAction as Action;
    if args.quick_gpu_context_only {
        let budget = query_selected_adapter_budget(adapter)?;
        let estimate = estimate_quick_resources(&QuickResourceInput { vertices_upper_bound: 0,
            vertex_stride_bytes: size_of::<GpuVertex>() as u64, indices_upper_bound: 0, geometry_buffer_count_upper_bound: 0,
            atlas_width: 1, atlas_height: 1, viewport_width: size.width.max(1), viewport_height: size.height.max(1),
            surface_buffer_count: if args.actual_graphics_backend == viewer_launch_contract::GraphicsApiPreference::Vulkan { 4 } else { 3 }, shadow_resolution: Some(LightingConfig::from_args(args.basic_lighting,
                args.basic_shadows, args.shadow_debug_view, args.shadow_force_test, bounds).shadow().resolution.max(1)),
            uniform_buffer_bytes: vec![size_of::<CameraUniform>() as u64, size_of::<CameraUniform>() as u64, size_of::<LightingUniform>() as u64] })?;
        eprintln!("[NOVA_GPU_INITIALIZATION] stage=static_budget_admitted budget_bytes={} current_usage_bytes={} pending_fixed_upper_bytes={} projection_reads=0 scene_mesh_builds=0", budget.budget_bytes, budget.current_usage_bytes, estimate.gpu_peak_upper_bytes);
        return Ok(());
    }
    let report_path = args.quick_resource_report.as_deref().context("Managed Fast launch is missing its resource report path")?;
    let decision_path = args.quick_resource_decision.as_deref().context("Managed Fast launch is missing its resource decision path")?;
    let source_bytes = std::fs::metadata(args.input.as_deref().context("Fast source is missing")?)?.len();
    let index = index.context("Fast resource estimate requires actual projection chunk metadata")?;
    let budget_result = query_selected_adapter_budget(adapter);
    let invalid_reason = budget_result.as_ref().err().map(|error| format!("{error:#}"));
    let report = serde_json::json!({"schema":"nova.quick.resource.v1", "requestId":args.launch_request_id,
        "sourceBytes":source_bytes, "preload":args.prebuild_before_show, "pressure":"unmeasured",
        "pressureBasisPoints":null, "budget":budget_result.as_ref().ok(),
        "estimate":null, "invalidReason":invalid_reason, "decisionRequired":args.prebuild_before_show,
        "fixedGpuResourcesAlreadyInLiveBudget":retained_shadow_resolution.is_some()});
    let temporary = report_path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec(&report)?)?;
    std::fs::rename(&temporary, report_path)?;
    // The index has chunk metadata, not full model/vertex/index demand. A
    // static-context estimate cannot admit a real projection. The Host handles
    // the nonblocking user choice before submitting ReplaceScene; this guard
    // rejects managed launch bypasses without stalling native Session events.
    let action = quick_resource_action(args.prebuild_before_show, "unmeasured", false,
        read_quick_resource_decision(decision_path, &args.launch_request_id)?)?;
    if action == Action::ReducedRealtime {
        // This is an explicit user option, never a silent budget downgrade.
        args.basic_shadows = false; args.shadow_force_test = false;
    }
    args.prebuild_before_show = action == Action::Preload;
    match budget_result {
        Ok(budget) => println!("[QUICK_RESOURCE] policy=complete_projection requested_preload={} actual_budget_bytes={} current_usage_bytes={} source_chunks={} artificial_residency_caps=false geometry_measurement=not_prepared renderer=legacy-wgpu",
            args.prebuild_before_show, budget.budget_bytes, budget.current_usage_bytes, index.chunk_count()),
        Err(error) => eprintln!("[QUICK_RESOURCE] policy=complete_projection actual_budget_unavailable={error:#} artificial_residency_caps=false allocation_failure_not_inferred=true"),
    }
    Ok(())
}

fn record_quick_gpu_failure(failure: &Mutex<Option<String>>, message: String) {
    let mut error = failure.lock().unwrap_or_else(|error| error.into_inner());
    if error.is_none() {
        eprintln!("[NOVA_QUICK_GPU_FAILURE] {message}");
        *error = Some(message);
    }
}

enum QuickChunkUploadAdmission {
    Admitted { evicted_chunks: usize },
    DeferredPreload,
    RetryBudget,
}

struct QuickVisibleSampleQuery {
    query: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    armed: bool,
    sample_attempts: u8,
    generation: u64,
    pending: Option<(Receiver<std::result::Result<(), wgpu::BufferAsyncError>>, u32, u64, u64)>,
    evidence: Option<(u32, u64)>,
}

struct QuickRedrawState {
    dirty: bool,
    next_progress_at: Option<Instant>,
    retry_after: Option<Instant>,
}

impl QuickVisibleSampleQuery {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            query: device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("fast_first_terrain_visible_samples"), ty: wgpu::QueryType::Occlusion, count: 1 }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor { label: Some("fast_first_samples_resolve"), size: 256, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false }),
            readback: device.create_buffer(&wgpu::BufferDescriptor { label: Some("fast_first_samples_readback"), size: 8, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false }),
            armed: false, sample_attempts: 0, generation: 0, pending: None, evidence: None,
        }
    }
    fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.armed = false; self.sample_attempts = 0; self.evidence = None;
        if self.pending.take().is_some() { self.readback.unmap(); }
    }
    fn arm(&mut self) { self.cancel(); self.armed = true; }
    fn retry(&mut self) {
        if !self.armed && self.pending.is_none() && self.sample_attempts < 8 {
            self.armed = true;
        }
    }
    fn should_query(&self) -> bool { self.armed && self.pending.is_none() && self.sample_attempts < 8 }
    fn submitted(&mut self, present_count: u32, scene_revision: u64) {
        self.sample_attempts = self.sample_attempts.saturating_add(1);
        let (tx, rx) = mpsc::channel();
        self.readback.slice(..).map_async(wgpu::MapMode::Read, move |result| { let _ = tx.send(result); });
        self.pending = Some((rx, present_count, scene_revision, self.generation));
    }
    fn poll(&mut self, device: &wgpu::Device, scene_revision: u64) -> Option<(u32, u64)> {
        if self.pending.is_some() { device.poll(wgpu::Maintain::Poll); }
        let result = self.pending.as_ref().and_then(|(rx, count, revision, generation)| match rx.try_recv() {
            Ok(result) => Some((result, *count, *revision, *generation)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some((Err(wgpu::BufferAsyncError), *count, *revision, *generation)),
        });
        if let Some((result, count, revision, generation)) = result {
            self.pending = None;
            if result.is_ok() {
                let bytes = self.readback.slice(..).get_mapped_range();
                let samples = u64::from_le_bytes(bytes[..8].try_into().expect("eight-byte occlusion result"));
                drop(bytes); self.readback.unmap();
                if generation == self.generation && revision == scene_revision && samples > 0 {
                    self.evidence = Some((count, revision)); self.armed = false;
                    eprintln!("[NOVA_FIRST_BLOCK] stage=fast_terrain_visible_samples samples={} sampled_present_count={} scene_revision={} bytes_read=8 asynchronous=true", samples, count, revision);
                }
            } else {
                self.readback.unmap(); self.armed = false;
                eprintln!("[NOVA_FIRST_BLOCK] stage=fast_sample_read_failed first_display=not_measured");
            }
            if self.sample_attempts == 8 && self.evidence.is_none() {
                self.armed = false;
                eprintln!("[NOVA_FIRST_BLOCK] stage=fast_visible_sample_limit attempts=8 first_display=not_measured");
            }
        }
        self.evidence.take()
    }
}

impl QuickRedrawState {
    fn new() -> Self { Self { dirty: true, next_progress_at: None, retry_after: None } }
    fn request(&mut self) { self.dirty = true; self.retry_after = None; }
    fn hide(&mut self) { self.dirty = false; self.next_progress_at = None; self.retry_after = None; }
    fn presented(&mut self, now: Instant) {
        self.dirty = false;
        self.retry_after = None;
        self.next_progress_at = Some(now + Duration::from_millis(16));
    }
    fn retry(&mut self, now: Instant) {
        self.dirty = true;
        self.retry_after = Some(now + Duration::from_millis(40));
    }
    fn needs_redraw(&self, now: Instant, presentable: bool, pending_work: bool,
        animated: bool, sample_retry: bool) -> bool {
        presentable && self.retry_after.is_none_or(|retry| now >= retry)
            && (self.dirty || ((pending_work || animated || sample_retry)
                && self.next_progress_at.is_none_or(|next| now >= next)))
    }
    fn poll_deadline(&self, now: Instant, pending_work: bool, awaiting_receipt: bool) -> Instant {
        if let Some(retry) = self.retry_after.filter(|retry| *retry > now) { return retry; }
        if pending_work {
            self.next_progress_at.filter(|next| *next > now)
                .unwrap_or(now + Duration::from_millis(16))
        } else {
            now + Duration::from_millis(if awaiting_receipt { 16 } else { 40 })
        }
    }
}

#[cfg(target_os = "windows")]
struct QuickGpuInitialization {
    surface: wgpu::Surface<'static>,
    composition: Option<crate::windows_composition::WindowsComposition>,
    vulkan_composition: Option<crate::windows_composition::VulkanComposition>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

#[cfg(target_os = "windows")]
fn quick_backend_attempts(preference: viewer_launch_contract::GraphicsApiPreference) -> &'static [viewer_launch_contract::GraphicsApiPreference] {
    use viewer_launch_contract::GraphicsApiPreference as Backend;
    match preference { Backend::Auto => &[Backend::Dx12, Backend::Vulkan], Backend::Dx12 => &[Backend::Dx12], Backend::Vulkan => &[Backend::Vulkan] }
}

#[cfg(all(test, target_os = "windows"))]
#[test]
fn quick_native_backend_manual_selection_never_silently_falls_back() {
    use viewer_launch_contract::GraphicsApiPreference as Backend;
    assert_eq!(quick_backend_attempts(Backend::Auto), &[Backend::Dx12, Backend::Vulkan]);
    assert_eq!(quick_backend_attempts(Backend::Dx12), &[Backend::Dx12]);
    assert_eq!(quick_backend_attempts(Backend::Vulkan), &[Backend::Vulkan]);
}

#[cfg(target_os = "windows")]
async fn initialize_quick_gpu(window: Arc<NativeViewportWindow>, index: Option<&ChunkSceneIndex>,
    atlas: Option<&RgbaImage>, bounds: SceneBounds, args: &mut ViewerArgs) -> Result<QuickGpuInitialization> {
    use viewer_launch_contract::GraphicsApiPreference as Backend;
    let embedded = args.quick_host.is_some();
    let size = window.inner_size();
    let preference = args.graphics_backend;
    let mut failures = Vec::new();
    for &backend in quick_backend_attempts(preference) {
        let mut resource_stop = false;
        let started = Instant::now();
        let attempt = async {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: match backend {
                Backend::Dx12 => wgpu::Backends::DX12, Backend::Vulkan => wgpu::Backends::VULKAN, Backend::Auto => unreachable!(),
            }, ..Default::default() });
            let composition = if embedded && backend == Backend::Dx12 {
                Some(crate::windows_composition::WindowsComposition::new(window_hwnd(window.as_ref())?)?)
            } else { None };
            let surface = if let Some(composition) = composition.as_ref() {
                unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CompositionVisual(composition.visual_ptr())) }
                    .context("Create DX12 native composition surface")?
            } else { instance.create_surface(window.clone()).context("Create native wgpu selection surface")? };
            let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions { compatible_surface: Some(&surface),
                power_preference: wgpu::PowerPreference::HighPerformance, force_fallback_adapter: false }).await
                .context("No adapter supports this backend and native surface")?;
            args.actual_graphics_backend = backend;
            if let Err(error) = prepare_quick_gpu_resources(&adapter, index, atlas, size, bounds, args, None) {
                resource_stop = true; return Err(error.context("Fast GPU resource admission"));
            }
            let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor { label: Some("native_viewer_device"),
                required_features: wgpu::Features::empty(), required_limits: wgpu::Limits::default() }, None).await
                .context("Create actual backend device")?;
            let capabilities = surface.get_capabilities(&adapter);
            let format = capabilities.formats.iter().copied().find(wgpu::TextureFormat::is_srgb)
                .context("Native Fast surface has no sRGB format")?;
            let config = wgpu::SurfaceConfiguration { format, width: size.width.max(1), height: size.height.max(1),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | if embedded && backend == Backend::Vulkan {
                    // This is Fast's real offscreen color texture, not the
                    // unconfigured Win32 selection surface.
                    wgpu::TextureUsages::COPY_SRC
                } else { capabilities.usages & wgpu::TextureUsages::COPY_SRC },
                present_mode: if capabilities.present_modes.contains(&wgpu::PresentMode::Mailbox) { wgpu::PresentMode::Mailbox } else { wgpu::PresentMode::Fifo },
                alpha_mode: if embedded && backend == Backend::Vulkan { wgpu::CompositeAlphaMode::PreMultiplied }
                    else { quick_surface_alpha_mode(embedded, &capabilities.alpha_modes)? },
                view_formats: vec![], desired_maximum_frame_latency: 2 };
            device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
            device.push_error_scope(wgpu::ErrorFilter::Validation);
            let vulkan_composition = if embedded && backend == Backend::Vulkan {
                Some(crate::windows_composition::VulkanComposition::new(window_hwnd(window.as_ref())?, &adapter, &device,
                    config.width, config.height, config.format)?)
            } else { surface.configure(&device, &config); None };
            if let Some(error) = device.pop_error_scope().await { bail!("Native Fast presentation validation failed: {error}"); }
            if let Some(error) = device.pop_error_scope().await { bail!("Native Fast presentation allocation failed: {error}"); }
            if let Some(composition) = composition.as_ref() { composition.resize(config.width, config.height, window.scale_factor())?; composition.commit()?; }
            let info = adapter.get_info();
            println!("[NOVA_GPU_BACKEND] mode=fast requested={preference:?} selected={backend:?} adapter={:?} driver={:?} driver_info={:?} native_embedded={} init_elapsed_ms={:.3}",
                info.name, info.driver, info.driver_info, embedded, started.elapsed().as_secs_f64()*1000.0);
            Ok(QuickGpuInitialization { surface, composition, vulkan_composition, adapter, device, queue, config })
        }.await;
        match attempt {
            Ok(initialized) => return Ok(initialized),
            Err(error) => {
                eprintln!("[NOVA_GPU_BACKEND] mode=fast requested={preference:?} rejected={backend:?} reason={error:#} retry_other_backend={}", preference == Backend::Auto && !resource_stop);
                if resource_stop || preference != Backend::Auto { return Err(error); }
                failures.push(format!("{backend:?}: {error:#}"));
            }
        }
    }
    bail!("No native Fast backend completed initialization: {}", failures.join("; "))
}

struct ViewerState {
    surface: Option<wgpu::Surface<'static>>,
    #[cfg(target_os = "macos")]
    mac_composition: Option<crate::mac_fast_composition::MacFastComposition>,
    // Surface owns the DXGI swapchain and must drop before its visual/target.
    #[cfg(target_os = "windows")]
    composition: Option<crate::windows_composition::WindowsComposition>,
    #[cfg(target_os = "windows")]
    vulkan_composition: Option<crate::windows_composition::VulkanComposition>,
    composition_dpi: f64,
    presentation_error: Option<String>,
    quick_gpu_failure: Arc<Mutex<Option<String>>>,
    quick_adapter: Option<wgpu::Adapter>,
    quick_visible_sample_query: QuickVisibleSampleQuery,
    quick_scene_revision: u64,
    quick_decision_path: Option<PathBuf>,
    quick_request_id: String,
    show_after_prebuild: bool,
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
    lighting_buffer: wgpu::Buffer,
    shadow_resources: ShadowMapResources,
    atlas_bind_group: wgpu::BindGroup,
    depth_texture: DepthTexture,
    label_overlay: Option<LabelOverlay>,
    label_anchors: Vec<LabelAnchor>,
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
    build_cancellation: Arc<AtomicBool>,
    retired_build_workers: Vec<JoinHandle<()>>,
    quick_source_identity: Option<(PathBuf, u64, Option<std::time::SystemTime>)>,
    build_worker_count: usize,
    trace: Arc<TraceCounters>,
    build_budget: BuildBudget,
    pending_upload_queue: VecDeque<PendingChunkUpload>,
    pending_upload_keys: HashSet<ChunkKey>,
    deferred_quick_preloads: HashSet<ChunkKey>,
    quick_upload_retry_at: Option<Instant>,
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
    embedded_overlay: bool,
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
    capture_output: Option<PathBuf>,
    capture_error: Option<String>,
    // CompositionVisual surfaces do not own the child HWND. Keep it alive until
    // after every GPU surface and composition COM reference has been dropped.
    _window: Arc<NativeViewportWindow>,
}

impl ViewerState {
    async fn new(
        window: Arc<NativeViewportWindow>,
        scene: ViewerScene,
        args: &ViewerArgs,
    ) -> Result<Self> {
        let mut effective_args = args.clone();
        let args = &mut effective_args;
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
            texture_atlas,
            label_anchors: scene_label_anchors,
        } = scene;

        let embedded_composition = args.quick_host.is_some();
        #[cfg(target_os = "windows")]
        let QuickGpuInitialization { surface, composition, vulkan_composition, adapter, device, queue, config } =
            initialize_quick_gpu(window.clone(), scene_chunk_scene_index.as_ref(), texture_atlas.as_ref(),
                scene_bounds, args).await?;
        #[cfg(not(target_os = "windows"))]
        let (surface, adapter, device, queue, config) = {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: if cfg!(target_os = "macos") { wgpu::Backends::METAL } else { wgpu::Backends::VULKAN },
                ..Default::default()
            });
            #[cfg(target_os = "macos")]
            let shared = window.shared().is_some();
            #[cfg(not(target_os = "macos"))]
            let shared = false;
            let surface = if shared { None } else {
                Some(instance.create_surface(window.clone()).context("create native GPU surface")?)
            };
            let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface.as_ref(), force_fallback_adapter: false,
            }).await.context("no GPU supports the native viewport backend")?;
            prepare_quick_gpu_resources(&adapter, scene_chunk_scene_index.as_ref(), texture_atlas.as_ref(), size, scene_bounds, args, None)?;
            let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("native_viewer_device"), required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
            }, None).await.context("create native GPU device")?;
            let config = if let Some(surface) = surface.as_ref() {
                let capabilities = surface.get_capabilities(&adapter);
                wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | (capabilities.usages & wgpu::TextureUsages::COPY_SRC),
                    format: capabilities.formats.iter().copied().find(wgpu::TextureFormat::is_srgb).context("native surface has no sRGB color format")?,
                    width: size.width.max(1), height: size.height.max(1),
                    present_mode: wgpu::PresentMode::Fifo,
                    alpha_mode: quick_surface_alpha_mode(false, &capabilities.alpha_modes)?,
                    view_formats: vec![], desired_maximum_frame_latency: 2,
                }
            } else { wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                format: wgpu::TextureFormat::Bgra8UnormSrgb,
                width: size.width.max(1), height: size.height.max(1),
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
                view_formats: vec![], desired_maximum_frame_latency: 2,
            }};
            if let Some(surface) = surface.as_ref() { surface.configure(&device, &config); }
            println!("[NOVA_GPU_BACKEND] mode=fast platform={} selected={:?} adapter={:?} driver={:?} native_embedded={}",
                std::env::consts::OS, adapter.get_info().backend, adapter.get_info().name, adapter.get_info().driver, embedded_composition);
            (surface, adapter, device, queue, config)
        };
        #[cfg(target_os = "windows")]
        let surface = Some(surface);
        #[cfg(target_os = "macos")]
        let mac_composition = if let Some(viewport) = window.shared() {
            let budget = crate::quick_resource_preflight::query_selected_adapter_budget(&adapter)?;
            let available = budget.budget_bytes.saturating_mul(95) / 100;
            let available = available.saturating_sub(budget.current_usage_bytes);
            let color_bytes = u64::from(config.width).checked_mul(u64::from(config.height))
                .and_then(|pixels| pixels.checked_mul(4 * 6)).context("native presentation size overflow")?;
            ensure!(available >= color_bytes, "native GPU presentation does not fit the current selected-device budget");
            viewport.set_presentation_budget(available);
            Some(crate::mac_fast_composition::MacFastComposition::new(
                Arc::clone(viewport), &adapter, &device, config.width, config.height, config.format)?)
        } else { None };
        let quick_gpu_failure = Arc::new(Mutex::new(None));
        if embedded_composition {
            let failure = Arc::clone(&quick_gpu_failure);
            device.on_uncaptured_error(Box::new(move |error: wgpu::Error| {
                record_quick_gpu_failure(&failure, format!("快速预览显卡执行失败：{error}。请关闭其他占用显卡资源的程序后重试。"));
            }));
            let failure = Arc::clone(&quick_gpu_failure);
            device.set_device_lost_callback(move |reason, detail| {
                if matches!(reason, wgpu::DeviceLostReason::Unknown) {
                    record_quick_gpu_failure(&failure, format!("快速预览显卡设备失去响应：{detail}。请关闭其他占用显卡资源的程序后重试。"));
                }
            });
        }
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
            Some(&shadow_depth_shader),
            None,
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
        // Shared with the GPU offscreen preview path so the headless render
        // reflects the exact same rasterizer state (cull_mode=Back, Ccw winding,
        // blend modes). Do not inline-duplicate these — see
        // create_viewer_render_pipelines.
        let (render_pipeline_solid, render_pipeline_translucent) = create_viewer_render_pipelines(
            &device,
            &shader,
            &render_pipeline_layout,
            config.format,
        );

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
        let build_cancellation = Arc::new(AtomicBool::new(false));
        let (build_request_tx, build_result_rx, build_workers, build_worker_count) =
            if let Some(scene_index) = chunk_scene_index.as_ref() {
                let (request_tx, result_rx, workers) = spawn_build_worker_pool(
                    scene_index.clone(),
                    Arc::new(palette_colors.clone()),
                    args.build_workers,
                    trace.clone(),
                    Some(Arc::clone(&build_cancellation)),
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
        let label_overlay = if block_labels_enabled() && !scene_label_anchors.is_empty() {
            Some(LabelOverlay::new(
                &device,
                config.format,
                wgpu::TextureFormat::Depth24Plus,
            ))
        } else {
            None
        };
        let quick_visible_sample_query = QuickVisibleSampleQuery::new(&device);
        let mut state = Self {
            surface,
            #[cfg(target_os = "windows")]
            composition,
            #[cfg(target_os = "windows")]
            vulkan_composition,
            #[cfg(target_os = "macos")]
            mac_composition,
            composition_dpi: window.scale_factor(),
            presentation_error: None,
            quick_gpu_failure,
            quick_adapter: args.quick_host.is_some().then_some(adapter),
            quick_visible_sample_query, quick_scene_revision: 0,
            quick_decision_path: args.quick_resource_decision.clone(),
            quick_request_id: args.launch_request_id.clone(),
            show_after_prebuild: args.initial_visible,
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
            lighting_buffer,
            shadow_resources,
            atlas_bind_group,
            depth_texture,
            label_overlay,
            label_anchors: scene_label_anchors,
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
            build_cancellation,
            retired_build_workers: Vec::new(),
            quick_source_identity: args.input.as_ref().and_then(|path| std::fs::metadata(path).ok().map(|meta| (path.clone(), meta.len(), meta.modified().ok()))),
            build_worker_count,
            trace,
            build_budget: BuildBudget::default(),
            pending_upload_queue: VecDeque::new(),
            pending_upload_keys: HashSet::new(),
            deferred_quick_preloads: HashSet::new(),
            quick_upload_retry_at: None,
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
            background_fill_enabled: args.prebuild_before_show || args.quick_host.is_some(),
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
            embedded_overlay: args.embed_parent_hwnd.is_some(),
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
            capture_output: None,
            capture_error: None,
            _window: window,
        };
        state.cpu_mesh_cache.retain_complete_source = args.quick_host.is_some();
        state.install_bootstrap_meshes(bootstrap_meshes);
        println!(
            "[VIEWER_CACHE] config cpu_max_chunks={} cpu_max_bytes={} resident_max_chunks={} resident_max_bytes={} stress_limits={} artificial_caps_enforced={}",
            args.cpu_cache_max_chunks,
            args.cpu_cache_max_bytes,
            args.resident_max_chunks,
            args.resident_max_bytes,
            args.stress_limits,
            !args.quick_host.is_some()
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
        if let Some(error) = state.quick_gpu_failure.lock().unwrap_or_else(|error| error.into_inner()).take() { bail!(error); }
        Ok(state)
    }

    fn quick_input_is_current(&self, args: &ViewerArgs) -> bool {
        self.quick_source_identity.is_some() && args.input.as_ref().and_then(|path| std::fs::metadata(path).ok().map(|meta|
            (path.clone(), meta.len(), meta.modified().ok()))) == self.quick_source_identity
    }

    fn take_quick_gpu_failure(&self) -> Option<String> {
        self.quick_gpu_failure.lock().unwrap_or_else(|error| error.into_inner()).take()
    }

    /// Replace only projection-owned state on the same native composition
    /// surface, device, queue and pipelines. A distinct result receiver plus
    /// cancellation prevents old workers from publishing into the new scene.
    fn replace_quick_scene(&mut self, scene: Option<ViewerScene>, args: &mut ViewerArgs) -> Result<()> {
        ensure!(args.display_mode == ViewerDisplayMode::FastExperimental && args.quick_host.is_some(), "Only the independent embedded Fast session can reuse this GPU context");
        let started = Instant::now();
        self.quick_visible_sample_query.cancel(); self.quick_scene_revision = self.quick_scene_revision.wrapping_add(1);
        self.drag_mode = None; self.last_cursor_position = None;
        self.capture_output = None; self.capture_error = None;
        self.resident_chunks.clear(); self.pending_upload_queue.clear(); self.pending_upload_keys.clear();
        // Retire already submitted references before querying real live usage;
        // no old/new scene double residency and no fixed timeout sleep.
        let retirement = self.queue.submit([]);
        self.device.poll(wgpu::Maintain::WaitForSubmissionIndex(retirement));
        let mut bootstrap = Vec::new();
        if let Some(scene) = scene {
            ensure!(scene.texture_atlas.is_none(), "Fast scene replacement cannot carry a material atlas");
            self.build_cancellation.store(true, Ordering::Release);
            self.build_request_tx.take();
            self.retired_build_workers.append(&mut self.build_workers);
            self.pending_build_queue.clear(); self.pending_build_keys.clear(); self.in_flight_build_keys.clear();
            self.cpu_mesh_cache = ChunkMeshCpuCache::new(args.cpu_cache_max_chunks, args.cpu_cache_max_bytes);
            self.cpu_mesh_cache.retain_complete_source = true;
            self.prebuild_empty_mesh_keys.clear(); self.sticky_target_keys.clear(); self.warm_cache_keys.clear();
            self.scene_bounds = scene.bounds;
            self.chunk_scene_index = scene.chunk_scene_index.map(Arc::new);
            self.bootstrap_visible_first = scene.bootstrap_visible_first;
            self.label_anchors = scene.label_anchors;
            bootstrap = scene.bootstrap_meshes;
            self.build_cancellation = Arc::new(AtomicBool::new(false));
            let index = self.chunk_scene_index.as_ref().context("Fast replacement has no actual scene index")?;
            let (tx, rx, workers) = spawn_build_worker_pool(Arc::clone(index), Arc::new(scene.palette_colors),
                args.build_workers, Arc::clone(&self.trace), Some(Arc::clone(&self.build_cancellation)));
            self.build_request_tx = Some(tx); self.build_result_rx = rx; self.build_workers = workers;
            self.build_worker_count = args.build_workers;
            self.max_in_flight_builds = self.build_worker_count.max(1) * 16;
            self.quick_source_identity = args.input.as_ref().and_then(|path| std::fs::metadata(path).ok().map(|meta|
                (path.clone(), meta.len(), meta.modified().ok())));
        }
        self.reap_retired_build_workers();
        prepare_quick_gpu_resources(self.quick_adapter.as_ref().context("Fast session lost its actual selected GPU adapter")?,
            self.chunk_scene_index.as_deref(), None, self.size, self.scene_bounds, args, Some(self.shadow_resources.config.resolution.max(1)))?;
        let lighting = LightingConfig::from_args(args.basic_lighting, args.basic_shadows, args.shadow_debug_view, args.shadow_force_test, self.scene_bounds);
        self.queue.write_buffer(&self.lighting_buffer, 0, bytemuck::bytes_of(&LightingUniform::from_config(lighting)));
        let shadow = lighting.shadow();
        if shadow.resolution.max(1) != self.shadow_resources.config.resolution.max(1) {
            self.shadow_resources = create_shadow_map_resources(&self.device, shadow,
                &self.render_pipeline_solid.get_bind_group_layout(3),
                &self.shadow_resources.depth_pipeline.get_bind_group_layout(0), None,
                Some(Arc::clone(&self.shadow_resources.depth_pipeline)));
        } else {
            let camera = CameraUniform { view_proj: shadow.light_camera.map(|camera| camera.view_proj).unwrap_or(Mat4::IDENTITY).to_cols_array_2d(),
                camera_position: [0.0;4] };
            self.queue.write_buffer(&self.shadow_resources.camera_buffer, 0, bytemuck::bytes_of(&camera));
            self.shadow_resources.config = shadow;
        }
        self.camera = OrbitCamera::from_bounds(self.scene_bounds, self.config.width, self.config.height);
        self.current_target_keys.clear(); self.current_preload_keys.clear(); self.current_retain_keys.clear();
        self.deferred_quick_preloads.clear(); self.quick_upload_retry_at = None;
        self.last_focus_chunk = None; self.last_target_radius_chunks = None;
        self.prebuild_finished_keys.clear(); self.prebuild_cache_written_keys.clear(); self.prebuild_manifest_chunks.clear();
        self.prebuild_before_show = args.prebuild_before_show; self.prebuild_cache_only = false;
        self.prebuild_completed = !args.prebuild_before_show; self.background_fill_enabled = true;
        self.show_after_prebuild = false; self.max_resident_chunks = args.resident_max_chunks; self.max_resident_bytes = args.resident_max_bytes;
        self.quick_request_id = args.launch_request_id.clone(); self.quick_decision_path = args.quick_resource_decision.clone();
        self.total_scene_chunks = self.chunk_scene_index.as_ref().map_or(0, |index| index.chunk_count());
        self.prebuild_eta_seconds = None; self.prebuild_eta_sample_resident = 0;
        self.prebuild_eta_sample_time = Instant::now(); self.prebuild_last_eta_update = Instant::now();
        self.prebuild_last_completed_count = 0; self.prebuild_last_completed_at = Instant::now();
        self.residency_dirty = true;
        self.install_bootstrap_meshes(bootstrap);
        self.sync_chunk_residency()?;
        let _ = self.process_pending_uploads();
        if let Some(error) = self.take_quick_gpu_failure() { bail!(error); }
        if let Some(error) = self.presentation_error.take() { bail!(error); }
        eprintln!("[NOVA_RENDER_PHASE] stage=fast_scene_installed elapsed_ms={:.3} existing_gpu_context=true new_gpu_devices=0 new_composition_surfaces=0 scene_chunks={} resident_chunks={} preload={} old_worker_receivers_reused=false", started.elapsed().as_secs_f64()*1000.0,
            self.total_scene_chunks, self.resident_chunks.len(), self.prebuild_before_show);
        Ok(())
    }

    fn reap_retired_build_workers(&mut self) {
        let mut index = 0;
        while index < self.retired_build_workers.len() {
            if self.retired_build_workers[index].is_finished() {
                let _ = self.retired_build_workers.swap_remove(index).join();
            } else { index += 1; }
        }
    }

    fn install_bootstrap_meshes(&mut self, bootstrap_meshes: Vec<PreparedChunkMesh>) {
        let bootstrap_count = bootstrap_meshes.len();
        for prepared in bootstrap_meshes {
            self.cpu_mesh_cache
                .insert(prepared.clone(), self.frame_index);
            if !self.prebuild_cache_only && matches!(self.admit_quick_chunk_upload(&prepared, QueuePriority::Target),
                QuickChunkUploadAdmission::Admitted { .. }) {
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

    fn finalize_prebuild(&mut self, window: &NativeViewportWindow, viewer_started_at: Instant) {
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
        if self.show_after_prebuild { window.set_visible(true); }
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
            "[NATIVE_VIEWER] prebuild_completed elapsed_ms={} ready_file={} visible_requested={}",
            viewer_started_at.elapsed().as_millis(),
            self.ready_file
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "none".to_string()),
            self.show_after_prebuild
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
        window: &NativeViewportWindow,
        viewer_started_at: Instant,
    ) -> Result<()> {
        if !self.prebuild_before_show || self.prebuild_completed {
            return Ok(());
        }
        let mut last_log = Instant::now();
        loop {
            if let Some(error) = self.take_quick_gpu_failure() { bail!(error); }
            if let Some(error) = self.presentation_error.take() { bail!(error); }
            if let Some(path) = self.quick_decision_path.as_deref() {
                if read_quick_resource_decision(path, &self.quick_request_id)?.is_some_and(|decision| decision.action == viewer_launch_contract::QuickResourceAction::Cancel) {
                    bail!("快速预加载已取消");
                }
            }
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
        self.trace.gpu_buffer_upload_requests.fetch_add(1, Ordering::Relaxed);
        let gpu_upper = self.quick_adapter.as_ref().map(|_| quick_gpu_geometry_upper(prepared));
        let prepared = prepared.gpu_upload_mesh();
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
            estimated_bytes: gpu_upper.unwrap_or_else(|| prepared.estimated_bytes()),
        }
    }

    fn admit_quick_chunk_upload(&mut self, _prepared: &PreparedChunkMesh, _priority: QueuePriority) -> QuickChunkUploadAdmission {
        // The complete Quick source stays resident. Actual GPU failures are
        // reported by the device error handler; no cache rank retires a chunk.
        QuickChunkUploadAdmission::Admitted { evicted_chunks: 0 }
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
        if !quick_upload_should_queue(&mut self.deferred_quick_preloads, key, priority) { return false; }
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
                    };
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
        if self.quick_upload_retry_at.is_some_and(|at| Instant::now() < at) {
            return QueueProcessingStats::default();
        }
        self.quick_upload_retry_at = None;
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
        let mut uploads = target_queue.into_iter().chain(preload_queue);
        while let Some(mut pending) = uploads.next() {
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
            match self.admit_quick_chunk_upload(&prepared, priority) {
                QuickChunkUploadAdmission::Admitted { evicted_chunks } => stats.gpu_evicted_chunks += evicted_chunks,
                QuickChunkUploadAdmission::DeferredPreload => {
                    self.deferred_quick_preloads.insert(pending.key);
                    stats.deferred_preload_chunks += 1;
                    continue;
                }
                QuickChunkUploadAdmission::RetryBudget => {
                    self.quick_upload_retry_at = Some(Instant::now() + Duration::from_millis(250));
                    next_queue.push_back(pending);
                    next_queue.extend(uploads);
                    break;
                }
            }
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
        if self.prebuild_before_show || self.quick_adapter.is_some() {
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
        if self.prebuild_before_show || self.quick_adapter.is_some() {
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
            self.prebuild_before_show || self.quick_adapter.is_some(),
        );
        let indexed_chunks = scene_index.chunk_count();
        let previous_targets = std::mem::replace(&mut self.current_target_keys, plan.target_keys.clone());
        let previous_preloads = std::mem::replace(&mut self.current_preload_keys, plan.preload_keys.clone());
        self.current_retain_keys = plan.retain_keys.clone();
        self.refresh_sticky_target_keys(&plan);
        quick_upload_refresh_demand(&mut self.deferred_quick_preloads, &mut self.quick_upload_retry_at,
            previous_targets != self.current_target_keys || previous_preloads != self.current_preload_keys
                || self.last_focus_chunk != Some(plan.focus_chunk));

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
        if self.embedded_overlay {
            "embedded_overlay"
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

    #[cfg(target_os = "windows")]
    fn native_present_statistics(&mut self) -> Option<(u32, Option<(u32, i64)>)> {
        if let Some(composition) = self.vulkan_composition.as_ref() {
            composition.present_count().map(|count| (count, composition.displayed()))
        } else { unsafe { self.surface.as_mut().unwrap().as_hal::<wgpu::hal::api::Dx12, _, _>(|surface|
            surface.and_then(|surface| surface.native_present_statistics())) }.flatten() }
    }

    fn resize(&mut self, new_size: PhysicalSize<u32>) {
        #[cfg(target_os = "windows")]
        if let Some(composition) = self.composition.as_ref() {
            if let Err(error) = composition.resize(new_size.width, new_size.height, self.composition_dpi) {
                self.presentation_error = Some(format!("native composition resize failed: {error:#}"));
                return;
            }
        }
        #[cfg(target_os = "windows")]
        if let Some(composition) = self.vulkan_composition.as_ref() {
            if let Err(error) = composition.resize_clip(new_size.width, new_size.height, self.composition_dpi) {
                self.presentation_error = Some(format!("Vulkan native composition clip failed: {error:#}"));
                return;
            }
            // Position/focus/page restoration does not recreate GPU resources.
            if new_size == self.size { return; }
        }
        if new_size.width == 0 || new_size.height == 0 {
            self.size = new_size;
            return;
        }
        self.size = new_size;
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.camera.update_aspect(new_size.width, new_size.height);
        #[cfg(target_os = "macos")]
        if let Some(composition) = self.mac_composition.as_mut() {
            if let Err(error) = composition.resize(new_size.width, new_size.height) {
                record_quick_gpu_failure(&self.quick_gpu_failure, format!("native Metal resize failed: {error:#}"));
            }
        }
        #[cfg(target_os = "windows")]
        if self.vulkan_composition.is_some() {
            self.device.poll(wgpu::Maintain::Wait);
            self.vulkan_composition = None;
            // Retire the dropped presentation aliases before querying current
            // residency for the replacement extent; keep scene resources alive.
            self.device.poll(wgpu::Maintain::Wait);
            let recreated = self.quick_adapter.as_ref().context("Vulkan native viewport lost its selected adapter")
                .and_then(|adapter| crate::windows_composition::VulkanComposition::new(window_hwnd(self._window.as_ref())?,
                    adapter, &self.device, self.config.width, self.config.height, self.config.format));
            match recreated {
                Ok(composition) => self.vulkan_composition = Some(composition),
                Err(error) => { self.presentation_error = Some(format!("Vulkan native viewport resize failed: {error:#}")); return; }
            }
        } else { if let Some(surface) = self.surface.as_ref() { surface.configure(&self.device, &self.config); } }
        #[cfg(not(target_os = "windows"))]
        if let Some(surface) = self.surface.as_ref() { surface.configure(&self.device, &self.config); }
        #[cfg(target_os = "windows")]
        if let Some(composition) = self.composition.as_ref() {
            if let Err(error) = composition.commit() {
                self.presentation_error = Some(format!("native composition commit failed: {error:#}"));
                return;
            }
        }
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

    fn render(
        &mut self,
        window: &NativeViewportWindow,
        viewer_started_at: Instant,
    ) -> Result<(), wgpu::SurfaceError> {
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
                "[VIEWER_STREAM] frame_uploads target_upload_chunks={} target_upload_vertices={} target_upload_indices={} preload_upload_chunks={} preload_upload_vertices={} preload_upload_indices={} preload_budget_chunks={} preload_budget_vertices={} preload_budget_indices={} target_deferred_chunks={} preload_deferred_chunks={} pending_uploads={} oldest_pending_age_frames={} cpu_cache_chunks={} resident_chunks={} admission_evicted_chunks={}",
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
                self.resident_chunks.len(),
                upload_stats.gpu_evicted_chunks
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

        if (self.frame_index < 5 || self.frame_index.is_multiple_of(120))
            && let Some(adapter) = self.quick_adapter.as_ref() {
            match crate::quick_resource_preflight::query_selected_adapter_budget(adapter) {
                Ok(budget) => println!("[QUICK_RESOURCE] stage=actual_usage frame={} current_usage_bytes={} actual_budget_bytes={} resident_chunks={} source_chunks={} artificial_residency_caps=false",
                    self.frame_index, budget.current_usage_bytes, budget.budget_bytes,
                    self.resident_chunks.len(), self.total_scene_chunks),
                Err(error) => eprintln!("[QUICK_RESOURCE] stage=actual_usage measurement_unavailable={error:#} allocation_failure_not_inferred=true"),
            }
        }

        #[cfg(target_os = "windows")]
        let has_vulkan_composition = self.vulkan_composition.is_some();
        #[cfg(not(target_os = "windows"))]
        let has_vulkan_composition = false;
        #[cfg(target_os = "macos")]
        let mac_frame = if let Some(composition) = self.mac_composition.as_mut() {
            match composition.begin_frame(&self.device).map_err(|error| {
                record_quick_gpu_failure(&self.quick_gpu_failure, format!("native Metal presentation failed: {error:#}"));
                wgpu::SurfaceError::Lost
            })? { Some(frame) => Some(frame), None => { self.device.poll(wgpu::Maintain::Poll); return Ok(()); } }
        } else { None };
        #[cfg(target_os = "macos")]
        let has_offscreen_presentation = has_vulkan_composition || mac_frame.is_some();
        #[cfg(not(target_os = "macos"))]
        let has_offscreen_presentation = has_vulkan_composition;
        let output = if has_offscreen_presentation { None } else {
            Some(self.surface.as_ref().ok_or(wgpu::SurfaceError::Lost)?.get_current_texture()?)
        };
        let output_texture = if let Some(output) = output.as_ref() { &output.texture } else {
            #[cfg(target_os = "windows")]
            { self.vulkan_composition.as_ref().expect("Vulkan native target exists").source() }
            #[cfg(target_os = "macos")]
            { mac_frame.as_ref().expect("native Metal frame lease exists").texture() }
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            { unreachable!() }
        };
        let view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());
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

        if let Some(overlay) = self.label_overlay.as_mut() {
            overlay.prepare(&self.device, &self.queue, &self.label_anchors, &self.camera);
        }

        let query_visible_samples = self.quick_visible_sample_query.should_query();
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("native_viewer_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(quick_clear_color(self.config.alpha_mode)),
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
                occlusion_query_set: query_visible_samples.then_some(&self.quick_visible_sample_query.query),
                timestamp_writes: None,
            });
            if query_visible_samples { render_pass.begin_occlusion_query(0); }
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
            // Translucent geometry must be drawn back-to-front so nearer alpha
            // surfaces (glass, slime, water, ice) blend over farther ones instead
            // of whichever chunk happens to come first in iteration order. Sort
            // resident chunks by distance from the camera, farthest first.
            let eye = self.camera.eye();
            let mut translucent_chunks: Vec<&ResidentChunkMesh> = self
                .resident_chunks
                .values()
                .filter(|resident| resident.gpu.translucent_index_buffer.is_some())
                .collect();
            translucent_chunks.sort_by(|a, b| {
                let da = a.bounds.center.distance_squared(eye);
                let db = b.bounds.center.distance_squared(eye);
                db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
            });
            for resident in translucent_chunks {
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
            if query_visible_samples { render_pass.end_occlusion_query(); }
        }

        if let Some(overlay) = self.label_overlay.as_ref() {
            let mut label_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("native_viewer_label_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_texture.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            overlay.draw(&mut label_pass);
        }

        let capture = self.capture_output.take().map(|path| {
            let stride = (self.config.width * 4 + 255) & !255;
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nova_real_frame_readback"), size: stride as u64 * self.config.height as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(wgpu::ImageCopyTexture { texture: output_texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::ImageCopyBuffer { buffer: &buffer, layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: Some(self.config.height) } },
                wgpu::Extent3d { width: self.config.width, height: self.config.height, depth_or_array_layers: 1 });
            (path, buffer, stride)
        });
        if query_visible_samples {
            encoder.resolve_query_set(&self.quick_visible_sample_query.query, 0..1, &self.quick_visible_sample_query.resolve, 0);
            encoder.copy_buffer_to_buffer(&self.quick_visible_sample_query.resolve, 0, &self.quick_visible_sample_query.readback, 0, 8);
        }
        #[cfg(target_os = "windows")]
        if let Some(composition) = self.vulkan_composition.as_ref() {
            composition.encode(&mut encoder).map_err(|error| {
                record_quick_gpu_failure(&self.quick_gpu_failure, format!("Vulkan native presentation failed: {error:#}"));
                wgpu::SurfaceError::Lost
            })?;
        }
        self.queue.submit(Some(encoder.finish()));
        if let Some((path, buffer, stride)) = capture {
            self.capture_error = save_gpu_frame(&self.device, &buffer, &path, self.config.width, self.config.height, stride, self.config.format).err().map(|error| error.to_string());
        }
        if let Some(output) = output { output.present(); }
        #[cfg(target_os = "macos")]
        if let Some(frame) = mac_frame {
            let real_geometry = self.resident_chunks.values().any(|resident|
                resident.gpu.solid_index_count > 0 || resident.gpu.translucent_index_count > 0);
            self.mac_composition.as_mut().expect("native Metal presenter exists")
                .submit(frame, &self.device, &self.queue, real_geometry).map_err(|error| {
                    record_quick_gpu_failure(&self.quick_gpu_failure, format!("native Metal submission failed: {error:#}"));
                    wgpu::SurfaceError::Lost
                })?;
        }
        #[cfg(target_os = "windows")]
        if let Some(composition) = self.vulkan_composition.as_ref() {
            composition.present().map_err(|error| {
                record_quick_gpu_failure(&self.quick_gpu_failure, format!("Vulkan native presentation failed: {error:#}"));
                wgpu::SurfaceError::Lost
            })?;
        }
        #[cfg(target_os = "windows")]
        if query_visible_samples {
            let present_count = self.native_present_statistics().map(|receipt| receipt.0);
            if let Some(count) = present_count { self.quick_visible_sample_query.submitted(count, self.quick_scene_revision); }
            else { self.quick_visible_sample_query.cancel(); }
        }
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
        self.build_cancellation.store(true, Ordering::Release);
        self.build_request_tx.take();
        self.build_workers.append(&mut self.retired_build_workers);
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

fn resolve_palette_color(
    block_id: &str,
    properties: Option<&BTreeMap<String, String>>,
    display_mode: ViewerDisplayMode,
) -> ResolvedBlockColor {

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
    let (rgb, matched_key, used_default) = if let Some(matched_key) = matched_key {
        (
            cache
                .get(matched_key.as_str())
                .copied()
                .unwrap_or(DEFAULT_BLOCK_COLOR),
            Some(matched_key),
            false,
        )
    } else {
        {
            let family = fallback_material_family(&normalized_key);
            (
                fallback_material_color(&normalized_key),
                Some(format!("fallback_material:{family}")),
                family == DEFAULT_BLOCK_COLOR_FAMILY,
            )
        }
    };
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
        let Ok(root) = runtime_paths::data_root() else {
            println!(
                "[VIEWER_COLOR] color_source=missing_data_root preview_chain=material_fallback native_chain=palette_vertex_color"
            );
            return HashMap::new();
        };
        let cache_path = root.join("blockColorCache.json");
        let payload = match std::fs::read_to_string(&cache_path) {
            Ok(payload) => payload,
            Err(error) => {
                println!(
                    "[VIEWER_COLOR] color_source=missing_cache path={} error={} native_chain=material_fallback",
                    cache_path.display(),
                    error
                );
                return HashMap::new();
            }
        };
        let cache = parse_block_color_cache(&payload).unwrap_or_else(|error| {
            println!(
                "[VIEWER_COLOR] color_source=invalid_cache path={} error={} native_chain=material_fallback",
                cache_path.display(),
                error
            );
            HashMap::new()
        });
        println!(
            "[VIEWER_COLOR] color_source=data/blockColorCache.json+material_fallback cache_entries={} path={}",
            cache.len(),
            cache_path.display()
        );
        cache
    })
}

fn parse_block_color_cache(payload: &str) -> Result<HashMap<String, [f32; 3]>, serde_json::Error> {
    if let Ok(cache) = serde_json::from_str::<HashMap<String, [f32; 3]>>(payload) {
        return Ok(cache);
    }
    let value = serde_json::from_str::<JsonValue>(payload)?;
    let colors = value
        .get("colors")
        .and_then(JsonValue::as_object)
        .cloned()
        .unwrap_or_default();
    let mut cache = HashMap::new();
    for (key, value) in colors {
        if let Some(rgb) = json_rgb(&value) {
            cache.insert(key, rgb);
        }
    }
    Ok(cache)
}

fn json_rgb(value: &JsonValue) -> Option<[f32; 3]> {
    let array = value.as_array()?;
    if array.len() != 3 {
        return None;
    }
    Some([
        array[0].as_f64()? as f32,
        array[1].as_f64()? as f32,
        array[2].as_f64()? as f32,
    ])
}

fn fallback_material_color(local: &str) -> [f32; 3] {
    match fallback_material_family(local) {
        "grass" => [0.34, 0.56, 0.22],
        "leaves" => [0.25, 0.49, 0.18],
        "dirt" => [0.45, 0.32, 0.20],
        "stone" => [0.50, 0.50, 0.48],
        "deepslate" => [0.25, 0.26, 0.28],
        "wood" => [0.55, 0.36, 0.18],
        "sand" => [0.82, 0.75, 0.50],
        "snow" => [0.88, 0.92, 0.94],
        "clay" => [0.49, 0.58, 0.62],
        "water" => [0.18, 0.36, 0.82],
        "lava" => [0.95, 0.36, 0.08],
        "glass" => [0.62, 0.82, 0.90],
        "redstone" => [0.72, 0.05, 0.04],
        "rail" => [0.55, 0.45, 0.28],
        "metal" => [0.68, 0.66, 0.60],
        "wool_white" => [0.86, 0.86, 0.82],
        "wool_orange" => [0.88, 0.42, 0.12],
        "wool_magenta" => [0.72, 0.28, 0.74],
        "wool_light_blue" => [0.38, 0.56, 0.86],
        "wool_yellow" => [0.92, 0.78, 0.20],
        "wool_lime" => [0.48, 0.72, 0.18],
        "wool_pink" => [0.90, 0.50, 0.62],
        "wool_gray" => [0.30, 0.32, 0.32],
        "wool_light_gray" => [0.62, 0.64, 0.62],
        "wool_cyan" => [0.22, 0.55, 0.62],
        "wool_purple" => [0.45, 0.25, 0.62],
        "wool_blue" => [0.20, 0.28, 0.68],
        "wool_brown" => [0.40, 0.25, 0.14],
        "wool_green" => [0.28, 0.42, 0.14],
        "wool_red" => [0.62, 0.16, 0.14],
        "wool_black" => [0.08, 0.08, 0.09],
        "terracotta" => [0.60, 0.36, 0.26],
        "concrete_white" => [0.82, 0.84, 0.80],
        "concrete_orange" => [0.88, 0.38, 0.08],
        "concrete_magenta" => [0.66, 0.20, 0.68],
        "concrete_light_blue" => [0.28, 0.50, 0.78],
        "concrete_yellow" => [0.86, 0.70, 0.08],
        "concrete_lime" => [0.44, 0.66, 0.10],
        "concrete_pink" => [0.84, 0.36, 0.50],
        "concrete_gray" => [0.26, 0.28, 0.28],
        "concrete_light_gray" => [0.55, 0.56, 0.54],
        "concrete_cyan" => [0.12, 0.46, 0.54],
        "concrete_purple" => [0.36, 0.18, 0.56],
        "concrete_blue" => [0.16, 0.22, 0.62],
        "concrete_brown" => [0.34, 0.20, 0.10],
        "concrete_green" => [0.22, 0.34, 0.08],
        "concrete_red" => [0.55, 0.10, 0.08],
        "concrete_black" => [0.05, 0.06, 0.07],
        _ => DEFAULT_BLOCK_COLOR,
    }
}

fn fallback_material_family(local: &str) -> &'static str {
    if local == "grass_block" || local == "short_grass" || local == "tall_grass" || local == "fern"
    {
        return "grass";
    }
    if local.ends_with("_leaves") || local == "vine" || local == "moss_block" {
        return "leaves";
    }
    if local.contains("dirt") || local == "mud" || local == "podzol" || local == "farmland" {
        return "dirt";
    }
    if local.contains("deepslate") || local.contains("blackstone") || local.contains("basalt") {
        return "deepslate";
    }
    if local.contains("stone")
        || local.contains("andesite")
        || local.contains("granite")
        || local.contains("diorite")
        || local.contains("tuff")
    {
        return "stone";
    }
    if local.contains("planks")
        || local.contains("_log")
        || local.contains("_wood")
        || local.contains("_stem")
        || local.contains("_hyphae")
        || local.contains("bamboo")
        || local.contains("chest")
        || local.contains("barrel")
    {
        return "wood";
    }
    if local.contains("sand") || local.contains("sandstone") {
        return "sand";
    }
    if local.contains("snow") || local == "ice" || local == "packed_ice" || local == "blue_ice" {
        return "snow";
    }
    if local.contains("clay") || local == "gravel" {
        return "clay";
    }
    if local == "water" || local == "kelp" || local == "seagrass" {
        return "water";
    }
    if local == "lava" || local.contains("magma") {
        return "lava";
    }
    if local.contains("glass") || local.ends_with("_pane") || local == "iron_bars" {
        return "glass";
    }
    if local.contains("redstone") || local == "repeater" || local == "comparator" {
        return "redstone";
    }
    if local.contains("rail") {
        return "rail";
    }
    if local.contains("iron")
        || local.contains("gold")
        || local.contains("copper")
        || local.contains("chain")
        || local.contains("anvil")
        || local.contains("hopper")
    {
        return "metal";
    }
    if local.ends_with("_wool") || local.ends_with("_carpet") {
        return dye_family(local, "wool");
    }
    if local.ends_with("_concrete") || local.ends_with("_concrete_powder") {
        return dye_family(local, "concrete");
    }
    if local.ends_with("_terracotta") || local == "terracotta" {
        return "terracotta";
    }
    DEFAULT_BLOCK_COLOR_FAMILY
}

const DEFAULT_BLOCK_COLOR_FAMILY: &str = "default_gray";

fn dye_family(local: &str, material: &str) -> &'static str {
    let colors = [
        "light_blue",
        "light_gray",
        "white",
        "orange",
        "magenta",
        "yellow",
        "lime",
        "pink",
        "gray",
        "cyan",
        "purple",
        "blue",
        "brown",
        "green",
        "red",
        "black",
    ];
    for color in colors {
        if local.starts_with(color) {
            return match (material, color) {
                ("wool", "white") => "wool_white",
                ("wool", "orange") => "wool_orange",
                ("wool", "magenta") => "wool_magenta",
                ("wool", "light_blue") => "wool_light_blue",
                ("wool", "yellow") => "wool_yellow",
                ("wool", "lime") => "wool_lime",
                ("wool", "pink") => "wool_pink",
                ("wool", "gray") => "wool_gray",
                ("wool", "light_gray") => "wool_light_gray",
                ("wool", "cyan") => "wool_cyan",
                ("wool", "purple") => "wool_purple",
                ("wool", "blue") => "wool_blue",
                ("wool", "brown") => "wool_brown",
                ("wool", "green") => "wool_green",
                ("wool", "red") => "wool_red",
                ("wool", "black") => "wool_black",
                ("concrete", "white") => "concrete_white",
                ("concrete", "orange") => "concrete_orange",
                ("concrete", "magenta") => "concrete_magenta",
                ("concrete", "light_blue") => "concrete_light_blue",
                ("concrete", "yellow") => "concrete_yellow",
                ("concrete", "lime") => "concrete_lime",
                ("concrete", "pink") => "concrete_pink",
                ("concrete", "gray") => "concrete_gray",
                ("concrete", "light_gray") => "concrete_light_gray",
                ("concrete", "cyan") => "concrete_cyan",
                ("concrete", "purple") => "concrete_purple",
                ("concrete", "blue") => "concrete_blue",
                ("concrete", "brown") => "concrete_brown",
                ("concrete", "green") => "concrete_green",
                ("concrete", "red") => "concrete_red",
                ("concrete", "black") => "concrete_black",
                _ => DEFAULT_BLOCK_COLOR_FAMILY,
            };
        }
    }
    DEFAULT_BLOCK_COLOR_FAMILY
}

fn load_scene(args: &ViewerArgs) -> Result<ViewerScene> {
 if args.quick_gpu_context_only { return Ok(ViewerScene::gpu_context_only()); } ViewerScene::from_litematic(args.input.as_deref().context("quick preview requires a scene")?,args.chunk_size,args.display_mode) }


// ------------------------------------------------------------------------
// Debug block-number overlay (LBA_BLOCK_LABELS_DEBUG)
//
// Floats a 1-based index label above each block "column" so renders can be
// matched to a legend by number instead of guessing. Temporary verification
// aid: opt-in via the LBA_BLOCK_LABELS_DEBUG env var, never on the normal path.
// Used by both the GPU offscreen preview and the live window (shared pipeline
// shape) so the same numbering is visible wherever you look.
// ------------------------------------------------------------------------

fn block_labels_enabled() -> bool {
    env_bool("LBA_BLOCK_LABELS_DEBUG")
}

#[derive(Clone)]
struct LabelAnchor {
    position: Vec3,
    text: String,
}

/// Group the scene's non-floor blocks into per-X "case columns" (the attach
/// test fixture spaces one case every few cells on X) and place a 1-based index
/// label just above each column's tallest block, left-to-right. The bottom-most
/// occupied Y layer is treated as ground and skipped so a floor slab doesn't
/// become hundreds of columns. For the fixture this reproduces the
/// `attach_legend.txt` numbering (01..N); for any other scene it still labels
/// each occupied X column in order. Debug-only.
fn compute_case_label_anchors(index: &ChunkSceneIndex) -> Vec<LabelAnchor> {
    let visual = match index.build_layer_index_output() {
        Ok(visual) => visual,
        Err(error) => {
            println!("[BLOCK_LABELS] enumerate_failed error={error}");
            return Vec::new();
        }
    };
    let mut blocks: Vec<(i32, i32, i32)> = Vec::new();
    for layer in &visual.visual.layers {
        for block in &layer.blocks {
            blocks.push((block.x, layer.y, block.z));
        }
    }
    if blocks.is_empty() {
        return Vec::new();
    }
    let min_y = blocks.iter().map(|(_, y, _)| *y).min().unwrap_or(0);
    let above_floor: Vec<(i32, i32, i32)> = blocks
        .iter()
        .copied()
        .filter(|(_, y, _)| *y > min_y)
        .collect();
    // Flat scene with a single layer: keep everything rather than label nothing.
    let working = if above_floor.is_empty() {
        blocks
    } else {
        above_floor
    };
    // gx -> (tallest_y, z_of_tallest)
    let mut columns: BTreeMap<i32, (i32, i32)> = BTreeMap::new();
    for (x, y, z) in working {
        columns
            .entry(x)
            .and_modify(|(top_y, top_z)| {
                if y > *top_y {
                    *top_y = y;
                    *top_z = z;
                }
            })
            .or_insert((y, z));
    }
    let anchors: Vec<LabelAnchor> = columns
        .into_iter()
        .enumerate()
        .map(|(idx, (gx, (top_y, gz)))| {
            let number = idx + 1;
            let text = if number < 100 {
                format!("{number:02}")
            } else {
                number.to_string()
            };
            LabelAnchor {
                position: Vec3::new(gx as f32 + 0.5, (top_y + 1) as f32 + 0.6, gz as f32 + 0.5),
                text,
            }
        })
        .collect();
    println!(
        "[BLOCK_LABELS] columns={} (enabled via LBA_BLOCK_LABELS_DEBUG)",
        anchors.len()
    );
    anchors
}

const OVERLAY_SHADER: &str = r#"
struct CameraUniform {
    view_proj: mat4x4<f32>,
    camera_position: vec4<f32>,
};
@group(0) @binding(0) var<uniform> camera: CameraUniform;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};
struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VsIn) -> VsOut {
    var out: VsOut;
    out.position = camera.view_proj * vec4<f32>(input.position, 1.0);
    out.color = input.color;
    return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    return input.color;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct OverlayVertex {
    position: [f32; 3],
    color: [f32; 4],
}

/// 3x5 bitmap glyphs for digits 0-9. Each row's low 3 bits are columns, bit 2 =
/// leftmost. Row 0 is the top row.
const DIGIT_FONT: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111], // 0
    [0b010, 0b110, 0b010, 0b010, 0b111], // 1
    [0b111, 0b001, 0b111, 0b100, 0b111], // 2
    [0b111, 0b001, 0b111, 0b001, 0b111], // 3
    [0b101, 0b101, 0b111, 0b001, 0b001], // 4
    [0b111, 0b100, 0b111, 0b001, 0b111], // 5
    [0b111, 0b100, 0b111, 0b101, 0b111], // 6
    [0b111, 0b001, 0b001, 0b001, 0b001], // 7
    [0b111, 0b101, 0b111, 0b101, 0b111], // 8
    [0b111, 0b101, 0b111, 0b001, 0b111], // 9
];

/// Push two triangles for the screen-aligned rectangle `[x0,x1] x [y0,y1]`
/// (label space, in glyph pixels) billboarded at `anchor` using camera `right`
/// and `up`, scaled by world units-per-pixel `px`.
fn push_overlay_quad(
    vertices: &mut Vec<OverlayVertex>,
    anchor: Vec3,
    right: Vec3,
    up: Vec3,
    px: f32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    color: [f32; 4],
) {
    let world = |a: f32, b: f32| -> [f32; 3] {
        let p = anchor + right * (a * px) + up * (b * px);
        [p.x, p.y, p.z]
    };
    let p00 = world(x0, y0);
    let p10 = world(x1, y0);
    let p11 = world(x1, y1);
    let p01 = world(x0, y1);
    for position in [p00, p10, p11, p00, p11, p01] {
        vertices.push(OverlayVertex { position, color });
    }
}

/// Self-contained billboard text pass: own pipeline (depth-compare Always so
/// labels draw on top), own camera uniform (only `view_proj` is read), digits
/// rendered as small solid quads. Reused by the offscreen and window paths.
struct LabelOverlay {
    pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    vertex_buffer: Option<wgpu::Buffer>,
    vertex_count: u32,
    vertex_capacity: usize,
}

impl LabelOverlay {
    fn new(
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("label_overlay_camera_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("label_overlay_camera"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("label_overlay_camera_bind_group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("label_overlay_shader"),
            source: wgpu::ShaderSource::Wgsl(OVERLAY_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("label_overlay_pipeline_layout"),
            bind_group_layouts: &[&camera_bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("label_overlay_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<OverlayVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x3,
                        },
                        wgpu::VertexAttribute {
                            offset: 12,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                    ],
                }],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
        });
        Self {
            pipeline,
            camera_buffer,
            camera_bind_group,
            vertex_buffer: None,
            vertex_count: 0,
            vertex_capacity: 0,
        }
    }

    /// Rebuild the billboarded glyph geometry for the current camera and upload
    /// the matching `view_proj`. Cheap (a few thousand verts), called per frame.
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        anchors: &[LabelAnchor],
        camera: &OrbitCamera,
    ) {
        let view_proj = camera.view_proj_matrix();
        let eye = camera.eye();
        queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&CameraUniform::from_matrix_and_eye(view_proj, eye)),
        );

        let forward = (camera.target - eye).normalize_or_zero();
        let mut right = forward.cross(Vec3::Y);
        if right.length_squared() < 1.0e-6 {
            right = Vec3::X;
        }
        let right = right.normalize();
        let up = right.cross(forward).normalize();

        let background = [0.05, 0.05, 0.07, 0.72];
        let digit_color = [1.0, 0.92, 0.15, 1.0];
        let mut vertices: Vec<OverlayVertex> = Vec::new();
        for anchor in anchors {
            let dist = (anchor.position - eye).length().max(0.001);
            // Keep labels at a roughly constant on-screen size regardless of zoom.
            let px = dist * 0.012;
            let digits: Vec<usize> = anchor
                .text
                .chars()
                .filter_map(|c| c.to_digit(10).map(|d| d as usize))
                .collect();
            if digits.is_empty() {
                continue;
            }
            let total_w = (digits.len() * 4 - 1) as f32;
            let half_w = total_w * 0.5;
            push_overlay_quad(
                &mut vertices,
                anchor.position,
                right,
                up,
                px,
                -half_w - 1.0,
                -3.0,
                half_w + 1.0,
                4.0,
                background,
            );
            for (ci, &digit) in digits.iter().enumerate() {
                let glyph = DIGIT_FONT[digit.min(9)];
                let base_x = (ci * 4) as f32 - half_w;
                for (row, bits) in glyph.iter().enumerate() {
                    let ly = (4 - row) as f32 - 2.0;
                    for col in 0..3u32 {
                        if (bits >> (2 - col)) & 1 == 1 {
                            let lx = base_x + col as f32;
                            push_overlay_quad(
                                &mut vertices,
                                anchor.position,
                                right,
                                up,
                                px,
                                lx,
                                ly,
                                lx + 1.0,
                                ly + 1.0,
                                digit_color,
                            );
                        }
                    }
                }
            }
        }

        self.vertex_count = vertices.len() as u32;
        if vertices.is_empty() {
            return;
        }
        let data: &[u8] = bytemuck::cast_slice(&vertices);
        if vertices.len() > self.vertex_capacity {
            self.vertex_buffer = Some(device.create_buffer_init(
                &wgpu::util::BufferInitDescriptor {
                    label: Some("label_overlay_vertices"),
                    contents: data,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                },
            ));
            self.vertex_capacity = vertices.len();
        } else if let Some(buffer) = &self.vertex_buffer {
            queue.write_buffer(buffer, 0, data);
        }
    }

    fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.vertex_count == 0 {
            return;
        }
        let Some(buffer) = &self.vertex_buffer else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..self.vertex_count, 0..1);
    }
}

/// GPU offscreen preview: renders one frame through the real wgpu pipeline
/// (identical shader, blend state and most importantly `cull_mode=Back` /
/// `front_face=Ccw`) to an offscreen texture, reads it back and writes a PNG.
///
/// Unlike [`render_offscreen_preview`] (a CPU rasterizer with *no* back-face
/// culling), this path faithfully reproduces winding / back-face / translucent
/// sorting behaviour. That makes it the only headless way to validate the
/// GPU-only bugs the CPU preview silently hides: water, glass panes, sign
/// front/back, single-sided geometry and over-cull.
///
/// Selected by setting `LBA_PREVIEW_GPU=1` alongside `--preview-output`.
/// View can be steered with `LBA_PREVIEW_GPU_SIZE=WxH`,
/// `LBA_PREVIEW_GPU_YAW`/`LBA_PREVIEW_GPU_PITCH` (degrees) and
/// `LBA_PREVIEW_GPU_DIST` (multiplier on the fit distance).
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
    format: &str,
    color_chain: &str,
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
        format: format.to_string(),
        color_chain: color_chain.to_string(),
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
        "[PREVIEW_CACHE] chunked_cache_manifest path={} chunk_dir={} renderable_chunks={} empty_mesh_chunks={} total_chunks={} format={} color_chain={}",
        path.display(),
        chunk_dir.display(),
        payload.renderable_chunks,
        payload.empty_mesh_chunks,
        payload.total_chunks,
        payload.format,
        payload.color_chain
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

#[cfg(target_os = "windows")]
fn window_hwnd(window: &impl HasWindowHandle) -> Result<isize> {
    match window.window_handle()?.as_raw() {
        RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get()),
        other => bail!("embed_parent_hwnd requires Win32 window handle, got {other:?}"),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PersistentPresentationMode {
    Hidden,
    Popup,
    Embedded,
}

#[derive(Debug)]
enum ViewerPresentationCommandOutcome {
    NotPresentation,
    Applied(ViewerSessionState),
    Release(ViewerSessionState),
}


fn complete_viewer_session_command(
    watcher: &mut ViewerSessionWatcher,
    command: &ViewerSessionCommand,
    state: ViewerSessionState,
    hwnd: isize,
) -> Result<()> {
    watcher.publish_status(command.sequence, state, std::process::id(), hwnd)?;
    watcher.acknowledge(command.sequence)
}

fn reject_viewer_session_command(
    watcher: &mut ViewerSessionWatcher,
    command: &ViewerSessionCommand,
    error: &anyhow::Error,
    hwnd: isize,
) -> Result<()> {
    complete_viewer_session_command(
        watcher,
        command,
        ViewerSessionState::Error {
            command_kind: command.kind,
            message: format!("{error:#}"),
        },
        hwnd,
    )
}

fn publish_viewer_hidden_event(
    watcher: Option<&mut ViewerSessionWatcher>,
    hwnd: isize,
    backend: &str,
    reason: &str,
) {
    let Some(watcher) = watcher else {
        return;
    };
    match watcher.publish_event(ViewerSessionState::Hidden, std::process::id(), hwnd) {
        Ok(path) => println!(
            "[QUICK_SESSION] status=event_published event=hidden backend={} reason={} path={}",
            backend,
            reason,
            path.display(),
        ),
        Err(error) => println!(
            "[QUICK_SESSION] status=event_publish_error event=hidden backend={} reason={} fatal=false error={error:#}",
            backend, reason,
        ),
    }
}


pub fn run(provenance: &BackendProvenance) -> Result<()> {
    let Some(args) = parse_launch_contract_args(provenance)? else { return Ok(()); };
    runtime_paths::ensure_runtime_layout()?;
    println!("[NATIVE_VIEWER] startup public_beta=true route=legacy-wgpu mode=fast_experimental request_id={}",args.launch_request_id);
    let total_trace_started_at=Instant::now();
    let mut session_watcher = if args.persistent_popup_presentation {
        Some(ViewerSessionWatcher::from_environment()?.context(
            "quick_preview requires a persistent Viewer session directory and ID",
        )?)
    } else {
        None
    };

    let scene = load_scene(&args)?;
    let title = format!("Litematica-Nova Viewer - {}", scene.label);
    #[cfg(target_os="linux")]
    let event_loop = {
        use winit::platform::x11::EventLoopBuilderExtX11;
        let mut builder = winit::event_loop::EventLoopBuilder::new();
        if args.quick_host.is_some() { builder.with_x11(); }
        builder.build()?
    };
    #[cfg(not(target_os="linux"))]
    let event_loop = EventLoop::new()?;
    let viewer_started_at = Instant::now();
    let auto_exit_after = args.auto_exit_seconds.map(Duration::from_secs);
    let window_builder = WindowBuilder::new()
            .with_title(title)
            .with_inner_size(PhysicalSize::new(1280_u32, 800_u32))
            .with_decorations(true)
            .with_visible(
                args.initial_visible
                    && !args.prebuild_before_show
                    && args.embed_parent_hwnd.is_none(),
            );
    #[cfg(target_os = "windows")]
    let (window_builder, _child_dpi_context) = if let Some(host) = &args.quick_host {
        // A redirection bitmap would remain an opaque HWND background below
        // the premultiplied composition visual. This child is GPU-only content.
        let (builder, context) = native_child::builder(window_builder.with_no_redirection_bitmap(true), host)?;
        (builder, Some(context))
    } else { (window_builder, None) };
    #[cfg(target_os="linux")]
    let window_builder = if let Some(host) = &args.quick_host {
        native_child::builder(window_builder, host)?
    } else { window_builder };
    let window = crate::native_viewport::build(window_builder, &event_loop, args.quick_host.as_ref())?;
    #[cfg(target_os = "windows")]
    if let Some(host) = &args.quick_host {
        native_child::verify(window.as_ref(), host)?;
    }
    #[cfg(target_os="linux")]
    if let Some(host) = &args.quick_host { native_child::verify(window.as_ref(), host)?; }
    #[cfg(not(target_os = "windows"))]
    if args.quick_host.is_none() {
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
    if args.quick_gpu_context_only {
        ensure!(state.chunk_scene_index.is_none() && state.resident_chunks.is_empty(), "Static GPU preparation unexpectedly owns projection geometry");
        eprintln!("[NOVA_GPU_INITIALIZATION] stage=static_context_ready epoch_ms={} total_prepare_ms={:.3} projection_reads=0 scene_parse_runs=0 scene_mesh_builds=0 resident_chunks=0 window_visible=false first_display=not_measured",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |value| value.as_millis()), total_trace_started_at.elapsed().as_secs_f64()*1000.0);
    }
    let mut pending_capture: Option<(ViewerSessionCommand, PathBuf, Instant)> = None;
    #[cfg(target_os = "windows")]
    let legacy_window_hwnd = window_hwnd(window.as_ref())?;
    #[cfg(target_os="linux")]
    let legacy_window_hwnd = {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        match window.window_handle()?.as_raw() {
            RawWindowHandle::Xlib(handle) => handle.window as isize,
            RawWindowHandle::Xcb(handle) => handle.window.get() as isize,
            _ => 0,
        }
    };
    #[cfg(not(any(target_os="windows",target_os="linux")))]
    let legacy_window_hwnd = 0_isize;
    register_viewer_shutdown_hwnd(legacy_window_hwnd);
    let mut presentation_mode = if args.initial_visible {
        PersistentPresentationMode::Popup
    } else {
        PersistentPresentationMode::Hidden
    };
    if state.prebuild_before_show {
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
    println!("[NATIVE_VIEWER] runtime_ready route=legacy-wgpu");
    if let Some(watcher) = session_watcher.as_ref() {
        watcher.publish_status(
            0,
            ViewerSessionState::Ready,
            std::process::id(),
            legacy_window_hwnd,
        )?;
        println!(
            "[QUICK_PREVIEW] status={} session_id={} mode=fast_experimental scene_loaded={} full_mode=false",
            if args.quick_gpu_context_only { "gpu_context_ready" } else { "ready" }, watcher.session_id(), !args.quick_gpu_context_only,
        );
        std::io::stdout().flush().context("flush quick preview readiness")?;
    }
    let fatal_error = Arc::new(Mutex::new(None::<String>));
    let event_error = Arc::clone(&fatal_error);
    let quick_host = args.quick_host.clone();

    let quick_request_id = args.launch_request_id.clone();
    let mut quick_presentation_audit = None::<(usize, usize)>;
    let mut quick_redraw = QuickRedrawState::new();
    let mut quick_occluded = false;
    #[cfg(target_os = "windows")]
    let mut quick_first_block_timing = first_block_timing::FirstBlockTiming::new_fast(
        args.input.as_deref().unwrap_or_else(|| Path::new("<empty>")));
    #[cfg(target_os = "macos")]
    let mut last_mac_display_epoch = None;
    let event_loop_result = event_loop.run(move |event, event_loop_window_target| {
        event_loop_window_target.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(40)));
        #[cfg(target_os = "macos")]
        let event = if let Some(viewport) = window.shared() {
            state.device.poll(wgpu::Maintain::Poll);
            let mut escape = false;
            for input in viewport.poll_events() {
                use crate::macos_presentation::ProducerEvent;
                match input {
                    ProducerEvent::Input(input) => match input.kind {
                        1 => { state.process_cursor_moved(PhysicalPosition::new(input.x, input.y)); quick_redraw.request(); }
                        2 | 3 => {
                            let button = match input.code {
                                0 => MouseButton::Left, 1 => MouseButton::Right, 2 => MouseButton::Middle,
                                other => MouseButton::Other(u16::try_from(other).unwrap_or(u16::MAX)),
                            };
                            state.process_mouse_input(if input.kind == 2 { ElementState::Pressed } else { ElementState::Released }, button);
                            quick_redraw.request();
                        }
                        4 => {
                            let delta = if input.flags & 2 != 0 {
                                MouseScrollDelta::PixelDelta(PhysicalPosition::new(input.dx, input.dy))
                            } else { MouseScrollDelta::LineDelta(input.dx as f32, input.dy as f32) };
                            state.process_scroll(delta); quick_redraw.request();
                        }
                        5 if input.code == 15 => { state.reset_camera(); quick_redraw.request(); }
                        5 if input.code == 53 => { escape = true; }
                        7 if input.flags & 1 == 0 => { state.drag_mode = None; state.last_cursor_position = None; }
                        _ => {}
                    },
                    ProducerEvent::ResetInput { .. } => { state.drag_mode = None; state.last_cursor_position = None; }
                    ProducerEvent::ViewportResized { .. } => { state.resize(window.inner_size()); quick_redraw.request(); }
                    ProducerEvent::Hidden { .. } => {
                        presentation_mode = PersistentPresentationMode::Hidden;
                        quick_redraw.hide(); state.drag_mode = None; state.last_cursor_position = None;
                        publish_viewer_hidden_event(session_watcher.as_mut(), 0, "quick-metal", "popup_close");
                    }
                    ProducerEvent::Displayed { epoch, frame_id, presented_time_seconds, .. } => {
                        if last_mac_display_epoch != Some(epoch) {
                            eprintln!("[NOVA_GPU_PRESENT] platform=macos mode=fast epoch={epoch} frame_id={frame_id} presented_time_seconds={presented_time_seconds:.9} native_compositor_receipt=true visible_pixel_sample=not_measured");
                            last_mac_display_epoch = Some(epoch);
                        }
                    }
                    ProducerEvent::Failed(message) => {
                        record_viewer_shutdown("native_presentation_failed", 1, &message);
                        *event_error.lock().expect("quick native error mutex") = Some(message);
                        event_loop_window_target.exit(); return;
                    }
                    ProducerEvent::FrameRejected { .. } => { quick_redraw.request(); }
                }
            }
            if escape {
                viewport.suspend(); presentation_mode = PersistentPresentationMode::Hidden;
                quick_redraw.hide(); state.drag_mode = None; state.last_cursor_position = None;
                publish_viewer_hidden_event(session_watcher.as_mut(), 0, "quick-metal", "escape_key");
            }
            if viewport.closed() { event_loop_window_target.exit(); return; }
            let size = window.inner_size();
            if size != state.size { state.resize(size); quick_redraw.request(); }
            if viewport.visible() && matches!(event, Event::NewEvents(_)) && viewport.take_redraw() {
                Event::WindowEvent { window_id: window.id(), event: WindowEvent::RedrawRequested }
            } else { event }
        } else { event };
        match event {
            Event::WindowEvent { event, window_id } if window_id == window.id() => match event {
                WindowEvent::Destroyed => {
                    record_viewer_shutdown("native_child_destroyed", 0, "native host destroyed the Viewer child");
                    event_loop_window_target.exit();
                }
                WindowEvent::CloseRequested => {
                    if session_watcher.is_some() {
                        if let Err(error) = hide_quick_preview_window(window.as_ref(), quick_host.as_ref()) {
                            let message = format!("quick preview hide failed: {error:#}");
                            record_viewer_shutdown("quick_preview_hide", 1, &message);
                            *event_error.lock().expect("quick preview event error mutex") = Some(message);
                            event_loop_window_target.exit();
                            return;
                        }
                        presentation_mode = PersistentPresentationMode::Hidden;
                        quick_redraw.hide();
                        state.drag_mode = None;
                        state.last_cursor_position = None;
                        publish_viewer_hidden_event(session_watcher.as_mut(), legacy_window_hwnd, "quick-wgpu", "window_close");
                        #[cfg(target_os = "windows")]
                        quick_first_block_timing.hide(); state.quick_visible_sample_query.cancel();
                        return;
                    }
                    record_viewer_shutdown(
                        "window_close_requested",
                        0,
                        "user closed the legacy Viewer window",
                    );
                    event_loop_window_target.exit();
                }
                WindowEvent::Resized(new_size) => { state.resize(new_size); quick_redraw.request(); },
                WindowEvent::Occluded(occluded) => {
                    quick_occluded = occluded;
                    if occluded { quick_redraw.hide(); } else { quick_redraw.request(); }
                }
                WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    state.composition_dpi = scale_factor;
                    state.resize(window.inner_size());
                    quick_redraw.request();
                }
                WindowEvent::MouseInput {
                    state: element_state,
                    button,
                    ..
                } => {
                    if element_state == ElementState::Pressed && quick_host.is_some() {
                        window.focus_window();
                    }
                    state.process_mouse_input(element_state, button);
                    quick_redraw.request();
                }
                WindowEvent::CursorMoved { position, .. } => {
                    if state.drag_mode.is_some() { quick_redraw.request(); }
                    state.process_cursor_moved(position);
                }
                WindowEvent::CursorLeft { .. } => {
                    state.last_cursor_position = None;
                }
                WindowEvent::Focused(false) if session_watcher.is_some() => {
                    state.drag_mode = None;
                    state.last_cursor_position = None;
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    state.process_scroll(delta);
                    quick_redraw.request();
                }
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed =>
                {
                    match event.physical_key {
                        #[cfg(any(target_os="windows",target_os="linux"))]
                        PhysicalKey::Code(KeyCode::Tab) if quick_host.is_some() => {
                            native_child::focus_host(quick_host.as_ref().unwrap());
                        }
                        PhysicalKey::Code(KeyCode::Escape) => {
                            if session_watcher.is_some() {
                                if let Err(error) = hide_quick_preview_window(window.as_ref(), quick_host.as_ref()) {
                                    let message = format!("quick preview hide failed: {error:#}");
                                    record_viewer_shutdown("quick_preview_hide", 1, &message);
                                    *event_error.lock().expect("quick preview event error mutex") = Some(message);
                                    event_loop_window_target.exit();
                                    return;
                                }
                                presentation_mode = PersistentPresentationMode::Hidden;
                                quick_redraw.hide();
                                state.drag_mode = None;
                                state.last_cursor_position = None;
                                publish_viewer_hidden_event(session_watcher.as_mut(), legacy_window_hwnd, "quick-wgpu", "escape_key");
                                #[cfg(target_os = "windows")]
                                quick_first_block_timing.hide(); state.quick_visible_sample_query.cancel();
                                return;
                            }
                            record_viewer_shutdown(
                                "escape_key",
                                0,
                                "user closed the legacy Viewer with Escape",
                            );
                            event_loop_window_target.exit();
                        }
                        PhysicalKey::Code(KeyCode::KeyR) => { state.reset_camera(); quick_redraw.request(); },
                        _ => {}
                    }
                }
                WindowEvent::RedrawRequested => {
                    if quick_occluded || state.size.width == 0 || state.size.height == 0
                        || (session_watcher.is_some()
                            && (presentation_mode == PersistentPresentationMode::Hidden || state.chunk_scene_index.is_none()))
                    {
                        return;
                    }
                    match state.render(window.as_ref(), viewer_started_at) {
                        Ok(()) => {
                            quick_redraw.presented(Instant::now());
                            #[cfg(target_os = "windows")]
                            if state.config.width != 0 && state.config.height != 0 && state.resident_chunks.iter().any(|(key, chunk)|
                                (chunk.gpu.solid_index_count != 0 || chunk.gpu.translucent_index_count != 0)
                                    && (state.uses_full_scene_cache_runtime() || state.current_target_keys.contains(key))) {
                                let receipt = state.native_present_statistics();
                                if let Some((submitted, displayed)) = receipt {
                                    quick_first_block_timing.exact_present_submitted(Some(submitted));
                                    quick_first_block_timing.observe_display(displayed);
                                    if quick_first_block_timing.needs_visible_sample() { state.quick_visible_sample_query.retry(); }
                                }
                            }
                            if let Some((before_mesh, before_upload)) = quick_presentation_audit.take() {
                                eprintln!("[NOVA_RENDER_PHASE] stage=fast_restore_render_call loading={} async_mesh_chunks_delta={} gpu_buffer_upload_requests_delta={} resident_chunks={} cold_scene=false display_completion=not_inferred",
                                    if state.prebuild_before_show { "preloaded" } else { "realtime" }, state.trace.worker_chunks.load(Ordering::Relaxed).saturating_sub(before_mesh), state.trace.gpu_buffer_upload_requests.load(Ordering::Relaxed).saturating_sub(before_upload), state.resident_chunks.len());
                            }
                        }
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            state.resize(state.size);
                            quick_redraw.retry(Instant::now());
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            println!("[WGPU_INIT] surface_out_of_memory");
                            record_viewer_shutdown(
                                "legacy_surface_out_of_memory",
                                1,
                                "legacy wgpu surface reported out of memory",
                            );
                            event_loop_window_target.exit();
                        }
                        Err(wgpu::SurfaceError::Timeout) => {
                            println!("[WGPU_INIT] surface_timeout");
                            quick_redraw.retry(Instant::now());
                        }
                    }
                }
                _ => {}
            },
            Event::AboutToWait => {
                state.reap_retired_build_workers();
                if let Some(message) = state.take_quick_gpu_failure() {
                    record_viewer_shutdown("quick_gpu_failure", 1, &message);
                    *event_error.lock().expect("quick GPU failure mutex") = Some(message);
                    event_loop_window_target.exit();
                    return;
                }
                #[cfg(target_os = "windows")]
                if let Some((count, revision)) = state.quick_visible_sample_query.poll(&state.device, state.quick_scene_revision) {
                    quick_first_block_timing.visible_projection_present_submitted(Some(count), revision);
                }
                if let Some(message) = state.presentation_error.take() {
                    record_viewer_shutdown("native_composition_failed", 1, &message);
                    *event_error.lock().expect("quick preview event error mutex") = Some(message);
                    event_loop_window_target.exit();
                    return;
                }
                #[cfg(any(target_os="windows",target_os="linux"))]
                if let Some(host) = &quick_host {
                    let parent = match native_child::validate_host(host) {
                        Ok(parent) => parent,
                        Err(_) => {
                            record_viewer_shutdown("native_host_closed", 0, "native Viewer host has ended");
                            event_loop_window_target.exit();
                            return;
                        }
                    };
                    if native_child::host_needs_hide(parent, presentation_mode) {
                        if let Err(error) = native_child::hide(window.as_ref(), host) {
                            let message = format!("quick preview host hide failed: {error:#}");
                            record_viewer_shutdown("quick_preview_hide", 1, &message);
                            *event_error.lock().expect("quick preview event error mutex") = Some(message);
                            event_loop_window_target.exit();
                            return;
                        }
                        // Clear WS_VISIBLE before the host can reappear. Only a
                        // fresh sequenced attach may restore the child viewport.
                        presentation_mode = PersistentPresentationMode::Hidden;
                        quick_redraw.hide();
                        state.drag_mode = None;
                        state.last_cursor_position = None;
                        #[cfg(target_os = "windows")]
                        { quick_first_block_timing.hide(); state.quick_visible_sample_query.cancel(); }
                    }
                }
                if let Some(watcher) = session_watcher.as_mut() {
                    // Win32 may coalesce wakeups. Drain a bounded batch so
                    // Hide -> ReplaceScene -> Attach does not wait 40ms per
                    // command; every command retains its sequence validation.
                    for _ in 0..16 {
                    let command = match watcher.poll_next() {
                        Ok(command) => command,
                        Err(error) => {
                            let message = format!("quick preview session poll failed: {error:#}");
                            record_viewer_shutdown("quick_preview_session_poll", 1, &message);
                            *event_error.lock().expect("quick preview event error mutex") = Some(message);
                            event_loop_window_target.exit();
                            return;
                        }
                    };
                    let Some(command) = command else { break; };
                    {
                        if state.chunk_scene_index.is_none() && matches!(command.kind,
                            ViewerSessionCommandKind::AttachEmbedded | ViewerSessionCommandKind::ShowOverlay
                            | ViewerSessionCommandKind::ShowPopup | ViewerSessionCommandKind::CaptureFrame) {
                            if let Err(error) = reject_viewer_session_command(watcher, &command, &anyhow!("请先加载投影。"), legacy_window_hwnd) {
                                *event_error.lock().expect("quick static context error mutex") = Some(error.to_string());
                                event_loop_window_target.exit(); return;
                            }
                        } else if command.kind == ViewerSessionCommandKind::ReplaceScene {
                            let started = Instant::now();
                            let replacement = (|| -> Result<()> {
                                hide_quick_preview_window(window.as_ref(), quick_host.as_ref())?;
                                presentation_mode = PersistentPresentationMode::Hidden;
                                quick_redraw.hide();
                                quick_presentation_audit = None;
                                #[cfg(target_os = "windows")]
                                quick_first_block_timing.hide(); state.quick_visible_sample_query.cancel();
                                if let Some((capture, _, _)) = pending_capture.take() {
                                    watcher.publish_status(capture.sequence, ViewerSessionState::Error { command_kind: capture.kind, message: "投影已切换，旧帧未保存。".into() }, std::process::id(), legacy_window_hwnd)?;
                                }
                                let path = command.launch_contract.as_deref().context("Fast scene replacement has no launch contract")?;
                                let mut next_args = parse_launch_contract_path(&provenance, path)?;
                                ensure!(next_args.display_mode == ViewerDisplayMode::FastExperimental
                                    && next_args.persistent_popup_presentation && next_args.quick_host == quick_host
                                    && next_args.launch_request_id == watcher.session_id(), "Fast replacement differs from the existing native session/host");
                                let same_input = state.quick_input_is_current(&next_args);
                                if !same_input {
                                    state.build_cancellation.store(true, Ordering::Release);
                                    state.build_request_tx.take();
                                }
                                let scene = if same_input { None } else { Some(load_scene(&next_args)?) };
                                #[cfg(target_os = "windows")]
                                { quick_first_block_timing = first_block_timing::FirstBlockTiming::new_fast(next_args.input.as_deref().context("Fast replacement source missing")?); }
                                state.replace_quick_scene(scene, &mut next_args)?;
                                quick_redraw.request();
                                if state.prebuild_before_show { state.run_prebuild_until_ready(window.as_ref(), started)?; }
                                // Replace old swapchain pixels while still
                                // hidden. GPU submission is not monitor display
                                // and does not complete the first-block timer.
                                state.render(window.as_ref(), started).context("submit new Fast scene before native visibility")?;
                                if let Some(error) = state.take_quick_gpu_failure() { bail!(error); }
                                eprintln!("[NOVA_RENDER_PHASE] stage=fast_scene_ready elapsed_ms={:.3} same_input_parsing_reused={} existing_gpu_context=true new_gpu_devices=0 new_composition_surfaces=0 display_verified=false", started.elapsed().as_secs_f64()*1000.0, same_input);
                                Ok(())
                            })();
                            let status = match replacement {
                                Ok(()) => ViewerSessionState::SceneReady { request_id: watcher.session_id().to_owned() },
                                Err(error) => ViewerSessionState::Error { command_kind: command.kind, message: format!("快速预览加载失败：{error:#}") },
                            };
                            if let Err(error) = complete_viewer_session_command(watcher, &command, status, legacy_window_hwnd) {
                                *event_error.lock().expect("quick replacement error mutex") = Some(error.to_string());
                                event_loop_window_target.exit(); return;
                            }
                        } else if command.kind == ViewerSessionCommandKind::CaptureFrame {
                            if pending_capture.is_some() {
                                let _ = reject_viewer_session_command(watcher, &command, &anyhow!("另一个真实帧读取尚未结束"), legacy_window_hwnd);
                            } else {
                                let path = watcher.capture_path(command.sequence);
                                if let Err(error) = watcher.acknowledge(command.sequence) { *event_error.lock().expect("capture error mutex") = Some(error.to_string()); event_loop_window_target.exit(); return; }
                                pending_capture = Some((command, path, Instant::now()));
                            }
                        } else {
                        let mesh_before = state.trace.worker_chunks.load(Ordering::Relaxed);
                        let upload_before = state.trace.gpu_buffer_upload_requests.load(Ordering::Relaxed);
                        if matches!(command.kind, ViewerSessionCommandKind::AttachEmbedded | ViewerSessionCommandKind::ShowPopup) {
                            quick_presentation_audit = Some((mesh_before, upload_before));
                            #[cfg(target_os = "windows")]
                            {
                                if let Some(click) = command.click_started_at_epoch_ms { quick_first_block_timing.set_click_for_presentation(click); }
                                quick_first_block_timing.begin_presentation();
                                state.quick_visible_sample_query.arm();
                            }
                        } else { quick_presentation_audit = None; }
                        #[cfg(target_os = "windows")]
                        if matches!(command.kind, ViewerSessionCommandKind::Hide | ViewerSessionCommandKind::Release) {
                            quick_first_block_timing.hide(); state.quick_visible_sample_query.cancel();
                        }
                        #[cfg(any(target_os="windows",target_os="linux"))]
                        let child_outcome = quick_host.as_ref().map(|host|
                            native_child::apply(window.as_ref(), host, &command, &mut presentation_mode));
                        #[cfg(target_os="macos")]
                        let child_outcome = window.shared().map(|shared| (|| -> Result<ViewerPresentationCommandOutcome> {
                            if !shared.apply(&command)? { return Ok(ViewerPresentationCommandOutcome::NotPresentation); }
                            let status = match command.kind {
                                ViewerSessionCommandKind::AttachEmbedded => {
                                    presentation_mode = PersistentPresentationMode::Embedded;
                                    ViewerSessionState::Attached { host_hwnd: "0".into(), width: shared.extent()[0], height: shared.extent()[1] }
                                }
                                ViewerSessionCommandKind::Resize => ViewerSessionState::Resized {
                                    width: shared.extent()[0], height: shared.extent()[1], focused: command.focused,
                                },
                                ViewerSessionCommandKind::ShowPopup => { presentation_mode = PersistentPresentationMode::Popup; ViewerSessionState::PopupVisible }
                                ViewerSessionCommandKind::Release => {
                                    presentation_mode = PersistentPresentationMode::Hidden;
                                    return Ok(ViewerPresentationCommandOutcome::Release(ViewerSessionState::Released));
                                }
                                _ => { presentation_mode = PersistentPresentationMode::Hidden; ViewerSessionState::Hidden }
                            };
                            Ok(ViewerPresentationCommandOutcome::Applied(status))
                        })());
                        #[cfg(not(any(target_os="windows",target_os="linux",target_os="macos")))]
                        let child_outcome: Option<Result<ViewerPresentationCommandOutcome>> = None;
                        let outcome = child_outcome.unwrap_or_else(|| Err(anyhow!("native quick preview requires its original platform Host")));
                        if matches!(command.kind, ViewerSessionCommandKind::Hide
                            | ViewerSessionCommandKind::AttachEmbedded
                            | ViewerSessionCommandKind::ShowOverlay
                            | ViewerSessionCommandKind::ShowPopup)
                        {
                            state.drag_mode = None;
                            state.last_cursor_position = None;
                        }
                        let mut release = false;
                        let completion = match outcome {
                            Ok(ViewerPresentationCommandOutcome::Applied(status)) => {
                                if presentation_mode == PersistentPresentationMode::Hidden { quick_redraw.hide(); }
                                else { quick_redraw.request(); }
                                if state.size != window.inner_size() {
                                    state.resize(window.inner_size());
                                }
                                complete_viewer_session_command(watcher, &command, status, legacy_window_hwnd)
                            }
                            Ok(ViewerPresentationCommandOutcome::Release(status)) => {
                                release = true;
                                complete_viewer_session_command(watcher, &command, status, legacy_window_hwnd)
                            }
                            Ok(ViewerPresentationCommandOutcome::NotPresentation) => {
                                reject_viewer_session_command(watcher, &command,
                                    &anyhow!("当前快速预览命令无效。"),
                                    legacy_window_hwnd)
                            }
                            Err(error) => reject_viewer_session_command(watcher, &command, &error, legacy_window_hwnd),
                        };
                        eprintln!("[NOVA_RENDER_PHASE] stage=fast_presentation_applied request_id={} command={:?} sequence={} loading={} async_mesh_chunks_delta={} gpu_buffer_upload_requests_delta={} resident_chunks={} scope=control_handler GPU_completion=not_inferred",
                            quick_request_id, command.kind, command.sequence, if state.prebuild_before_show { "preloaded" } else { "realtime" }, state.trace.worker_chunks.load(Ordering::Relaxed).saturating_sub(mesh_before), state.trace.gpu_buffer_upload_requests.load(Ordering::Relaxed).saturating_sub(upload_before), state.resident_chunks.len());
                        if let Err(error) = completion {
                            let message = format!("quick preview session acknowledgement failed: {error:#}");
                            record_viewer_shutdown("quick_preview_session_ack", 1, &message);
                            *event_error.lock().expect("quick preview event error mutex") = Some(message);
                            event_loop_window_target.exit();
                            return;
                        }
                        if release {
                            record_viewer_shutdown("session_release", 0, "Nova released the independent quick Viewer");
                            event_loop_window_target.exit();
                            return;
                        }
                        }
                    }
                    }
                    if let Some((command, path, started)) = pending_capture.as_ref() {
                        let ready = state.frame_index > 0 && state.pending_build_queue.is_empty() && state.in_flight_build_keys.is_empty() && state.pending_upload_queue.is_empty();
                        let outcome = (|| -> Result<bool> {
                            if started.elapsed() > Duration::from_secs(60) { bail!("快速预览仍在构建，请稍后生成缩略图"); }
                            if !state.config.usage.contains(wgpu::TextureUsages::COPY_SRC) { bail!("当前GPU表面不支持真实帧读取"); }
                            if ready { state.capture_error = None; state.capture_output = Some(path.clone()); }
                            state.render(window.as_ref(), viewer_started_at).context("read actual preview frame")?;
                            if let Some(error) = state.capture_error.take() { bail!("{error}"); }
                            Ok(ready)
                        })();
                        let status = match outcome {
                            Ok(false) => None,
                            Ok(true) => Some(ViewerSessionState::Captured),
                            Err(error) => Some(ViewerSessionState::Error { command_kind: command.kind, message: error.to_string() }),
                        };
                        if let Some(status) = status {
                            let _ = watcher.publish_status(command.sequence, status, std::process::id(), legacy_window_hwnd);
                            pending_capture = None;
                        }
                    }
                    if presentation_mode == PersistentPresentationMode::Hidden {
                        return;
                    }
                }
                #[cfg(target_os = "windows")]
                if presentation_mode != PersistentPresentationMode::Hidden
                    && state.chunk_scene_index.is_some()
                    && quick_first_block_timing.needs_display_confirmation() {
                    let displayed = state.native_present_statistics().and_then(|receipt| receipt.1);
                    quick_first_block_timing.observe_display(displayed);
                    if quick_first_block_timing.needs_visible_sample() { state.quick_visible_sample_query.retry(); }
                }
                if let Some(auto_exit_after) = auto_exit_after
                    && viewer_started_at.elapsed() >= auto_exit_after
                {
                    println!(
                        "[NATIVE_VIEWER] auto_exit elapsed_seconds={} reason=auto_exit_seconds",
                        auto_exit_after.as_secs()
                    );
                    record_viewer_shutdown(
                        "auto_exit_seconds",
                        0,
                        format!(
                            "configured legacy auto-exit after {}s",
                            auto_exit_after.as_secs()
                        ),
                    );
                    event_loop_window_target.exit();
                    return;
                }
                let now = Instant::now();
                let pending_work = state.residency_dirty || !state.pending_build_queue.is_empty()
                    || !state.in_flight_build_keys.is_empty()
                    || (!state.pending_upload_queue.is_empty()
                        && state.quick_upload_retry_at.is_none_or(|at| now >= at))
                    || state.prebuild_writer_completed_chunks < state.prebuild_writer_submitted_chunks
                    || (!state.prebuild_before_show && !state.background_fill_enabled
                        && state.chunk_scene_index.is_some());
                let animated = (state.preview_mode && state.preview_spin) || state.stress_camera_sweep;
                let presentable = !quick_occluded && state.size.width != 0 && state.size.height != 0
                    && (session_watcher.is_none() || (presentation_mode != PersistentPresentationMode::Hidden
                        && state.chunk_scene_index.is_some()));
                let sample_retry = state.quick_visible_sample_query.should_query();
                #[cfg(target_os = "windows")]
                let awaiting_receipt = quick_first_block_timing.needs_display_confirmation();
                #[cfg(not(target_os = "windows"))]
                let awaiting_receipt = false;
                if quick_redraw.needs_redraw(now, presentable, pending_work, animated, sample_retry) {
                    window.request_redraw();
                }
                event_loop_window_target.set_control_flow(ControlFlow::WaitUntil(
                    quick_redraw.poll_deadline(now, presentable && (pending_work || animated || sample_retry),
                        presentable && (awaiting_receipt || state.quick_visible_sample_query.pending.is_some()))));
            }
            _ => {}
        }
    });

    if let Err(error) = event_loop_result {
        let message = format!("legacy event loop returned an error: {error:#}");
        record_viewer_shutdown("legacy_event_loop_error", 1, &message);
        emit_viewer_shutdown();
        return Err(error.into());
    }
    if let Some(error) = fatal_error.lock().expect("quick preview event error mutex").take() {
        emit_viewer_shutdown();
        bail!("{error}");
    }

    record_viewer_shutdown(
        "legacy_event_loop_returned",
        0,
        "legacy event loop returned after an explicit shutdown request",
    );
    emit_viewer_shutdown();
    Ok(())
}

/// Runs the cache-generation process role without creating a renderer or event loop.
///
/// The launch contract is role-checked before its ACK is emitted. This keeps
/// Interactive, Preview, combined warmup, and PrepareViewer tasks in the
/// dedicated Viewer executable even if a caller addresses the wrong binary.

fn parse_launch_contract_args(provenance: &BackendProvenance) -> Result<Option<ViewerArgs>> {
 let args=std::env::args().skip(1).collect::<Vec<_>>();
 if args==["--help"] || args==["-h"] { println!("usage: litematica_native_viewer --launch-contract=<ABS_JSON>");return Ok(None); }
 if args==["--contract-info"] { println!("{}",serde_json::json!({"schema":VIEWER_LAUNCH_SCHEMA,"ackSchema":VIEWER_LAUNCH_ACK_SCHEMA,"renderCore":"legacy-wgpu","publicBeta":true,"acceptedTasks":["quick_preview","quick_gpu_context"],"quickPreview":{"task":"quick_preview","renderCore":"legacy-wgpu","buildMode":"fast_experimental","initialVisible":false}}));return Ok(None); }
 let [value]=args.as_slice() else { bail!("expected exactly one --launch-contract=<ABS_JSON>"); };
 let path=value.strip_prefix("--launch-contract=").map(PathBuf::from).context("expected --launch-contract=<ABS_JSON>")?;
 if !path.is_absolute() { bail!("launch contract path must be absolute"); }
 parse_launch_contract_path(provenance, &path).map(Some)
}

fn parse_launch_contract_path(provenance: &BackendProvenance, path: &Path) -> Result<ViewerArgs> {
 parse_launch_contract_path_inner(provenance, path, true)
}

fn parse_launch_contract_path_inner(provenance: &BackendProvenance, path: &Path, require_native_host: bool) -> Result<ViewerArgs> {
 let bytes=read_process_file(path)?;let hash=format!("{:x}",Sha256::digest(&bytes));
 let contract=ViewerLaunchContract::from_json_slice(&bytes).map_err(|error|anyhow!("invalid launch contract: {error}"))?;

 let (host, initial_visible, preload, gpu_context_only, resource_report, resource_decision, graphics_backend) = match &contract.task {
     ViewerTask::QuickPreview(task) => (task.host.clone(),task.initial_visible,task.preload,false,task.resource_report_path.clone(),task.resource_decision_path.clone(),task.graphics_backend),
     ViewerTask::QuickGpuContext(task) => (Some(task.host.clone()),false,false,true,None,None,task.graphics_backend),
     _ => bail!("the public Fast executable accepts only Quick tasks; protected tasks require the authenticated module loader"),
 };
 if require_native_host && host.is_none() { bail!("public quick preview requires a native child host"); }
 install_viewer_build_mode(ViewerBuildMode::FastExperimental).map_err(|error|anyhow!("build mode conflict: {error:?}"))?;
 let ack=ViewerLaunchAck{schema:VIEWER_LAUNCH_ACK_SCHEMA.to_owned(),contract_schema:VIEWER_LAUNCH_SCHEMA.to_owned(),request_id:contract.request_id.clone(),contract_sha256:hash.clone(),backend_git_sha:provenance.git_sha().to_owned(),backend_profile:provenance.profile().to_owned(),backend_exe_path:provenance.exe_path().to_path_buf(),backend_sha256:provenance.sha256().to_owned()};
 println!("{VIEWER_LAUNCH_ACK_PREFIX}{}",serde_json::to_string(&ack)?);std::io::stdout().flush()?;
 Ok(ViewerArgs{launch_request_id:contract.request_id,launch_contract_sha256:hash,input:(!gpu_context_only).then_some(contract.scene),chunk_size:32,probe_scene:false,prebuild_only:false,build_cache_and_warm:false,prepare_viewer_only:false,persistent_popup_presentation:true,initial_visible,auto_exit_seconds:None,prebuild_before_show:preload,cache_input:None,ready_file:None,cache_file:None,preview_output:None,build_workers:default_build_worker_count(),cpu_cache_max_chunks:DEFAULT_CPU_CACHE_BUDGET.max_chunks,cpu_cache_max_bytes:DEFAULT_CPU_CACHE_BUDGET.max_bytes,resident_max_chunks:DEFAULT_RESIDENT_BUDGET.max_chunks,resident_max_bytes:DEFAULT_RESIDENT_BUDGET.max_bytes,stress_limits:false,embed_parent_hwnd:None,quick_host:host,quick_gpu_context_only:gpu_context_only,graphics_backend,actual_graphics_backend:viewer_launch_contract::GraphicsApiPreference::Auto,quick_resource_report:resource_report,quick_resource_decision:resource_decision,preview_mode:false,preview_spin:false,display_mode:ViewerDisplayMode::FastExperimental,basic_lighting:true,basic_shadows:true,shadow_debug:false,shadow_debug_view:ShadowDebugViewMode::Off,shadow_force_test:false})
}

fn env_bool(name: &str) -> bool { matches!(std::env::var(name).ok().as_deref(),Some("1"|"true"|"on"|"yes")) }
fn hide_quick_preview_window(
    window: &NativeViewportWindow,
    host: Option<&viewer_launch_contract::QuickPreviewHostWindow>,
) -> Result<()> {
    #[cfg(any(target_os="windows",target_os="linux"))]
    if let Some(host) = host {
        return native_child::hide(window, host);
    }
    #[cfg(not(any(target_os="windows",target_os="linux")))]
    let _ = host;
    window.set_visible(false);
    Ok(())
}

#[cfg(test)]
mod quick_lifecycle_tests {
    use super::*;

    #[test]
    fn quick_redraw_completed_scene_is_idle_but_controls_and_receipts_still_poll() {
        let now = Instant::now();
        let mut redraw = QuickRedrawState::new();
        assert!(redraw.needs_redraw(now, true, false, false, false));
        redraw.presented(now);
        for elapsed in [0, 16, 40, 120_000] {
            assert!(!redraw.needs_redraw(now + Duration::from_millis(elapsed), true, false, false, false));
        }
        assert_eq!(redraw.poll_deadline(now, false, false), now + Duration::from_millis(40));
        assert_eq!(redraw.poll_deadline(now, false, true), now + Duration::from_millis(16));
    }

    #[test]
    fn quick_redraw_real_inputs_and_new_scene_resume_immediately_without_hidden_draws() {
        let now = Instant::now();
        let mut redraw = QuickRedrawState::new();
        redraw.presented(now);
        // Production input, resize, reset, expose and Show all invalidate the
        // same scheduler. An input does not wait for the async progress tick.
        for _ in 0..5 {
            redraw.request();
            assert!(redraw.needs_redraw(now, true, false, false, false));
            redraw.presented(now);
        }
        redraw.hide();
        assert!(!redraw.needs_redraw(now, false, true, true, true));
        redraw.request(); // New source / restored page: its first draw is fresh.
        assert!(!redraw.needs_redraw(now, false, true, true, true));
        assert!(redraw.needs_redraw(now, true, false, false, false));
        redraw.presented(now);
        assert!(!redraw.needs_redraw(now, true, false, false, false));
    }

    #[test]
    fn quick_redraw_async_animation_and_surface_retry_keep_bounded_progress() {
        let now = Instant::now();
        let mut redraw = QuickRedrawState::new();
        redraw.presented(now);
        for (pending, animated, sample) in [(true, false, false), (false, true, false), (false, false, true)] {
            assert!(!redraw.needs_redraw(now, true, pending, animated, sample));
            assert_eq!(redraw.poll_deadline(now, true, false), now + Duration::from_millis(16));
            assert!(redraw.needs_redraw(now + Duration::from_millis(16), true, pending, animated, sample));
            assert!(!redraw.needs_redraw(now + Duration::from_millis(16), false, pending, animated, sample));
        }
        let completed_at = now + Duration::from_millis(16);
        assert!(redraw.needs_redraw(completed_at, true, true, false, false));
        redraw.presented(completed_at); // The same loop consumes an async result.
        assert!(!redraw.needs_redraw(completed_at, true, true, false, false));
        assert!(!redraw.needs_redraw(completed_at + Duration::from_millis(40), true, false, false, false),
            "drained build/upload queues become idle instead of drawing another unchanged frame");
        redraw.retry(now); // Real Surface timeout/outdated, not a failed task.
        assert!(!redraw.needs_redraw(now, true, false, false, false));
        assert_eq!(redraw.poll_deadline(now, true, false), now + Duration::from_millis(40));
        assert!(redraw.needs_redraw(now + Duration::from_millis(40), true, false, false, false));
        redraw.hide(); // A delayed retry must not reopen a hidden/minimized page.
        assert!(!redraw.needs_redraw(now + Duration::from_millis(40), false, true, true, true));
    }

    #[test]
    fn quick_complete_upload_order_preserves_all_chunks_nearest_first() {
        let focus = ChunkKey { cx: 0, cy: 0, cz: 0 };
        let input = vec![ChunkKey { cx: 20_000, cy: 0, cz: 0 },
            ChunkKey { cx: 1, cy: 0, cz: 0 }, focus,
            ChunkKey { cx: -1, cy: 0, cz: 0 }];
        let order = complete_projection_upload_order(input.iter().copied(), focus);
        assert_eq!(order, vec![focus, input[3], input[1], input[0]]);
        assert_eq!(order.iter().copied().collect::<HashSet<_>>(), input.iter().copied().collect());
        let large = complete_projection_upload_order((0..65_536).map(|cx| ChunkKey { cx, cy: 0, cz: 0 }), focus);
        assert_eq!(large.len(), 65_536, "complete Fast scene must not be clipped by a residency cap");
        assert_eq!(large.last().unwrap().cx, 65_535);
    }

    #[test]
    fn quick_unknown_or_incomplete_estimate_never_enables_preload() {
        use viewer_launch_contract::QuickResourceAction;
        assert!(!quick_preload_allowed("unknown", true));
        assert!(!quick_preload_allowed("over_budget", true));
        assert!(!quick_preload_allowed("warning", false));
        assert!(quick_preload_allowed("normal", false));
        assert!(quick_preload_allowed("warning", true));
        assert!(quick_resource_action(true, "normal", false, None).is_err());
        assert!(quick_resource_action(true, "unknown", true, None).is_err());
        assert_eq!(quick_resource_action(false, "unknown", false, None).unwrap(), QuickResourceAction::Realtime);
    }
}


/// CPU-only Fast diagnosis: no Window, GPU admission proof or presentation receipt.
pub fn audit_render_cpu(arguments: &[String]) -> Result<()> {
    ensure!(arguments.len() == 1, "usage: render-cpu-audit --launch-contract=<ABS_JSON>");
    let path = arguments[0].strip_prefix("--launch-contract=")
        .context("render-cpu-audit requires --launch-contract=<ABS_JSON>")?;
    let provenance = crate::backend_provenance::collect_backend_provenance()?;
    let args = parse_launch_contract_path_inner(&provenance, Path::new(path), false)?;
    ensure!(args.display_mode == ViewerDisplayMode::FastExperimental
        && args.persistent_popup_presentation && !args.prebuild_only,
        "render-cpu-audit accepts only the independent QuickPreview contract; exact modes use nova-build-worker");
    ensure!(args.quick_host.is_none(), "CPU-only audit must not reference a live HWND");
    let source = args.input.as_deref().context("Fast CPU audit needs a real projection")?;
    let started = Instant::now();
    println!("[NOVA_RENDER_CPU_AUDIT] stage=start request_id={} mode=fast loading={} source={} source_bytes={} scope=CPU_only existing_scene_in_process=false os_file_cache=not_flushed existing_gpu_state=not_measured gpu_initialized=false presentation=not_executed",
        args.launch_request_id, if args.prebuild_before_show { "preloaded" } else { "realtime" }, source.display(), std::fs::metadata(source)?.len());
    let scene = load_scene(&args)?;
    ensure!(scene.texture_atlas.is_none(),
        "Fast unexpectedly prepared Full resources");
    let bootstrap_elapsed = started.elapsed();
    let index = Arc::new(scene.chunk_scene_index.context("Fast input did not produce an actual scene index")?);
    let camera = OrbitCamera::from_bounds(scene.bounds, 1280, 800);
    let config = ChunkStreamingConfig::default();
    let plan = ChunkResidencyPlan::build(&index, &camera, &config, 0, false, false);
    let keys = if args.prebuild_before_show {
        index.chunk_entries().iter().map(|entry| entry.key).collect::<Vec<_>>()
    } else {
        // Same fitted-camera demand and configured residency bound as the live
        // Fast queue; subsequent camera moves remain a manual runtime check.
        plan.upload_order.iter().copied().take(args.resident_max_chunks).collect::<Vec<_>>()
    };
    let source_blocks = index.chunk_entries().iter().map(|entry| entry.non_air_blocks as u64).sum::<u64>();
    let mut cache = ChunkMeshCpuCache::new(args.cpu_cache_max_chunks, args.cpu_cache_max_bytes);
    let mut completed_keys = HashSet::new();
    let mut vertices = 0_usize;
    let mut indices = 0_usize;
    for mesh in scene.bootstrap_meshes {
        vertices = vertices.saturating_add(mesh.vertex_count());
        indices = indices.saturating_add(mesh.index_count());
        completed_keys.insert(mesh.key);
        cache.insert(mesh, 0);
    }
    let bootstrap_count = completed_keys.len();
    let pending = keys.iter().copied().filter(|key| !completed_keys.contains(key)).collect::<Vec<_>>();
    println!("[NOVA_RENDER_CPU_AUDIT] stage=index_and_bootstrap executions=1 source_non_air_blocks={} total_chunks={} bootstrap_chunks={} scope_chunks={} bootstrap_cpu_meshes_reused={} elapsed_ms={} wait=read_decompress_parse_initial_mesh full_resource_initializations=0",
        source_blocks, index.chunk_count(), bootstrap_count, keys.len(), bootstrap_count, bootstrap_elapsed.as_millis());
    let trace = Arc::new(TraceCounters::default());
    let worker_count = args.build_workers.max(1);
    let (request_tx, result_rx, workers) = spawn_build_worker_pool(Arc::clone(&index), Arc::new(scene.palette_colors), worker_count, Arc::clone(&trace), None);
    let mesh_started = Instant::now();
    let mut result_wait = Duration::ZERO;
    let mut batch_id = 0_u64;
    let mut empty_meshes = 0_usize;
    // Keep the production Fast in-flight bound (workers*16). Receive a wave
    // of asynchronous batch results, never per-cell read/upload/wait lockstep.
    for wave in pending.chunks(worker_count.saturating_mul(16).max(1)) {
        for batch in wave.chunks(16) {
            batch_id += 1;
            request_tx.send(BuildWorkerRequest { batch_id, keys: batch.to_vec(), priority: QueuePriority::Target, enqueued_at: Instant::now() })?;
        }
        for _ in 0..wave.len() {
            let wait = Instant::now();
            let result = result_rx.recv().context("Fast CPU worker results disconnected")?;
            result_wait += wait.elapsed();
            if let Some(error) = result.error { bail!("Fast CPU worker failed: {error}"); }
            ensure!(completed_keys.insert(result.key), "Fast CPU build repeated a previously prepared chunk");
            if let Some(mesh) = result.mesh {
                vertices = vertices.saturating_add(mesh.vertex_count());
                indices = indices.saturating_add(mesh.index_count());
                cache.insert(mesh, 0);
            } else { empty_meshes += 1; }
        }
        cache.prune(&HashSet::new(), 0);
    }
    drop(request_tx);
    for worker in workers { worker.join().map_err(|_| anyhow!("Fast CPU worker panicked"))?; }
    let missing = keys.iter().filter(|key| !completed_keys.contains(key)).count();
    ensure!(missing == 0, "Fast CPU audit omitted {missing} requested chunks");
    println!("[NOVA_RENDER_CPU_AUDIT] stage=complete mode=fast loading={} source_non_air_blocks={} total_chunks={} scope_chunks={} completed_unique_chunks={} pending_mesh_executions={} empty_meshes={} vertices={} indices={} batches={} worker_count={} bounded_in_flight={} cpu_cache_chunks={} cpu_cache_bytes={} mesh_elapsed_ms={} worker_result_wait_ms={} worker_busy_ms_cumulative={} worker_queue_wait_ms_cumulative={} total_elapsed_ms={} GPU_uploads=not_executed UI=not_executed first_display=not_measured",
        if args.prebuild_before_show { "preloaded" } else { "realtime" }, source_blocks, index.chunk_count(), keys.len(), completed_keys.len(), pending.len(), empty_meshes, vertices, indices, batch_id, worker_count, worker_count.saturating_mul(16), cache.len(), cache.current_bytes, mesh_started.elapsed().as_millis(), result_wait.as_millis(), trace.worker_busy_ms.load(Ordering::Relaxed), trace.build_dequeue_ms.load(Ordering::Relaxed), started.elapsed().as_millis());
    Ok(())
}
