//! Composition uses the resident Fast geometry buffers and the existing native
//! overlay pass. No extra window, renderer, scene parser or per-instance mesh.
use super::*;
use crate::composition_editor::{EditorRuntime, Ray};

pub(super) struct InstanceUniform {
    pub resource_id: String,
    pub model: Mat4,
    pub mirrored: bool,
    camera: wgpu::Buffer,
    pub camera_group: wgpu::BindGroup,
    shadow: wgpu::Buffer,
    pub shadow_group: wgpu::BindGroup,
}
pub(super) struct EditorGpuState {
    pub runtime: EditorRuntime,
    pub instances: BTreeMap<String, InstanceUniform>,
    pub overlay: LabelOverlay,
}
impl EditorGpuState {
    pub fn draws(&self, key: ChunkKey, chunk_size: u32) -> impl Iterator<Item = &InstanceUniform> {
        let owner = self
            .runtime
            .resource_for_chunk([key.cx, key.cy, key.cz], chunk_size as i32);
        self.runtime
            .scene
            .instances
            .iter()
            .filter(move |p| p.visible && Some(p.resource_id.as_str()) == owner)
            .filter_map(|p| self.instances.get(&p.id))
    }
}
fn uniform(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &str,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: std::mem::size_of::<CameraUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    (buffer, group)
}
impl ViewerState {
    pub(super) fn configure_editor(&mut self, args: &ViewerArgs) -> Result<()> {
        self.editor = None;
        self.editor_cursor = None;
        self.editor_cycle = false;
        if let Some(path) = &args.composition_editor_path {
            let runtime = EditorRuntime::load(path)?;
            self.editor = Some(EditorGpuState {
                runtime,
                instances: BTreeMap::new(),
                overlay: LabelOverlay::new(
                    &self.device,
                    self.config.format,
                    wgpu::TextureFormat::Depth24Plus,
                ),
            });
            self.refresh_editor(true)?;
        }
        Ok(())
    }
    fn editor_handle_length(&self) -> f32 {
        let center = self
            .editor
            .as_ref()
            .and_then(|v| v.runtime.selected().and_then(|i| v.runtime.gizmo_origin(i)))
            .unwrap_or(self.camera.target);
        let distance = self
            .camera
            .eye()
            .distance(center)
            .max(self.camera.near * 4.);
        (2. * distance * (self.camera.fovy_radians * 0.5).tan() / self.config.height.max(1) as f32
            * 76.)
            .max(0.08)
    }
    pub(super) fn refresh_editor(&mut self, fit: bool) -> Result<()> {
        let Some(editor) = &mut self.editor else {
            return Ok(());
        };
        if let Some((min, max)) = editor.runtime.scene_bounds() {
            self.scene_bounds = SceneBounds {
                center: (min + max) * 0.5,
                radius: ((max - min).length() * 0.5).max(2.),
            };
        }
        if fit {
            self.camera =
                OrbitCamera::from_bounds(self.scene_bounds, self.config.width, self.config.height);
        }
        self.camera.set_free_mode(false);
        self.camera.set_free_input_mode("drag");
        let light = LightingConfig::from_args(
            self.editor_lighting.basic.enabled,
            self.editor_lighting.shadow.enabled,
            self.editor_lighting.shadow.debug_view,
            self.editor_lighting.shadow.force_test,
            self.scene_bounds,
        );
        self.editor_lighting = light;
        self.shadow_resources.config = light.shadow();
        self.queue.write_buffer(
            &self.lighting_buffer,
            0,
            bytemuck::bytes_of(&LightingUniform::from_config(light)),
        );
        let layout = self.render_pipeline_solid.get_bind_group_layout(0);
        let ids = editor
            .runtime
            .scene
            .instances
            .iter()
            .map(|p| p.id.as_str())
            .collect::<HashSet<_>>();
        editor.instances.retain(|id, _| ids.contains(id.as_str()));
        for (index, part) in editor.runtime.scene.instances.iter().enumerate() {
            let model = editor
                .runtime
                .matrix(index)
                .context("组合实例没有几何资源")?;
            let gpu = editor.instances.entry(part.id.clone()).or_insert_with(|| {
                let (camera, camera_group) =
                    uniform(&self.device, &layout, "composition_instance_camera");
                let (shadow, shadow_group) =
                    uniform(&self.device, &layout, "composition_instance_shadow");
                InstanceUniform {
                    resource_id: part.resource_id.clone(),
                    model,
                    mirrored: model.determinant() < 0.,
                    camera,
                    camera_group,
                    shadow,
                    shadow_group,
                }
            });
            ensure!(
                gpu.resource_id == part.resource_id,
                "已存在实例的几何身份改变，必须更新编辑场景"
            );
            gpu.model = model;
            gpu.mirrored = model.determinant() < 0.;
        }
        self.write_camera();
        Ok(())
    }
    pub(super) fn write_editor_cameras(&self) {
        let Some(editor) = &self.editor else { return };
        let shadow_matrix = self
            .shadow_resources
            .config
            .light_camera
            .map(|v| v.view_proj)
            .unwrap_or(Mat4::IDENTITY);
        for instance in editor.instances.values() {
            let mut camera = CameraUniform::from_matrix_and_eye(
                self.camera.view_proj_matrix(),
                self.camera.eye(),
            );
            camera.model = instance.model.to_cols_array_2d();
            camera.layer_range = self.layer_range;
            self.queue
                .write_buffer(&instance.camera, 0, bytemuck::bytes_of(&camera));
            camera.view_proj = shadow_matrix.to_cols_array_2d();
            self.queue
                .write_buffer(&instance.shadow, 0, bytemuck::bytes_of(&camera));
        }
    }
    pub(super) fn prepare_editor_overlay(&mut self) {
        let length = self.editor_handle_length();
        let Some(editor) = &mut self.editor else {
            return;
        };
        let lines = editor.runtime.lines(length);
        let mut vertices = Vec::with_capacity(lines.len() * 6);
        let eye = self.camera.eye();
        for (a, b, color) in lines {
            let center = (a + b) * 0.5;
            let side = (b - a).cross(eye - center).normalize_or_zero();
            if side.length_squared() < 0.1 {
                continue;
            }
            let thickness = (eye.distance(center) * 2. * (self.camera.fovy_radians * 0.5).tan()
                / self.config.height.max(1) as f32)
                .max(0.0001);
            let side = side * thickness;
            for p in [a - side, b - side, b + side, a - side, b + side, a + side] {
                vertices.push(OverlayVertex {
                    position: p.to_array(),
                    color,
                });
            }
        }
        let camera = CameraUniform::from_matrix_and_eye(self.camera.view_proj_matrix(), eye);
        self.queue.write_buffer(
            &editor.overlay.camera_buffer,
            0,
            bytemuck::bytes_of(&camera),
        );
        editor.overlay.vertex_count = vertices.len() as u32;
        if vertices.is_empty() {
            return;
        }
        if vertices.len() > editor.overlay.vertex_capacity {
            editor.overlay.vertex_buffer = Some(self.device.create_buffer_init(
                &wgpu::util::BufferInitDescriptor {
                    label: Some("composition_native_gizmo_vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                },
            ));
            editor.overlay.vertex_capacity = vertices.len();
        } else if let Some(buffer) = &editor.overlay.vertex_buffer {
            self.queue
                .write_buffer(buffer, 0, bytemuck::cast_slice(&vertices));
        }
        editor.runtime.dirty = false;
    }
    fn editor_ray(&self) -> Option<Ray> {
        let p = self.editor_cursor?;
        Ray::from_viewport(
            self.camera.view_proj_matrix(),
            [f64::from(p.x), f64::from(p.y)],
            [self.config.width, self.config.height],
        )
    }
    pub(super) fn editor_mouse(
        &mut self,
        state: ElementState,
        button: MouseButton,
    ) -> Result<bool> {
        if self.editor.is_none() || button != MouseButton::Left {
            return Ok(false);
        }
        self.drag_mode = None;
        if state == ElementState::Released {
            if self.editor.as_mut().unwrap().runtime.finish()? {
                self.refresh_editor(false)?;
            }
            return Ok(true);
        }
        let Some(ray) = self.editor_ray() else {
            return Ok(true);
        };
        let length = self.editor_handle_length();
        let editor = self.editor.as_ref().unwrap();
        let size = self
            .chunk_scene_index
            .as_ref()
            .map_or(32, |i| i.chunk_size()) as i32;
        let hits = editor.runtime.pick(
            ray,
            self.resident_chunks.iter().filter_map(|(key, chunk)| {
                editor
                    .runtime
                    .resource_for_chunk([key.cx, key.cy, key.cz], size)
                    .map(|id| {
                        (
                            id,
                            chunk.editor_triangles.as_slice(),
                            chunk.editor_pick_bounds,
                        )
                    })
            }),
        );
        self.editor
            .as_mut()
            .unwrap()
            .runtime
            .press(ray, &hits, length, self.editor_cycle)?;
        self.refresh_editor(false)?;
        Ok(true)
    }
    pub(super) fn editor_motion(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        self.editor_cursor = Some(Vec2::new(position.x as f32, position.y as f32));
        if self
            .editor
            .as_ref()
            .is_none_or(|v| !v.runtime.gesture_active())
        {
            return Ok(false);
        }
        let Some(ray) = self.editor_ray() else {
            return Ok(true);
        };
        if self.editor.as_mut().unwrap().runtime.motion(ray) {
            self.refresh_editor(false)?;
        }
        Ok(true)
    }
    pub(super) fn cancel_editor_gesture(&mut self) {
        let result = self.editor.as_mut().map(|v| v.runtime.cancel()).transpose();
        match result {
            Ok(Some(true)) => {
                if let Err(e) = self.refresh_editor(false) {
                    self.presentation_error = Some(format!("组合编辑恢复失败：{e:#}"));
                }
            }
            Err(e) => self.presentation_error = Some(format!("组合编辑取消失败：{e:#}")),
            _ => {}
        }
    }
}
