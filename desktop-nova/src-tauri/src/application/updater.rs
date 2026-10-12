//! Product update orchestration; Tauri's official updater owns transport, signature and installation.
use crate::platform::process_supervisor::{ExecutableRole, ProcessSupervisor};
use super::quick_preview::QuickPreviewHost;
use serde::Serialize;
use std::{path::PathBuf, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::watch;

const EVENT: &str = "nova-update-state";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateView {
    phase: &'static str,
    current_version: String,
    version: Option<String>,
    date: Option<String>,
    notes: Option<String>,
    downloaded: u64,
    total: Option<u64>,
    source: Option<String>,
    message: Option<String>,
}

struct Pending { update: Update, endpoint: usize, bytes: Option<Vec<u8>> }

pub(crate) struct UpdateHost {
    view: Mutex<UpdateView>,
    pending: Mutex<Option<Pending>>,
    busy: AtomicBool,
    cancel: watch::Sender<u64>,
    skipped_path: PathBuf,
}

struct Busy<'a>(&'a AtomicBool);
impl Drop for Busy<'_> { fn drop(&mut self) { self.0.store(false, Ordering::Release); } }

impl UpdateHost {
    pub(crate) fn new(version: String, data_root: PathBuf) -> Self {
        let (cancel, _) = watch::channel(0);
        Self {
            view: Mutex::new(UpdateView { phase: "idle", current_version: version, version: None,
                date: None, notes: None, downloaded: 0, total: None, source: None, message: None }),
            pending: Mutex::new(None), busy: AtomicBool::new(false), cancel,
            skipped_path: data_root.join("updater-skipped-version.txt"),
        }
    }
    fn acquire(&self) -> Result<Busy<'_>, String> {
        if self.snapshot().phase == "restartRequired" { return Err("请先重启恢复后台服务。".into()); }
        self.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "更新操作正在进行，请稍候。".to_owned())?;
        Ok(Busy(&self.busy))
    }
    fn snapshot(&self) -> UpdateView { self.view.lock().unwrap().clone() }
    fn publish(&self, app: &AppHandle, change: impl FnOnce(&mut UpdateView)) {
        let view = { let mut view = self.view.lock().unwrap(); change(&mut view); view.clone() };
        let _ = app.emit_to("main", EVENT, view);
    }
    fn fail(&self, app: &AppHandle, error: String) -> String {
        self.publish(app, |v| { v.phase = "error"; v.message = Some(error.clone()); }); error
    }
    fn restart_required(&self, app: &AppHandle, error: String) -> String {
        self.publish(app, |v| { v.phase = "restartRequired"; v.message = Some(format!("{error}；草稿已保留，请重启以恢复后台服务。")); }); error
    }
    pub(crate) fn prevents_close(&self) -> bool {
        matches!(self.snapshot().phase, "waiting" | "installing")
    }
}

fn require_main(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "main" { Ok(()) } else { Err("更新操作仅由主窗口执行。".into()) }
}

fn validate_https(url: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(url).map_err(|_| "更新地址格式错误。".to_owned())?;
    if url.scheme() != "https" || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err("更新接口及下载必须使用无凭据 HTTPS 地址。".into());
    }
    Ok(url)
}

fn endpoints(app: &AppHandle) -> Result<Vec<reqwest::Url>, String> {
    let config = app.config().plugins.0.get("updater").ok_or("尚未配置更新服务。")?;
    if config.get("pubkey").and_then(|v| v.as_str()).unwrap_or("").trim().is_empty() {
        return Err("尚未配置更新签名公钥。".into());
    }
    let values = config.get("endpoints").and_then(|v| v.as_array()).ok_or("尚未配置更新服务。")?;
    let result = values.iter().map(|v| validate_https(v.as_str().unwrap_or(""))).collect::<Result<Vec<_>, _>>()?;
    if result.is_empty() { Err("尚未配置更新服务。".into()) } else { Ok(result) }
}

