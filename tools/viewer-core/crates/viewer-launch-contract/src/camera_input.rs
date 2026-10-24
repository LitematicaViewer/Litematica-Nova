//! Renderer-independent camera input math. Cursor and viewport coordinates are
//! physical pixels; relative mouse deltas are consumed once, without DPI or dt.
pub fn rotation_delta(delta: [f64; 2], sensitivity: f64) -> Option<[f64; 2]> {
    if !sensitivity.is_finite() || sensitivity <= 0.0 || delta.iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some([-delta[0] * 0.006 * sensitivity, delta[1] * 0.006 * sensitivity])
}

pub fn screen_pan(delta: [f64; 2], height: u32, distance: f64, fov: f64,
    right: [f64; 3], up: [f64; 3]) -> [f64; 3] {
    if height == 0 || delta.iter().any(|v| !v.is_finite()) || !distance.is_finite() || !fov.is_finite() {
        return [0.0; 3];
    }
    let scale = 2.0 * distance * (fov * 0.5).tan() / f64::from(height);
    std::array::from_fn(|i| (-right[i] * delta[0] + up[i] * delta[1]) * scale)
}

/// Uses the target-depth plane as a stable cursor anchor when no scene hit is
/// available. It never queries or rebuilds geometry.
pub fn zoom_anchor_shift(cursor: Option<[f64; 2]>, viewport: [u32; 2], distance: f64,
    next_distance: f64, fov: f64, right: [f64; 3], up: [f64; 3]) -> [f64; 3] {
    let Some(cursor) = cursor else { return [0.0; 3]; };
    if viewport.contains(&0) || distance <= 0.0 || !distance.is_finite()
        || !next_distance.is_finite() || cursor.iter().any(|v| !v.is_finite()) {
        return [0.0; 3];
    }
    let pixel_scale = 2.0 * (distance - next_distance) * (fov * 0.5).tan() / f64::from(viewport[1]);
    let offset = [cursor[0] - f64::from(viewport[0]) * 0.5, f64::from(viewport[1]) * 0.5 - cursor[1]];
    std::array::from_fn(|i| (right[i] * offset[0] + up[i] * offset[1]) * pixel_scale)
}

pub fn zoom_ratio(lines: f64) -> Option<f64> {
    lines.is_finite().then(|| (-lines * 0.12).exp())
}

/// W/S/A/D, Space, left/right Shift, left/right Ctrl. Both controllers use
/// world-up elevation and normalize the combined movement, not its axes.
pub fn movement_step(keys: [bool; 9], forward: [f64; 3], right: [f64; 3],
    speed: f64, dt: f64) -> Option<[f64; 3]> {
    if !dt.is_finite() || dt <= 0.0 || !speed.is_finite() || speed <= 0.0 { return None; }
    let axis = |a: usize, b: usize| keys[a] as u8 as f64 - keys[b] as u8 as f64;
    let vertical = keys[4] as u8 as f64 - (keys[5] || keys[6]) as u8 as f64;
    let v: [f64; 3] = std::array::from_fn(|i| forward[i] * axis(0, 1) + right[i] * axis(3, 2)
        + if i == 1 { vertical } else { 0.0 });
    let length = v.iter().map(|v| v * v).sum::<f64>().sqrt();
    if !length.is_finite() || length <= f64::EPSILON { return None; }
    let boost = if keys[7] || keys[8] { 4.0 } else { 1.0 };
    Some(v.map(|v| v * speed * boost * dt / length))
}

#[derive(Clone, Debug, serde::Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SceneViewFeedback {
    pub request_id: String,
    pub camera_locked: bool,
    pub world_time: Option<u32>,
    pub weather: Option<String>,
    pub error: Option<String>,
}

/// The Host supplies an absolute, session-owned receipt path. A sibling
/// temporary file keeps readers from observing partial success/error state.
pub fn publish_feedback(path: &std::path::Path, value: &SceneViewFeedback) -> Result<(), String> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    if !path.is_absolute() || value.request_id.is_empty() {
        return Err("Scene view feedback requires an absolute receipt path and request identity".into());
    }
    let parent = path.parent().ok_or("Scene view receipt has no parent")?;
    let name = path.file_name().ok_or("Scene view receipt has no file name")?.to_string_lossy();
    let temporary = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)));
    let result = (|| {
        let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true)
            .open(&temporary).map_err(|e| format!("Create scene view feedback: {e}"))?;
        file.write_all(&bytes).map_err(|e| format!("Write scene view feedback: {e}"))?;
        drop(file);
        std::fs::rename(&temporary, path).map_err(|e| format!("Publish scene view feedback: {e}"))
    })();
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_atomically_replaces_actual_capture_and_failure_state() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("nova-camera-feedback-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("state.json");
        let mut feedback = SceneViewFeedback { request_id: "camera-test".into(), camera_locked: true,
            world_time: Some(6000), weather: Some("auto".into()), error: None };
        publish_feedback(&path, &feedback).unwrap();
        feedback.camera_locked = false;
        feedback.error = Some("Native focus was lost".into());
        publish_feedback(&path, &feedback).unwrap();
        let actual: SceneViewFeedback = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(actual, feedback);
        assert!(publish_feedback(std::path::Path::new("relative.json"), &feedback).is_err());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn mouse_rotation_is_dt_independent_and_shared_by_both_modes() {
        let [yaw, pitch] = rotation_delta([40.0, 20.0], 1.0).unwrap();
        assert!((yaw + 0.24).abs() < 1e-12 && (pitch - 0.12).abs() < 1e-12);
        assert_eq!(rotation_delta([f64::NAN, 0.0], 1.0), None);
    }

    #[test]
    fn movement_is_normalized_and_integrates_elapsed_time_once() {
        let mut keys = [false; 9]; keys[0] = true; keys[3] = true; keys[4] = true;
        let step = |dt| movement_step(keys, [0.0, 0.0, -1.0], [1.0, 0.0, 0.0], 12.0, dt).unwrap();
        let a = step(0.5); let b = step(1.0);
        assert!((b.iter().map(|v| v*v).sum::<f64>().sqrt() - 12.0).abs() < 1e-10);
        for i in 0..3 { assert!((a[i] * 2.0 - b[i]).abs() < 1e-10); }
        assert!(movement_step([false; 9], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0], 12.0, 1.0).is_none());
    }

    #[test]
    fn physical_dpi_coordinates_preserve_pan_and_cursor_anchor() {
        let right = [1.0, 0.0, 0.0]; let up = [0.0, 1.0, 0.0];
        let a = screen_pan([20.0, 10.0], 800, 30.0, 0.8, right, up);
        let b = screen_pan([40.0, 20.0], 1600, 30.0, 0.8, right, up);
        assert_eq!(a, b);
        let center = zoom_anchor_shift(Some([640.0, 400.0]), [1280, 800], 30.0, 20.0, 0.8, right, up);
        assert_eq!(center, [0.0; 3]);
        let anchor = zoom_anchor_shift(Some([1000.0, 250.0]), [1280, 800], 30.0, 20.0, 0.8, right, up);
        assert!(anchor[0] > 0.0 && anchor[1] > 0.0);
        assert!(zoom_ratio(1.0).unwrap() < 1.0);
    }
}

