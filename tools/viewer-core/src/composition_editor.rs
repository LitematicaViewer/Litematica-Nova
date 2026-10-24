//! Native viewport editing. All rays, grid snapping and gesture transactions
//! stay in the Rust data plane; the web UI only selects tools and displays state.
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};
use serde::Serialize;
use std::{fs, io::Write, path::Path};
use viewer_launch_contract::{
    CompositionEditorBounds as Bounds, CompositionEditorScene as Scene,
    CompositionEditorTransform as Transform,
};

pub const EVENT_SCHEMA: &str = "nova.composition.editor-event.v1";

#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}
impl Ray {
    pub fn from_viewport(view_proj: Mat4, position: [f64; 2], extent: [u32; 2]) -> Option<Self> {
        if extent.contains(&0) {
            return None;
        }
        let inverse = view_proj.inverse();
        let x = (position[0] / f64::from(extent[0]) * 2. - 1.) as f32;
        let y = (1. - position[1] / f64::from(extent[1]) * 2.) as f32;
        let near = inverse.project_point3(Vec3::new(x, y, 0.));
        let far = inverse.project_point3(Vec3::new(x, y, 1.));
        let direction = (far - near).normalize_or_zero();
        (near.is_finite() && direction.is_finite() && direction.length_squared() > 0.5).then_some(
            Self {
                origin: near,
                direction,
            },
        )
    }
    pub fn transformed(self, matrix: Mat4) -> Self {
        Self {
            origin: matrix.transform_point3(self.origin),
            direction: matrix.transform_vector3(self.direction).normalize_or_zero(),
        }
    }
    pub fn triangle(self, points: [Vec3; 3]) -> Option<f32> {
        // Two-sided: mirrored instances use the same source triangles. The GPU
        // uses reversed index buffers for their negative determinant.
        let edge1 = points[1] - points[0];
        let edge2 = points[2] - points[0];
        let p = self.direction.cross(edge2);
        let det = edge1.dot(p);
        if det.abs() < 1e-7 {
            return None;
        }
        let inverse = 1. / det;
        let t = self.origin - points[0];
        let u = t.dot(p) * inverse;
        if !(0. ..=1.).contains(&u) {
            return None;
        }
        let q = t.cross(edge1);
        let v = self.direction.dot(q) * inverse;
        if v < 0. || u + v > 1. {
            return None;
        }
        let distance = edge2.dot(q) * inverse;
        (distance >= 0. && distance.is_finite()).then_some(distance)
    }
    pub fn intersects_box(self, min: Vec3, max: Vec3) -> bool {
        let mut near = 0f32;
        let mut far = f32::INFINITY;
        for a in 0..3 {
            if self.direction[a].abs() < 1e-8 {
                if self.origin[a] < min[a] || self.origin[a] > max[a] {
                    return false;
                }
            } else {
                let t0 = (min[a] - self.origin[a]) / self.direction[a];
                let t1 = (max[a] - self.origin[a]) / self.direction[a];
                near = near.max(t0.min(t1));
                far = far.min(t0.max(t1));
                if near > far {
                    return false;
                }
            }
        }
        true
    }
    fn point_distance(self, point: Vec3) -> Option<f32> {
        let along = (point - self.origin).dot(self.direction);
        (along > 0.).then_some((self.origin + along * self.direction).distance(point))
    }
    fn axis_parameter(self, origin: Vec3, axis: usize) -> Option<f32> {
        let a = Vec3::AXES[axis];
        let ad = a.dot(self.direction);
        let denominator = 1. - ad * ad;
        if denominator < 1e-5 {
            return None;
        }
        let delta = self.origin - origin;
        Some((delta.dot(a) - delta.dot(self.direction) * ad) / denominator)
    }
    fn plane_y(self, y: f32) -> Option<Vec3> {
        if self.direction.y.abs() < 1e-6 {
            return None;
        }
        let t = (y - self.origin.y) / self.direction.y;
        (t >= 0.).then_some(self.origin + t * self.direction)
    }
}