async fn check_endpoint(app: &AppHandle, url: reqwest::Url) -> Result<Option<Update>, String> {
    let result = app.updater_builder().endpoints(vec![url]).map_err(|e| e.to_string())?
        .timeout(Duration::from_secs(15))
        // This applies to manifest AND binary, including redirects. No insecure TLS switches.
        .configure_client(|client| client.https_only(true))
        .build().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    if let Some(update) = &result {
        validate_https(update.download_url.as_str())?;
        if update.signature.trim().is_empty() { return Err("更新包缺少签名。".into()); }
    }
    Ok(result)
}

async fn first_success<T, F, Fut>(count: usize, mut check: F) -> Result<(usize, Option<T>), String>
where F: FnMut(usize) -> Fut, Fut: std::future::Future<Output = Result<Option<T>, String>> {
    let mut errors = Vec::new();
    for index in 0..count {
        match check(index).await {
            Ok(value) => return Ok((index, value)),
            Err(error) => errors.push(format!("源 {}：{error}", index + 1)),
        }
    }
    Err(errors.join("\n"))
}

async fn await_or_cancel<T>(cancel: &mut watch::Receiver<u64>, future: impl std::future::Future<Output = T>) -> Option<T> {
    tokio::select! { biased;
        _ = cancel.changed() => None,
        result = future => Some(result),
    }
}

fn same_release(version: &str, signature: &str, candidate: &Update) -> bool {
    candidate.version == version && candidate.signature == signature
}

#[tauri::command]
pub(crate) fn application_update_snapshot(window: WebviewWindow, host: State<'_, UpdateHost>) -> Result<UpdateView, String> {
    require_main(&window)?; Ok(host.snapshot())
}

#[tauri::command]
pub(crate) async fn application_update_check(app: AppHandle, window: WebviewWindow, host: State<'_, UpdateHost>, manual: bool) -> Result<UpdateView, String> {
    require_main(&window)?;
    let _busy = host.acquire()?;
    let mut cancel = host.cancel.subscribe();
    let urls = match endpoints(&app) { Ok(v) => v, Err(error) => {
        host.publish(&app, |v| { v.phase = "disabled"; v.message = Some(error); }); return Ok(host.snapshot());
    }};
    host.publish(&app, |v| { v.phase = "checking"; v.message = None; });
    let Some(result) = await_or_cancel(&mut cancel, first_success(urls.len(), |i| check_endpoint(&app, urls[i].clone()))).await else { return Ok(host.snapshot()); };
    match result {
        Ok((index, Some(update))) => {
            let skipped = std::fs::read_to_string(&host.skipped_path).unwrap_or_default();
            host.publish(&app, |v| {
                v.phase = if !manual && skipped.trim() == update.version { "skipped" } else { "available" };
                v.version = Some(update.version.clone()); v.notes = update.body.clone();
                v.date = update.date.map(|d| d.date().to_string());
                v.source = Some(if index == 0 { "GitHub" } else { "镜像服务器" }.into());
                v.downloaded = 0; v.total = None;
            });
            *host.pending.lock().unwrap() = Some(Pending { update, endpoint: index, bytes: None });
        }
        Ok((_, None)) => { *host.pending.lock().unwrap() = None; host.publish(&app, |v| { v.phase = "current"; v.message = None; }); }
        Err(error) => { return Err(host.fail(&app, format!("更新检查失败：{error}"))); }
    }
    Ok(host.snapshot())
}

#[tauri::command]
pub(crate) fn application_update_cancel(app: AppHandle, window: WebviewWindow, host: State<'_, UpdateHost>) -> Result<UpdateView, String> {
    require_main(&window)?;
    if matches!(host.snapshot().phase, "installing" | "restartRequired") { return Err("后台服务已进入退出阶段，请等待安装或重启恢复。".into()); }
    host.cancel.send_modify(|value| *value += 1);
    host.publish(&app, |v| { v.phase = "cancelled"; v.message = None; });
    Ok(host.snapshot())
}

#[tauri::command]
pub(crate) fn application_update_skip(app: AppHandle, window: WebviewWindow, host: State<'_, UpdateHost>) -> Result<UpdateView, String> {
    require_main(&window)?;
    let _busy = host.acquire()?;
    let version = host.snapshot().version.ok_or("当前没有可跳过的版本。")?;
    if let Some(parent) = host.skipped_path.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    std::fs::write(&host.skipped_path, version).map_err(|e| e.to_string())?;
    host.publish(&app, |v| v.phase = "skipped"); Ok(host.snapshot())
}

