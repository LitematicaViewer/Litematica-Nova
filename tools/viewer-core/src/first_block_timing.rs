//! Click -> exact GPU frame -> compositor display receipts, never Ready/ACK.
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn QueryPerformanceCounter(value: *mut i64) -> i32;
    fn QueryPerformanceFrequency(value: *mut i64) -> i32;
}

pub(super) struct FirstBlockTiming {
    click_epoch_ms: Option<u64>,
    request: Option<Instant>,
    submitted_count: Option<u32>,
    completed: bool,
    visible_samples_verified: bool,
    sample_scene_revision: Option<u64>,
    unverified_display_logged: bool,
    display_receipts: std::collections::VecDeque<(u32, i64)>,
    source: String,
    mode: &'static str,
}

impl FirstBlockTiming {
    pub(super) fn new(source: &std::path::Path, unlit: bool) -> Self {
        Self::with_mode(source, if unlit { "standard" } else { "full" })
    }
    pub(super) fn without_projection(unlit: bool) -> Self {
        let mut timing = Self::with_mode(std::path::Path::new(""), if unlit { "standard" } else { "full" });
        timing.click_epoch_ms = None;
        timing
    }
    pub(super) fn new_fast(source: &std::path::Path) -> Self {
        Self::with_mode(source, "fast")
    }
    fn with_mode(source: &std::path::Path, mode: &'static str) -> Self {
        Self { click_epoch_ms: std::env::var("LITEMATICA_NOVA_CLICK_EPOCH_MS").ok().and_then(|v| v.parse().ok()),
            request: None, submitted_count: None, completed: false, visible_samples_verified: false,
            sample_scene_revision: None, unverified_display_logged: false, display_receipts: Default::default(),
            source: source.display().to_string(), mode }
    }
    pub(super) fn record_fixed_resources_ready(&self, proof: (u64, u64, [u32; 2])) {
        eprintln!("[NOVA_FIRST_BLOCK] stage=fixed_gpu_resources_ready source={:?} mode={} click_epoch_ms={:?} click_elapsed_ms={:?} gpu_device_initialized=true terrain_atlas_upload_complete=true atlas_completion=published_after_timeline_wait device_generation={} atlas_upload_serial={} atlas_extent={}x{} existing_scene_attached=false scene_bootstrap_ready=false",
            self.source, self.mode, self.click_epoch_ms, self.click_epoch_ms.map(|click| epoch_ms().saturating_sub(click)),
            proof.0, proof.1, proof.2[0], proof.2[1]);
    }

    pub(super) fn begin_presentation(&mut self) {
        // Placement/DPI changes reuse the same scene and user action. Preserve
        // its in-flight sample until the compositor reports that exact frame.
        // New clicks, scenes and Hide already clear request at their boundaries.
        if self.request.is_some() { return; }
        if self.completed { self.click_epoch_ms = None; }
        self.request = Some(Instant::now()); self.submitted_count = None; self.completed = false;
        self.visible_samples_verified = false; self.sample_scene_revision = None; self.unverified_display_logged = false; self.display_receipts.clear();
        eprintln!("[NOVA_FIRST_BLOCK] stage=presentation_requested source={:?} mode={} click_epoch_ms={:?}",
            self.source, self.mode, self.click_epoch_ms);
    }
    pub(super) fn hide(&mut self) { self.request = None; self.submitted_count = None; self.visible_samples_verified = false; self.sample_scene_revision = None; self.display_receipts.clear(); }
    pub(super) fn set_click_for_presentation(&mut self, click_epoch_ms: u64) {
        self.click_epoch_ms = Some(click_epoch_ms);
        self.request = None; self.submitted_count = None; self.completed = false;
        self.visible_samples_verified = false; self.sample_scene_revision = None; self.unverified_display_logged = false; self.display_receipts.clear();
    }

    pub(super) fn needs_visible_sample(&self) -> bool {
        self.request.is_some() && !self.completed && !self.visible_samples_verified
    }

    pub(super) fn needs_display_confirmation(&self) -> bool {
        self.request.is_some() && !self.completed
    }

    pub(super) fn exact_present_submitted(&mut self, count: Option<u32>) {
        if self.completed || self.request.is_none() || self.submitted_count.is_some() { return; }
        self.submitted_count = count.filter(|count| *count != 0);
        eprintln!("[NOVA_FIRST_BLOCK] stage=presented_nonempty_draw source={:?} mode={} click_to_gpu_ms={:?} display_verified=false visible_samples_verified=false final_precision=true present_count={:?}",
            self.source, self.mode, self.click_epoch_ms.map(|click| epoch_ms().saturating_sub(click)), self.submitted_count);
    }