pub fn model_matrix(transform: &Transform, base_offset: [i32; 3]) -> Mat4 {
    let origin = Vec3::from_array(transform.origin.map(|v| v as f32));
    let offset = Vec3::from_array(transform.offset.map(|v| v as f32));
    let mirror = Vec3::new(
        if transform.mirror_x { -1. } else { 1. },
        1.,
        if transform.mirror_z { -1. } else { 1. },
    );
    // CompositionTransform's positive 90 degrees maps X to +Z.
    Mat4::from_translation(offset + origin)
        * Mat4::from_rotation_y(-(transform.rotation as f32).to_radians())
        * Mat4::from_scale(mirror)
        * Mat4::from_translation(-origin - Vec3::from_array(base_offset.map(|v| v as f32)))
}
pub fn world_bounds(bounds: &Bounds, transform: Mat4) -> (Vec3, Vec3) {
    let min = Vec3::from_array(bounds.min.map(|v| v as f32));
    let max = Vec3::from_array(bounds.max.map(|v| v as f32 + 1.));
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for bits in 0..8 {
        let p = Vec3::new(
            if bits & 1 == 0 { min.x } else { max.x },
            if bits & 2 == 0 { min.y } else { max.y },
            if bits & 4 == 0 { min.z } else { max.z },
        );
        let p = transform.transform_point3(p);
        lo = lo.min(p);
        hi = hi.max(p);
    }
    (lo, hi)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Event<'a> {
    schema: &'static str,
    project_id: &'a str,
    scene_key: &'a str,
    generation: u64,
    sequence: u64,
    selected_instance_id: &'a Option<String>,
    committed: bool,
    cancelled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    instance_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transform: Option<&'a Transform>,
    split_boxes: &'a [viewer_launch_contract::CompositionEditorBox],
}
#[derive(Clone)]
enum Gesture {
    Translate {
        index: usize,
        axis: usize,
        origin: Vec3,
        initial: f32,
        before: Transform,
    },
    Rotate {
        index: usize,
        center: Vec3,
        angle: f32,
        before: Transform,
    },
    Split {
        index: usize,
        axis: usize,
        maximum: bool,
        origin: Vec3,
        initial: f32,
        before: Bounds,
    },
}
pub struct EditorRuntime {
    pub scene: Scene,
    sequence: u64,
    gesture: Option<Gesture>,
    pending_commit: Option<u64>,
    pub dirty: bool,
}
impl EditorRuntime {
    pub fn load(path: &Path) -> Result<Self> {
        let metadata = fs::metadata(path)?;
        ensure!(metadata.len() <= 16 * 1024 * 1024, "组合编辑描述超过容量");
        let scene: Scene = serde_json::from_slice(&fs::read(path)?)?;
        scene.validate().map_err(anyhow::Error::msg)?;
        let sequence = scene.acknowledged_event_sequence.unwrap_or(0);
        Ok(Self {
            scene,
            sequence,
            gesture: None,
            pending_commit: None,
            dirty: true,
        })
    }
    pub fn update(&mut self, scene: Scene) -> Result<()> {
        scene.validate().map_err(anyhow::Error::msg)?;
        ensure!(
            scene.project_id == self.scene.project_id
                && scene.scene_key == self.scene.scene_key
                && scene.resources == self.scene.resources,
            "编辑场景几何身份发生变化，必须明确更新编辑场景"
        );
        ensure!(
            scene.generation >= self.scene.generation,
            "已忽略过期组合编辑状态"
        );
        // A gesture has one durable event slot. Until the backend acknowledges
        // its CAS transaction, neither a selection update nor a page-hide may
        // overwrite that event or replace its unacknowledged preview state.
        if self.pending_commit.is_some_and(|sequence| {
            scene
                .acknowledged_event_sequence
                .is_none_or(|ack| ack < sequence)
        }) {
            return Ok(());
        }
        self.pending_commit = None;
        if scene.event_path != self.scene.event_path {
            self.sequence = scene.acknowledged_event_sequence.unwrap_or(0);
        } else if let Some(sequence) = scene.acknowledged_event_sequence {
            self.sequence = self.sequence.max(sequence);
        }
        self.gesture = None;
        self.scene = scene;
        self.dirty = true;
        Ok(())
    }
    pub fn matrix(&self, instance: usize) -> Option<Mat4> {
        let part = self.scene.instances.get(instance)?;
        let resource = self
            .scene
            .resources
            .iter()
            .find(|v| v.id == part.resource_id)?;
        Some(model_matrix(&part.transform, resource.base_offset))
    }
    pub fn resource_for_chunk(&self, key: [i32; 3], chunk_size: i32) -> Option<&str> {
        // Source packing is chunk-aligned with an empty chunk between sources.
        // Ownership is assigned from the source's declared cells, not a mesh
        // center: edge quads may extend outside a chunk and still belong to it.
        self.scene
            .resources
            .iter()
            .find(|resource| {
                (0..3).all(|a| {
                    let min = (i64::from(resource.bounds.min[a])
                        + i64::from(resource.base_offset[a]))
                    .div_euclid(i64::from(chunk_size));
                    let max = (i64::from(resource.bounds.max[a])
                        + i64::from(resource.base_offset[a]))
                    .div_euclid(i64::from(chunk_size));
                    i64::from(key[a]) >= min && i64::from(key[a]) <= max
                })
            })
            .map(|v| v.id.as_str())
    }
    pub fn instances_for_resource<'a>(
        &'a self,
        resource_id: &'a str,
    ) -> impl Iterator<Item = usize> + 'a {
        self.scene
            .instances
            .iter()
            .enumerate()
            .filter(move |(_, v)| v.visible && v.resource_id == resource_id)
            .map(|(i, _)| i)
    }
    pub fn pick<'a>(
        &self,
        ray: Ray,
        geometry: impl Iterator<Item = (&'a str, &'a [[Vec3; 3]], (Vec3, Vec3))>,
    ) -> Vec<(usize, f32)> {
        let mut hits: Vec<(usize, f32)> = Vec::new();
        for (resource, triangles, bounds) in geometry {
            for index in self.instances_for_resource(resource) {
                let matrix = self.matrix(index).expect("validated resource");
                let local = ray.transformed(matrix.inverse());
                if !local.intersects_box(bounds.0, bounds.1) {
                    continue;
                }
                let distance = triangles
                    .iter()
                    .filter_map(|t| local.triangle(*t))
                    .min_by(f32::total_cmp);
                if let Some(distance) = distance {
                    if let Some(hit) = hits.iter_mut().find(|(i, _)| *i == index) {
                        if distance < hit.1 {
                            hit.1 = distance;
                        }
                    } else {
                        hits.push((index, distance));
                    }
                }
            }
        }
        hits.sort_by(|a, b| {
            a.1.total_cmp(&b.1).then_with(|| {
                self.scene.instances[a.0]
                    .id
                    .cmp(&self.scene.instances[b.0].id)
            })
        });
        hits
    }
    pub fn center(&self, instance: usize) -> Option<Vec3> {
        let part = self.scene.instances.get(instance)?;
        let resource = self
            .scene
            .resources
            .iter()
            .find(|v| v.id == part.resource_id)?;
        let transform = model_matrix(&part.transform, [0; 3]);
        let (lo, hi) = world_bounds(&resource.bounds, transform);
        Some((lo + hi) * 0.5)
    }
    pub fn gizmo_origin(&self, instance: usize) -> Option<Vec3> {
        if matches!(self.scene.tool.as_str(), "rotate" | "mirror_x" | "mirror_z") {
            let transform = &self.scene.instances.get(instance)?.transform;
            Some(Vec3::from_array(std::array::from_fn(|axis| {
                transform.offset[axis] as f32 + transform.origin[axis] as f32
            })))
        } else {
            self.center(instance)
        }
    }
    pub fn scene_bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut all: Option<(Vec3, Vec3)> = None;
        for part in self.scene.instances.iter().filter(|p| p.visible) {
            let resource = self
                .scene
                .resources
                .iter()
                .find(|v| v.id == part.resource_id)?;
            let (lo, hi) = world_bounds(&resource.bounds, model_matrix(&part.transform, [0; 3]));
            all = Some(match all {
                Some((a, b)) => (lo.min(a), hi.max(b)),
                None => (lo, hi),
            });
        }
        all
    }
    pub fn selected(&self) -> Option<usize> {
        self.scene
            .selected_instance_id
            .as_ref()
            .and_then(|id| self.scene.instances.iter().position(|p| &p.id == id))
    }
    pub fn gesture_active(&self) -> bool {
        self.gesture.is_some()
    }
    pub fn press(
        &mut self,
        ray: Ray,
        hits: &[(usize, f32)],
        handle_length: f32,
        cycle: bool,
    ) -> Result<()> {
        if self.pending_commit.is_some() {
            return Ok(());
        }
        if self.scene.tool == "split" {
            let threshold = handle_length * 0.13;
            let mut closest = None;
            for (index, b) in self.scene.split_boxes.iter().enumerate() {
                let center = Vec3::from_array(std::array::from_fn(|a| {
                    (b.bounds.min[a] as f32 + b.bounds.max[a] as f32 + 1.) * 0.5
                }));
                for axis in 0..3 {
                    for maximum in [false, true] {
                        let mut p = center;
                        p[axis] = if maximum {
                            b.bounds.max[axis] as f32 + 1.
                        } else {
                            b.bounds.min[axis] as f32
                        };
                        if let Some(distance) = ray.point_distance(p).filter(|d| *d < threshold) {
                            if closest
                                .as_ref()
                                .is_none_or(|(_, _, _, _, _, d)| distance < *d)
                            {
                                closest = Some((
                                    index,
                                    axis,
                                    maximum,
                                    p,
                                    ray.axis_parameter(p, axis),
                                    distance,
                                ));
                            }
                        }
                    }
                }
            }
            if let Some((index, axis, maximum, origin, Some(initial), _)) = closest {
                self.gesture = Some(Gesture::Split {
                    index,
                    axis,
                    maximum,
                    origin,
                    initial,
                    before: self.scene.split_boxes[index].bounds.clone(),
                });
                return Ok(());
            }
        }
        if let Some(index) = self.selected().filter(|i| !self.scene.instances[*i].locked) {
            let center = self
                .gizmo_origin(index)
                .expect("validated selected instance");
            let threshold = handle_length * 0.13;
            if self.scene.tool == "translate" {
                let axis = (0..3)
                    .filter_map(|axis| {
                        ray.point_distance(center + Vec3::AXES[axis] * handle_length)
                            .filter(|v| *v < threshold)
                            .map(|d| (axis, d))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|v| v.0);
                if let Some(axis) = axis {
                    if let Some(initial) = ray.axis_parameter(center, axis) {
                        self.gesture = Some(Gesture::Translate {
                            index,
                            axis,
                            origin: center,
                            initial,
                            before: self.scene.instances[index].transform.clone(),
                        });
                        return Ok(());
                    }
                }
            } else if self.scene.tool == "rotate" {
                if let Some(point) = ray.plane_y(center.y) {
                    let delta = point - center;
                    let radius = Vec3::new(delta.x, 0., delta.z).length();
                    if (radius - handle_length).abs() < threshold {
                        self.gesture = Some(Gesture::Rotate {
                            index,
                            center,
                            angle: delta.z.atan2(delta.x),
                            before: self.scene.instances[index].transform.clone(),
                        });
                        return Ok(());
                    }
                }
            } else if matches!(self.scene.tool.as_str(), "mirror_x" | "mirror_z") {
                let axis = if self.scene.tool == "mirror_x" { 0 } else { 2 };
                if ray
                    .point_distance(center + Vec3::AXES[axis] * handle_length)
                    .is_some_and(|d| d < threshold)
                {
                    if axis == 0 {
                        self.scene.instances[index].transform.mirror_x ^= true;
                    } else {
                        self.scene.instances[index].transform.mirror_z ^= true;
                    }
                    self.dirty = true;
                    return self.publish(true, false, Some(index));
                }
            }
        }
        let chosen = if cycle {
            let current = self.selected();
            hits.iter()
                .position(|(i, _)| Some(*i) == current)
                .and_then(|i| hits.get((i + 1) % hits.len().max(1)))
                .or_else(|| hits.first())
        } else {
            hits.first()
        };
        self.scene.selected_instance_id = chosen.map(|(i, _)| self.scene.instances[*i].id.clone());
        self.dirty = true;
        self.publish(false, false, None)
    }
    pub fn motion(&mut self, ray: Ray) -> bool {
        let Some(gesture) = self.gesture.clone() else {
            return false;
        };
        match gesture {
            Gesture::Translate {
                index,
                axis,
                origin,
                initial,
                before,
            } => {
                let Some(value) = ray.axis_parameter(origin, axis) else {
                    return false;
                };
                let delta = (value - initial).round();
                if !delta.is_finite() || delta.abs() > i32::MAX as f32 {
                    return false;
                }
                let Some(value) = before.offset[axis].checked_add(delta as i32) else {
                    return false;
                };
                self.scene.instances[index].transform = before;
                self.scene.instances[index].transform.offset[axis] = value;
            }
            Gesture::Rotate {
                index,
                center,
                angle,
                before,
            } => {
                let Some(point) = ray.plane_y(center.y) else {
                    return false;
                };
                let delta = point - center;
                let turns =
                    ((delta.z.atan2(delta.x) - angle) / std::f32::consts::FRAC_PI_2).round() as i32;
                self.scene.instances[index].transform = before.clone();
                self.scene.instances[index].transform.rotation =
                    (i32::from(before.rotation) + turns * 90).rem_euclid(360) as u16;
            }
            Gesture::Split {
                index,
                axis,
                maximum,
                origin,
                initial,
                before,
            } => {
                let Some(value) = ray.axis_parameter(origin, axis) else {
                    return false;
                };
                let delta = (value - initial).round();
                if !delta.is_finite() || delta.abs() > i32::MAX as f32 {
                    return false;
                }
                let b = &mut self.scene.split_boxes[index].bounds;
                *b = before.clone();
                if maximum {
                    if let Some(value) = before.max[axis].checked_add(delta as i32) {
                        b.max[axis] = value.max(b.min[axis]);
                    }
                } else if let Some(value) = before.min[axis].checked_add(delta as i32) {
                    b.min[axis] = value.min(b.max[axis]);
                }
            }
        }
        self.dirty = true;
        true
    }
    pub fn finish(&mut self) -> Result<bool> {
        let Some(gesture) = self.gesture.take() else {
            return Ok(false);
        };
        let index = match gesture {
            Gesture::Translate { index, .. } | Gesture::Rotate { index, .. } => Some(index),
            Gesture::Split { .. } => None,
        };
        self.publish(true, false, index)?;
        self.dirty = true;
        Ok(true)
    }
    pub fn cancel(&mut self) -> Result<bool> {
        let Some(gesture) = self.gesture.take() else {
            return Ok(false);
        };
        match gesture {
            Gesture::Translate { index, before, .. } | Gesture::Rotate { index, before, .. } => {
                self.scene.instances[index].transform = before
            }
            Gesture::Split { index, before, .. } => self.scene.split_boxes[index].bounds = before,
        }
        self.publish(false, true, None)?;
        self.dirty = true;
        Ok(true)
    }
    fn publish(&mut self, committed: bool, cancelled: bool, index: Option<usize>) -> Result<()> {
        ensure!(self.pending_commit.is_none(), "组合编辑提交尚未确认");
        self.sequence = self.sequence.checked_add(1).context("编辑事件序号溢出")?;
        let part = index.and_then(|i| self.scene.instances.get(i));
        let event = Event {
            schema: EVENT_SCHEMA,
            project_id: &self.scene.project_id,
            scene_key: &self.scene.scene_key,
            generation: self.scene.generation,
            sequence: self.sequence,
            selected_instance_id: &self.scene.selected_instance_id,
            committed,
            cancelled,
            instance_id: part.map(|v| v.id.as_str()),
            transform: part.map(|v| &v.transform),
            split_boxes: &self.scene.split_boxes,
        };
        let parent = self.scene.event_path.parent().context("编辑回执目录无效")?;
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&serde_json::to_vec(&event)?)?;
        temporary.flush()?;
        temporary
            .persist(&self.scene.event_path)
            .map_err(|e| e.error)?;
        if committed {
            self.pending_commit = Some(self.sequence);
        }
        Ok(())
    }
    pub fn lines(&self, handle_length: f32) -> Vec<(Vec3, Vec3, [f32; 4])> {
        let mut lines = Vec::new();
        if let Some(index) = self.selected() {
            let part = &self.scene.instances[index];
            if let Some(resource) = self
                .scene
                .resources
                .iter()
                .find(|v| v.id == part.resource_id)
            {
                let (min, max) =
                    world_bounds(&resource.bounds, model_matrix(&part.transform, [0; 3]));
                wire_box(
                    &mut lines,
                    min,
                    max,
                    if part.locked {
                        [0.55, 0.55, 0.55, 1.]
                    } else {
                        [0.3, 0.85, 1., 1.]
                    },
                );
            }
            if !part.locked {
                if let Some(center) = self.gizmo_origin(index) {
                    if self.scene.tool == "rotate" {
                        for step in 0..64 {
                            let a = step as f32 * std::f32::consts::TAU / 64.;
                            let b = (step + 1) as f32 * std::f32::consts::TAU / 64.;
                            lines.push((
                                center + Vec3::new(a.cos(), 0., a.sin()) * handle_length,
                                center + Vec3::new(b.cos(), 0., b.sin()) * handle_length,
                                [0.3, 0.9, 0.4, 1.],
                            ));
                        }
                    } else if matches!(
                        self.scene.tool.as_str(),
                        "translate" | "mirror_x" | "mirror_z"
                    ) {
                        for axis in 0..3 {
                            if self.scene.tool == "mirror_x" && axis != 0
                                || self.scene.tool == "mirror_z" && axis != 2
                            {
                                continue;
                            }
                            let color = [
                                [1., 0.25, 0.2, 1.],
                                [0.3, 0.95, 0.3, 1.],
                                [0.3, 0.5, 1., 1.],
                            ][axis];
                            let end = center + Vec3::AXES[axis] * handle_length;
                            lines.push((center, end, color));
                            wire_box(
                                &mut lines,
                                end - Vec3::splat(handle_length * 0.065),
                                end + Vec3::splat(handle_length * 0.065),
                                color,
                            );
                        }
                    }
                }
            }
        }
        for b in &self.scene.split_boxes {
            let min = Vec3::from_array(b.bounds.min.map(|v| v as f32));
            let max = Vec3::from_array(b.bounds.max.map(|v| v as f32 + 1.));
            wire_box(&mut lines, min, max, [1., 0.7, 0.2, 1.]);
            if self.scene.tool == "split" {
                for axis in 0..3 {
                    for maximum in [false, true] {
                        let mut p = (min + max) * 0.5;
                        p[axis] = if maximum { max[axis] } else { min[axis] };
                        wire_box(
                            &mut lines,
                            p - Vec3::splat(handle_length * 0.065),
                            p + Vec3::splat(handle_length * 0.065),
                            [1., 0.7, 0.2, 1.],
                        );
                    }
                }
            }
        }
        lines
    }
}
fn wire_box(lines: &mut Vec<(Vec3, Vec3, [f32; 4])>, min: Vec3, max: Vec3, color: [f32; 4]) {
    for bits in 0..8 {
        let p = Vec3::new(
            if bits & 1 == 0 { min.x } else { max.x },
            if bits & 2 == 0 { min.y } else { max.y },
            if bits & 4 == 0 { min.z } else { max.z },
        );
        for axis in 0..3 {
            if bits & (1 << axis) == 0 {
                let mut q = p;
                q[axis] = max[axis];
                lines.push((p, q, color));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn runtime(path: &Path) -> EditorRuntime {
        let scene: Scene = serde_json::from_value(serde_json::json!({
            "schema":"nova.composition.editor-scene.v1", "projectId":"project", "sceneKey":"mesh-identity", "generation":1,
            "resources":[{"id":"source","bounds":{"min":[0,0,0],"max":[0,0,0]},"baseOffset":[32,0,0]}],
            "instances":[
                {"id":"first","resourceId":"source","transform":{"offset":[0,0,0],"origin":[0,0,0],"rotation":0,"mirrorX":false,"mirrorZ":false},"visible":true,"locked":false},
                {"id":"second","resourceId":"source","transform":{"offset":[0,0,0],"origin":[0,0,0],"rotation":0,"mirrorX":false,"mirrorZ":false},"visible":true,"locked":false}],
            "selectedInstanceId":"first", "tool":"translate", "splitBoxes":[], "eventPath":path
        })).unwrap();
        scene.validate().unwrap();
        EditorRuntime {
            scene,
            sequence: 0,
            gesture: None,
            pending_commit: None,
            dirty: true,
        }
    }
    #[test]
    fn exact_triangles_reject_empty_bounds_and_keep_mirrors() {
        let ray = Ray {
            origin: Vec3::new(0.2, 0.2, -2.),
            direction: Vec3::Z,
        };
        assert!(ray.triangle([Vec3::ZERO, Vec3::X, Vec3::Y]).is_some());
        assert!(
            Ray {
                origin: Vec3::new(0.9, 0.9, -2.),
                ..ray
            }
            .triangle([Vec3::ZERO, Vec3::X, Vec3::Y])
            .is_none()
        );
        let t = Transform {
            offset: [3, 2, -1],
            origin: [0; 3],
            rotation: 90,
            mirror_x: true,
            mirror_z: false,
        };
        let matrix = model_matrix(&t, [32, 0, 0]);
        let p = matrix.transform_point3(Vec3::new(32.5, 0.5, 0.5));
        let expected = crate::composition_transform::Transform {
            offset: t.offset,
            origin: t.origin,
            rotation: t.rotation,
            mirror_x: t.mirror_x,
            mirror_z: t.mirror_z,
        }
        .point([0.5, 0.5, 0.5]);
        assert!(p.distance(Vec3::from_array(expected.map(|v| v as f32))) < 1e-5);
    }
    #[test]
    fn exact_overlap_selection_and_gpu_resource_identity_survive_transforms() {
        let directory = tempfile::tempdir().unwrap();
        let mut runtime = runtime(&directory.path().join("event.json"));
        let triangles = [[
            Vec3::new(32., 0., 0.),
            Vec3::new(33., 0., 0.),
            Vec3::new(32., 1., 0.),
        ]];
        let ray = Ray {
            origin: Vec3::new(0.2, 0.2, -2.),
            direction: Vec3::Z,
        };
        let hits = runtime.pick(
            ray,
            std::iter::once((
                "source",
                triangles.as_slice(),
                (Vec3::new(32., 0., 0.), Vec3::new(33., 1., 0.)),
            )),
        );
        assert_eq!(hits.iter().map(|v| v.0).collect::<Vec<_>>(), vec![0, 1]);
        runtime.scene.tool = "select".into();
        runtime.press(ray, &hits, 1., true).unwrap();
        assert_eq!(
            runtime.scene.selected_instance_id.as_deref(),
            Some("second")
        );
        runtime.scene.instances[1].transform.offset = [100, 0, 0];
        assert_eq!(runtime.scene.resources.len(), 1);
        assert_eq!(runtime.resource_for_chunk([1, 0, 0], 32), Some("source"));
        assert_eq!(runtime.scene.scene_key, "mesh-identity");
        assert_eq!(runtime.instances_for_resource("source").count(), 2);
    }
    #[test]
    fn snapped_gesture_commits_once_and_waits_for_authoritative_ack() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("event.json");
        let mut runtime = runtime(&path);
        let ray = |x| Ray {
            origin: Vec3::new(x, 0.5, -3.),
            direction: Vec3::Z,
        };
        runtime.press(ray(1.5), &[], 1., false).unwrap();
        assert!(runtime.gesture_active());
        assert!(runtime.motion(ray(3.7)));
        assert_eq!(runtime.scene.instances[0].transform.offset, [2, 0, 0]);
        runtime.finish().unwrap();
        let committed = fs::read(&path).unwrap();
        runtime.press(ray(20.), &[], 1., false).unwrap();
        runtime.cancel().unwrap();
        assert_eq!(fs::read(&path).unwrap(), committed);
        let mut authority = runtime.scene.clone();
        authority.instances[0].transform.offset = [0, 0, 0];
        runtime.update(authority.clone()).unwrap();
        assert_eq!(runtime.scene.instances[0].transform.offset, [2, 0, 0]);
        authority.acknowledged_event_sequence = Some(1);
        authority.generation = 2;
        runtime.update(authority).unwrap();
        assert_eq!(runtime.scene.instances[0].transform.offset, [0, 0, 0]);
        runtime.press(ray(1.5), &[], 1., false).unwrap();
        runtime.motion(ray(3.7));
        assert!(runtime.cancel().unwrap());
        assert_eq!(runtime.scene.instances[0].transform.offset, [0, 0, 0]);
        let event: viewer_launch_contract::CompositionEditorEvent =
            serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert!(event.cancelled && !event.committed);
    }
    #[test]
    fn split_face_drag_is_grid_snapped_and_cancel_restores_bounds() {
        let directory = tempfile::tempdir().unwrap();
        let mut runtime = runtime(&directory.path().join("event.json"));
        runtime.scene.tool = "split".into();
        runtime.scene.split_boxes = vec![viewer_launch_contract::CompositionEditorBox {
            id: "box".into(),
            name: "region".into(),
            bounds: Bounds {
                min: [0, 0, 0],
                max: [3, 3, 3],
            },
        }];
        let ray = |x| Ray {
            origin: Vec3::new(x, 2., -10.),
            direction: Vec3::Z,
        };
        runtime.press(ray(4.), &[], 1., false).unwrap();
        assert!(runtime.gesture_active());
        runtime.motion(ray(6.4));
        assert_eq!(runtime.scene.split_boxes[0].bounds.max, [5, 3, 3]);
        runtime.cancel().unwrap();
        assert_eq!(runtime.scene.split_boxes[0].bounds.max, [3, 3, 3]);
    }
}