#[tauri::command]
pub(crate) async fn application_update_download(app: AppHandle, window: WebviewWindow, host: State<'_, UpdateHost>) -> Result<UpdateView, String> {
    require_main(&window)?;
    let _busy = host.acquire()?;
    let mut cancel = host.cancel.subscribe();
    let (original, endpoint) = host.pending.lock().unwrap().as_ref().map(|p| (p.update.clone(), p.endpoint)).ok_or("请先检查更新。")?;
    let urls = endpoints(&app)?;
    let mut last_error = String::new();
    // Original first, then each configured mirror at most once; never switch release identity.
    let order = std::iter::once(endpoint).chain((0..urls.len()).filter(|i| *i != endpoint));
    for index in order {
        let mut update = if index == endpoint { original.clone() } else {
            let checked = tokio::select! { biased;
                _ = cancel.changed() => return Ok(host.snapshot()),
                result = check_endpoint(&app, urls[index].clone()) => result,
            };
            match checked {
                Ok(Some(value)) if same_release(&original.version, &original.signature, &value) => value,
                Ok(_) => { last_error = "镜像尚无同版本、同签名更新包。".into(); continue; }
                Err(error) => { last_error = error; continue; }
            }
        };
        update.timeout = Some(Duration::from_secs(600));
        host.publish(&app, |v| { v.phase = "downloading"; v.downloaded = 0; v.total = None; v.message = None;
            v.source = Some(if index == 0 { "GitHub" } else { "镜像服务器" }.into()); });
        let mut downloaded = 0u64;
        let mut last_progress = Instant::now() - Duration::from_secs(1);
        let result = tokio::select! { biased;
            _ = cancel.changed() => return Ok(host.snapshot()),
            result = update.download(|chunk, total| {
                downloaded += chunk as u64;
                if last_progress.elapsed() >= Duration::from_millis(100) || total == Some(downloaded) {
                    host.publish(&app, |v| { v.downloaded = downloaded; v.total = total; });
                    last_progress = Instant::now();
                }
            }, || {}) => result,
        };
        match result {
            Ok(bytes) => {
                // Official download returns bytes only AFTER mandatory minisign verification.
                *host.pending.lock().unwrap() = Some(Pending { update, endpoint: index, bytes: Some(bytes) });
                host.publish(&app, |v| v.phase = "downloaded"); return Ok(host.snapshot());
            }
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(host.fail(&app, format!("更新下载或签名校验失败：{last_error}")))
}

#[tauri::command]
pub(crate) async fn application_update_install(app: AppHandle, window: WebviewWindow, host: State<'_, UpdateHost>) -> Result<(), String> {
    require_main(&window)?;
    let _busy = host.acquire()?;
    let mut cancel = host.cancel.subscribe();
    if !host.pending.lock().unwrap().as_ref().is_some_and(|p| p.bytes.is_some()) { return Err("尚无通过签名校验的更新包。".into()); }
    let supervisor = app.state::<Arc<ProcessSupervisor>>().inner().clone();
    let _pause = supervisor.pause_admitting();
    host.publish(&app, |v| { v.phase = "waiting"; v.message = Some("草稿已保留，等待现有任务安全结束…".into()); });
    let deadline = Instant::now() + Duration::from_secs(30 * 60);
    while supervisor.active_children().iter().any(|c| matches!(c.role, ExecutableRole::CoreHelper)) {
        if Instant::now() >= deadline { return Err(host.fail(&app, "任务仍在运行，更新未安装；请任务完成后重试。".into())); }
        tokio::select! { biased;
            _ = cancel.changed() => return Ok(()),
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }
    // The reversible/cancellable stage ends before shutting down shared services.
    host.publish(&app, |v| { v.phase = "installing"; v.message = Some("正在安全退出后台并安装，完成后自动重启…".into()); });
    // From here a failure requires the explicit recovery restart; a dropped pause
    // must never reopen admission into a service that has already shut down.
    supervisor.stop_admitting();
    if let Some(view) = app.state::<QuickPreviewHost>().snapshot().map_err(|e| host.restart_required(&app, e))? {
        app.state::<QuickPreviewHost>().close(&view.preview_id).map_err(|e| host.restart_required(&app, e))?;
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while !supervisor.active_children().is_empty() {
        if Instant::now() >= deadline { return Err(host.restart_required(&app, "后台任务尚未退出，更新未安装".into())); }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    host.publish(&app, |v| { v.phase = "installing"; v.message = Some("正在安装，完成后自动重启…".into()); });
    let pending = host.pending.lock().unwrap().take().ok_or_else(|| host.restart_required(&app, "更新包已失效".into()))?;
    let bytes = pending.bytes.ok_or_else(|| host.restart_required(&app, "更新包已失效".into()))?;
    let result = pending.update.install(bytes);
    result.map_err(|e| host.restart_required(&app, format!("更新安装失败：{e}")))?;
    #[cfg(not(windows))]
    app.restart();
    Ok(())
}

#[tauri::command]
pub(crate) fn application_update_restart(app: AppHandle, window: WebviewWindow, host: State<'_, UpdateHost>) -> Result<(), String> {
    require_main(&window)?;
    if host.snapshot().phase != "restartRequired" { return Err("当前无需恢复后台服务。".into()); }
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn updater_https_rejects_plaintext_credentials_and_bad_urls() {
        for bad in ["http://example.com/a", "file:///a", "https://u:p@example.com/a", "broken"] { assert!(validate_https(bad).is_err()); }
        assert!(validate_https("https://45-62-112-121.sslip.io:8448/nova-updates/latest.json").is_ok());
    }
    #[tokio::test]
    async fn updater_network_or_parse_failure_uses_mirror() {
        let (index, value) = first_success(2, |i| async move { if i == 0 { Err("connection/JSON error".into()) } else { Ok(Some("0.2.0")) } }).await.unwrap();
        assert_eq!((index, value), (1, Some("0.2.0")));
    }
    #[tokio::test]
    async fn updater_no_update_is_success_not_failed_source() {
        let (index, value) = first_success::<u32, _, _>(2, |i| async move { assert_eq!(i, 0); Ok(None) }).await.unwrap();
        assert_eq!((index, value), (0, None));
    }
    #[tokio::test]
    async fn updater_all_sources_failure_is_not_fake_success() {
        assert!(first_success::<u32, _, _>(2, |_| async { Err("offline".into()) }).await.is_err());
    }
    #[tokio::test]
    async fn updater_cancel_wins_over_late_check_result() {
        let (sender, mut receiver) = watch::channel(0);
        sender.send_modify(|value| *value += 1);
        let late_success = std::future::ready(Ok::<_, String>(Some("0.2.0")));
        assert!(await_or_cancel(&mut receiver, late_success).await.is_none());
    }
    #[test]
    fn updater_operation_guard_releases_on_error_and_drop() {
        let host = UpdateHost::new("0.1.0".into(), PathBuf::new());
        let guard = host.acquire().unwrap(); assert!(host.acquire().is_err()); drop(guard);
        assert!(host.acquire().is_ok()); assert!(!host.prevents_close());
        host.view.lock().unwrap().phase = "restartRequired";
        assert!(host.acquire().is_err(), "a stopped service must not accept normal updater actions");
    }
    #[test]
    fn updater_official_minisign_accepts_signed_fixture_and_rejects_tampering() {
        use base64::Engine;
        let config: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let decode = |value: &str| String::from_utf8(base64::engine::general_purpose::STANDARD.decode(value.trim()).unwrap()).unwrap();
        let public = minisign_verify::PublicKey::decode(&decode(config["plugins"]["updater"]["pubkey"].as_str().unwrap())).unwrap();
        let signature = minisign_verify::Signature::decode(&decode(include_str!("../../tests/fixtures/updater-signature.txt.sig"))).unwrap();
        let payload = include_bytes!("../../tests/fixtures/updater-signature.txt");
        assert!(public.verify(payload, &signature, true).is_ok());
        let mut tampered = payload.to_vec(); tampered[0] ^= 1;
        assert!(public.verify(&tampered, &signature, true).is_err());
        assert!(minisign_verify::Signature::decode("unsigned").is_err());
    }
}