    pub(super) fn visible_projection_present_submitted(&mut self, count: Option<u32>, scene_revision: u64) {
        if self.completed || self.request.is_none() || self.visible_samples_verified { return; }
        let Some(count) = count.filter(|count| *count != 0) else { return; };
        self.submitted_count = Some(count);
        self.visible_samples_verified = true;
        self.sample_scene_revision = Some(scene_revision);
        eprintln!("[NOVA_FIRST_BLOCK] stage=gpu_visible_projection_sample source={:?} mode={} scene_revision={} present_count={} click_to_gpu_ms={:?} visible_samples_verified=true display_verified=false final_precision=true",
            self.source, self.mode, scene_revision, count, self.click_epoch_ms.map(|click| epoch_ms().saturating_sub(click)));
    }

    pub(super) fn observe_display(&mut self, receipt: Option<(u32, i64)>) {
        if self.completed || self.request.is_none() { return; }
        if let Some((observed, qpc)) = receipt {
            if self.display_receipts.back().map(|value| value.0) != Some(observed) {
                self.display_receipts.push_back((observed, qpc));
                if self.display_receipts.len() > 8 { self.display_receipts.pop_front(); }
            }
        }
        let Some(required) = self.submitted_count else { return; };
        // A later PresentCount alone does not prove that an older flip-discard
        // frame was displayed. Retain a short history of exact monitor receipts
        // because the one-query GPU result can arrive a frame later.
        let Some((observed, qpc)) = self.display_receipts.iter().copied().find(|value| value.0 == required) else {
            if receipt.is_some_and(|value| value.0 > required) {
                self.submitted_count = None; self.visible_samples_verified = false; self.sample_scene_revision = None;
            }
            return;
        };
        if !self.visible_samples_verified {
            if !self.unverified_display_logged {
                eprintln!("[NOVA_FIRST_BLOCK] stage=displayed_nonempty_draw source={:?} mode={} present_count={} visible_samples_verified=false target_met=not_measured", self.source, self.mode, observed);
                self.unverified_display_logged = true;
            }
            return;
        }
        // SyncQPCTime belongs to SyncRefreshCount, not necessarily this frame's
        // PresentRefreshCount. A fresh monitor-display receipt plus the current
        // clock gives a conservative display-confirmation upper bound instead
        // of inventing an earlier end point. <=100ms is only claimed when this
        // stronger upper bound itself meets the target.
        let display_confirmed_epoch_ms = epoch_ms();
        let elapsed = self.click_epoch_ms.and_then(|click| display_confirmed_epoch_ms.checked_sub(click));
        eprintln!("[NOVA_FIRST_BLOCK] stage=compositor_displayed source={:?} mode={} click_epoch_ms={:?} display_confirmed_epoch_ms={} click_to_display_upper_bound_ms={:?} timing=monitor_display_receipt_upper_bound target_ms=100 target_met={:?} final_precision=true visible_samples_verified=true scene_revision={:?} sampled_present_count={} present_count={} dxgi_sync_qpc={} dxgi_sync_epoch_ms={:?}",
            self.source, self.mode, self.click_epoch_ms, display_confirmed_epoch_ms, elapsed,
            elapsed.map(|elapsed| elapsed <= 100), self.sample_scene_revision, required, observed, qpc, qpc_epoch_ms(qpc));
        self.completed = true;
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |value| value.as_millis().min(u128::from(u64::MAX)) as u64)
}
fn qpc_epoch_ms(display_qpc: i64) -> Option<u64> {
    let mut counter = 0_i64; let mut frequency = 0_i64;
    if unsafe { QueryPerformanceCounter(&mut counter) } == 0
        || unsafe { QueryPerformanceFrequency(&mut frequency) } == 0 || frequency <= 0 || display_qpc <= 0 { return None; }
    let delta_ms = i128::from(display_qpc - counter) * 1000 / i128::from(frequency);
    u64::try_from(i128::from(epoch_ms()) + delta_ms).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prepared_or_old_refresh_does_not_complete_first_block_timing() {
        let mut timing = FirstBlockTiming::new(std::path::Path::new("fixture.litematic"), true);
        timing.begin_presentation();
        timing.observe_display(Some((100, 1))); assert!(!timing.completed);
        timing.exact_present_submitted(Some(101));
        timing.observe_display(Some((100, 1))); assert!(!timing.completed);
        timing.hide(); timing.observe_display(Some((101, 1))); assert!(!timing.completed);
    }
    #[test]
    fn displayed_draw_without_visible_sample_is_not_a_first_block() {
        let mut timing = FirstBlockTiming::new(std::path::Path::new("fixture.litematic"), true);
        timing.begin_presentation(); timing.exact_present_submitted(Some(101));
        timing.observe_display(Some((101, 1))); assert!(!timing.completed);
        timing.visible_projection_present_submitted(Some(102), 7);
        timing.observe_display(Some((101, 1))); assert!(!timing.completed);
        timing.observe_display(Some((102, 1))); assert!(timing.completed);
        assert_eq!(timing.sample_scene_revision, Some(7));
        timing.hide(); assert!(!timing.visible_samples_verified);
    }
    #[test]
    fn newer_monitor_count_does_not_prove_a_skipped_sample_frame() {
        let mut timing = FirstBlockTiming::new_fast(std::path::Path::new("fixture.litematic"));
        timing.begin_presentation(); timing.visible_projection_present_submitted(Some(102), 3);
        timing.observe_display(Some((103, 1))); assert!(!timing.completed);
        timing.visible_projection_present_submitted(Some(104), 3);
        timing.observe_display(Some((104, 1))); assert!(timing.completed);
    }
    #[test]
    fn asynchronous_sample_uses_its_exact_earlier_monitor_receipt() {
        let mut timing = FirstBlockTiming::new_fast(std::path::Path::new("fixture.litematic"));
        timing.begin_presentation(); timing.observe_display(Some((102, 1)));
        timing.observe_display(Some((103, 1)));
        timing.visible_projection_present_submitted(Some(102), 4);
        timing.observe_display(Some((103, 1))); assert!(timing.completed);
        assert_eq!(timing.submitted_count, Some(102));
    }
    #[test]
    fn same_scene_placement_updates_preserve_sample_until_passive_display_confirmation() {
        let mut timing = FirstBlockTiming::new(std::path::Path::new("same-scene.litematic"), true);
        timing.set_click_for_presentation(100);
        timing.begin_presentation();
        let requested = timing.request;
        timing.visible_projection_present_submitted(Some(1), 7);
        timing.observe_display(None);
        assert!(timing.needs_display_confirmation()); assert!(!timing.completed);
        for _ in 0..3 { timing.begin_presentation(); timing.observe_display(None); }
        assert_eq!(timing.request, requested);
        assert_eq!(timing.click_epoch_ms, Some(100));
        assert_eq!(timing.submitted_count, Some(1));
        assert_eq!(timing.sample_scene_revision, Some(7));
        assert!(timing.visible_samples_verified);
        // A later passive DXGI read, without another draw, confirms the same frame.
        timing.observe_display(Some((1, 11)));
        assert!(timing.completed); assert!(!timing.needs_display_confirmation());
        timing.begin_presentation();
        assert!(timing.completed); assert_eq!(timing.request, requested);
        assert_eq!(timing.click_epoch_ms, Some(100), "completed timing is not restarted by a resize");
    }

    #[test]
    fn new_click_hidden_page_and_new_scene_do_not_reuse_old_display_proof() {
        let mut timing = FirstBlockTiming::new_fast(std::path::Path::new("first-scene.litematic"));
        timing.set_click_for_presentation(100); timing.begin_presentation();
        timing.visible_projection_present_submitted(Some(10), 1);
        timing.set_click_for_presentation(200); timing.begin_presentation();
        timing.observe_display(Some((10, 11)));
        assert!(!timing.completed); assert!(!timing.visible_samples_verified);
        timing.visible_projection_present_submitted(Some(11), 1);
        timing.observe_display(Some((11, 12))); assert!(timing.completed);
        timing.set_click_for_presentation(300); timing.begin_presentation();
        timing.visible_projection_present_submitted(Some(12), 1);
        timing.hide();
        assert!(!timing.needs_display_confirmation());
        timing.observe_display(Some((12, 13))); assert!(!timing.completed);
        timing.begin_presentation(); timing.observe_display(Some((12, 13)));
        assert!(!timing.completed, "restore still requires a fresh sampled frame");
        let mut new_scene = FirstBlockTiming::new_fast(std::path::Path::new("second-scene.litematic"));
        new_scene.set_click_for_presentation(400); new_scene.begin_presentation();
        new_scene.observe_display(Some((12, 13))); assert!(!new_scene.completed);
        new_scene.visible_projection_present_submitted(Some(13), 2);
        new_scene.observe_display(Some((12, 13))); assert!(!new_scene.completed);
        new_scene.observe_display(Some((13, 14))); assert!(new_scene.completed);
        assert_eq!(new_scene.sample_scene_revision, Some(2));
    }

}
