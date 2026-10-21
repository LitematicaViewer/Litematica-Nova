//! Product update orchestration; Tauri's official updater owns transport, signature and installation.
use crate::platform::process_supervisor::{ExecutableRole, ProcessSupervisor};
use crate::render_service_client::RenderServiceClient;
use super::quick_preview::QuickPreviewHost;
use super::operations::ApplicationOperations;
use serde::Serialize;
use std::{path::PathBuf, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}}, time::{Duration, Instant}};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::watch;
#[path = "updater_network.rs"]
mod network;

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

struct CheckedUpdate { update: Update, trace: network::Trace }
struct Pending { update: Update, endpoint: usize, bytes: Option<Vec<u8>>, trace: network::Trace }

pub(crate) struct UpdateHost {
    view: Mutex<UpdateView>,
    pending: Mutex<Option<Pending>>,
    busy: AtomicBool,
    closing: AtomicBool,
    checks_started: AtomicU64,
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
            pending: Mutex::new(None), busy: AtomicBool::new(false), closing: AtomicBool::new(false), checks_started: AtomicU64::new(0), cancel,
            skipped_path: data_root.join("updater-skipped-version.txt"),
        }
    }
    fn acquire(&self) -> Result<Busy<'_>, String> {
        if self.closing.load(Ordering::Acquire) { return Err("应用正在关闭".into()); }
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
        self.publish(app, |v| { v.phase = "restartRequired"; v.message = Some(format!("{error}；更新未安装，草稿已保留。重启当前版本仅恢复后台服务，也可退出后运行新版安装包覆盖升级。")); }); error
    }
    pub(crate) fn prevents_close(&self) -> bool {
        self.snapshot().phase == "installing"
    }
    pub(crate) fn prepare_for_close(&self, app: &AppHandle) -> bool {
        let Some(view) = self.prepare_close_state() else { return false; };
        let _ = app.emit_to("main", EVENT, view);
        true
    }
    fn prepare_close_state(&self) -> Option<UpdateView> {
        let mut view = self.view.lock().unwrap();
        if view.phase == "installing" { return None; }
        if view.phase == "waiting" {
            // Share the transition lock with commit_installing_after_wait:
            // whichever wins decides cancellable waiting versus actual install.
            self.cancel.send_modify(|value| *value += 1);
            view.phase = "cancelled";
            view.message = None;
            eprintln!("[NOVA_LIFECYCLE] stage=update_wait_cancelled_by_close installing=false");
        }
        Some(view.clone())
    }
    fn commit_installing_after_wait(&self, cancel: &watch::Receiver<u64>) -> Option<UpdateView> {
        let mut view = self.view.lock().unwrap();
        if view.phase != "waiting" || self.closing.load(Ordering::Acquire)
            || cancel.has_changed().unwrap_or(true) { return None; }
        view.phase = "installing";
        view.message = Some("正在安全退出后台并安装，完成后自动重启…".into());
        Some(view.clone())
    }
    pub(crate) fn cancel_for_shutdown(&self) {
        self.closing.store(true, Ordering::Release);
        self.cancel.send_modify(|value| *value += 1);
        let retained_bytes = self.pending.lock().unwrap().as_ref().and_then(|pending|pending.bytes.as_ref()).map_or(0,Vec::len);
        let mut view = self.view.lock().unwrap();
        if !matches!(view.phase, "installing" | "restartRequired") { view.phase = "cancelled"; }
        eprintln!("[NOVA_LIFECYCLE] stage=update_activity_at_close checks_started={} retained_bytes={retained_bytes}", self.checks_started.load(Ordering::Relaxed));
    }
    pub(crate) fn resume_after_cancelled_shutdown(&self) { self.closing.store(false, Ordering::Release); }
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
    configured_endpoints(config)
}

fn configured_endpoints(config: &serde_json::Value) -> Result<Vec<reqwest::Url>, String> {
    if config.get("pubkey").and_then(|v| v.as_str()).unwrap_or("").trim().is_empty() {
        return Err("尚未配置更新签名公钥。".into());
    }
    let values = config.get("endpoints").and_then(|v| v.as_array()).ok_or("尚未配置更新服务。")?;
    let result = values.iter().map(|v| validate_https(v.as_str().unwrap_or(""))).collect::<Result<Vec<_>, _>>()?;
    if result.is_empty() { Err("尚未配置更新服务。".into()) } else { Ok(result) }
}

async fn check_endpoint(app: &AppHandle, url: reqwest::Url) -> Result<Option<CheckedUpdate>, String> {
    let trace = network::Trace::new("manifest", &url);
    let client_trace = trace.clone();
    let builder = app.updater_builder().endpoints(vec![url]).map_err(|e| e.to_string())?
        .timeout(Duration::from_secs(15))
        // This applies to manifest AND binary, including redirects. No insecure TLS switches.
        .configure_client(move |client| client_trace.configure(client));
    #[cfg(windows)]
    let builder = {
        let app = app.clone();
        // The official installer handoff now calls this only AFTER a real
        // successful launch. Cancelling elevation leaves current services usable.
        builder.on_before_exit(move || {
            if let Err(error) = shutdown_update_children(&app) {
                eprintln!("[NOVA_UPDATE] stage=installer_handoff_shutdown message={error}");
            }
            app.cleanup_before_exit();
        })
    };
    let result = builder.build().map_err(|e| e.to_string())?.check().await;
    trace.finish(if result.is_ok() { "success" } else { "failed" });
    let result = result.map_err(|error| {
        let reason = trace.failure(network::error_details(&error));
        eprintln!("[NOVA_UPDATE] stage=manifest_failed fallback_reason={reason}");
        reason
    })?;
    if let Some(update) = &result {
        validate_https(update.download_url.as_str())?;
        if update.signature.trim().is_empty() { return Err("更新包缺少签名。".into()); }
    }
    Ok(result.map(|update| CheckedUpdate { update, trace }))
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

fn shutdown_update_children(app: &AppHandle) -> Result<(), String> {
    let supervisor = app.state::<Arc<ProcessSupervisor>>().inner().clone();
    supervisor.stop_admitting();
    // close() retains the ready GPU process by design. Installation instead
    // releases the whole application, including an idle startup Viewer.
    let quick = app.state::<QuickPreviewHost>().request_shutdown();
    app.state::<RenderServiceClient>().request_shutdown();
    let shutdown = supervisor.shutdown(Duration::from_millis(500), Duration::from_millis(1500));
    eprintln!("[NOVA_UPDATE] stage=installer_handoff_children remaining={} elapsed_ms={}", shutdown.remaining.len(), shutdown.elapsed_ms);
    if !shutdown.remaining.is_empty() { return Err("后台任务尚未退出".into()); }
    quick
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
    let started = Instant::now();
    let call = host.checks_started.fetch_add(1, Ordering::Relaxed) + 1;
    let urls = match endpoints(&app) { Ok(v) => v, Err(error) => {
        host.publish(&app, |v| { v.phase = "disabled"; v.message = Some(error); }); return Ok(host.snapshot());
    }};
    host.publish(&app, |v| { v.phase = "checking"; v.message = None; });
    let Some(result) = await_or_cancel(&mut cancel, first_success(urls.len(), |i| check_endpoint(&app, urls[i].clone()))).await else {
        eprintln!("[NOVA_LIFECYCLE] stage=update_query calls={call} cancelled=true elapsed_ms={}", started.elapsed().as_millis());
        return Ok(host.snapshot());
    };
    eprintln!("[NOVA_LIFECYCLE] stage=update_query calls={call} cancelled=false elapsed_ms={}", started.elapsed().as_millis());
    match result {
        Ok((index, Some(checked))) => {
            let CheckedUpdate { update, trace } = checked;
            let skipped = std::fs::read_to_string(&host.skipped_path).unwrap_or_default();
            host.publish(&app, |v| {
                v.phase = if !manual && skipped.trim() == update.version { "skipped" } else { "available" };
                v.version = Some(update.version.clone()); v.notes = update.body.clone();
                v.date = update.date.map(|d| d.date().to_string());
                v.source = Some(network::source_name(&urls[index]));
                v.downloaded = 0; v.total = None;
            });
            eprintln!("[NOVA_UPDATE] stage=release_selected manifest_origin={} download_origin={} version={} target={} signature_present=true unrequested_sources={}",
                network::origin(&urls[index]), network::origin(&update.download_url), update.version, update.target,
                urls[index + 1..].iter().map(network::origin).collect::<Vec<_>>().join(","));
            *host.pending.lock().unwrap() = Some(Pending { update, endpoint: index, bytes: None, trace });
        }
        Ok((index, None)) => {
            eprintln!("[NOVA_UPDATE] stage=current manifest_origin={} unrequested_sources={}", network::origin(&urls[index]),
                urls[index + 1..].iter().map(network::origin).collect::<Vec<_>>().join(","));
            *host.pending.lock().unwrap() = None; host.publish(&app, |v| { v.phase = "current"; v.message = None; v.source = Some(network::source_name(&urls[index])); });
        }
        Err(error) => { return Err(host.fail(&app, format!("更新检查失败：{error}"))); }
    }
    Ok(host.snapshot())
}

#[tauri::command]
pub(crate) fn application_update_cancel(app: AppHandle, window: WebviewWindow, host: State<'_, UpdateHost>) -> Result<UpdateView, String> {
    require_main(&window)?;
    let view = {
        let mut view = host.view.lock().unwrap();
        if matches!(view.phase, "installing" | "restartRequired") { return Err("后台服务已进入退出阶段，请等待安装或重启恢复。".into()); }
        host.cancel.send_modify(|value| *value += 1);
        view.phase = "cancelled"; view.message = None;
        view.clone()
    };
    let _ = app.emit_to("main", EVENT, &view);
    Ok(view)
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
    let (original, endpoint, original_trace) = host.pending.lock().unwrap().as_ref().map(|p| (p.update.clone(), p.endpoint, p.trace.clone())).ok_or("请先检查更新。")?;
    let urls = endpoints(&app)?;
    let mut last_error = String::new();
    // Original first, then each configured mirror at most once; never switch release identity.
    let order = std::iter::once(endpoint).chain((0..urls.len()).filter(|i| *i != endpoint));
    for index in order {
        let CheckedUpdate { mut update, trace } = if index == endpoint { CheckedUpdate { update: original.clone(), trace: original_trace.clone() } } else {
            eprintln!("[NOVA_UPDATE] stage=download_fallback next_manifest_origin={} reason={} expected_version={} signature_identity=unchanged",
                network::origin(&urls[index]), last_error, original.version);
            let checked = tokio::select! { biased;
                _ = cancel.changed() => return Ok(host.snapshot()),
                result = check_endpoint(&app, urls[index].clone()) => result,
            };
            match checked {
                Ok(Some(value)) if same_release(&original.version, &original.signature, &value.update) && value.update.target == original.target => value,
                Ok(_) => { last_error = "镜像尚无同版本、同目标、同签名更新包。".into(); continue; }
                Err(error) => { last_error = error; continue; }
            }
        };
        update.timeout = Some(Duration::from_secs(600));
        trace.begin("download", &update.download_url);
        host.publish(&app, |v| { v.phase = "downloading"; v.downloaded = 0; v.total = None; v.message = None;
            v.source = Some(network::source_name(&update.download_url)); });
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
                trace.finish("signature_verified");
                // Official download returns bytes only AFTER mandatory minisign verification.
                *host.pending.lock().unwrap() = Some(Pending { update, endpoint: index, bytes: Some(bytes), trace });
                host.publish(&app, |v| v.phase = "downloaded"); return Ok(host.snapshot());
            }
            Err(error) => { trace.finish("failed"); last_error = trace.failure(network::error_details(&error)); },
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
    while supervisor.active_children().iter().any(|c| matches!(c.role, ExecutableRole::CoreHelper | ExecutableRole::BuildWorker)) {
        if Instant::now() >= deadline { return Err(host.fail(&app, "任务仍在运行，更新未安装；请任务完成后重试。".into())); }
        tokio::select! { biased;
            _ = cancel.changed() => return Ok(()),
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }
    // The reversible/cancellable stage ends before shutting down shared services.
    let Some(view) = host.commit_installing_after_wait(&cancel) else { return Ok(()); };
    let _ = app.emit_to("main", EVENT, view);
    // Preserve the same write-drain boundary as normal application close.
    let operations = app.state::<ApplicationOperations>().inner().clone();
    let reads = operations.begin_shutdown();
    supervisor.terminate_many(&reads, 0);
    let draining = operations.clone();
    let writes_drained = tauri::async_runtime::spawn_blocking(move || draining.wait_for_writes(Duration::from_secs(8))).await;
    if !matches!(writes_drained, Ok(true)) {
        operations.cancel_shutdown();
        return Err(host.fail(&app, "保存尚未安全结束，更新未安装；完成保存后可重试。".into()));
    }
    #[cfg(not(windows))]
    {
        // Other official platform installers replace files before returning.
        let closing_app = app.clone();
        tauri::async_runtime::spawn_blocking(move || shutdown_update_children(&closing_app))
            .await.map_err(|e| host.restart_required(&app, format!("后台退出失败：{e}")))?
            .map_err(|e| host.restart_required(&app, e))?;
    }
    host.publish(&app, |v| { v.phase = "installing"; v.message = Some("正在安装，完成后自动重启…".into()); });
    let pending = host.pending.lock().unwrap().take().ok_or_else(|| host.restart_required(&app, "更新包已失效".into()))?;
    let bytes = pending.bytes.as_ref().ok_or_else(|| host.restart_required(&app, "更新包已失效".into()))?;
    eprintln!("[NOVA_UPDATE] stage=install version={} target={} bundle_type={:?} data_cleanup=false permission_flow=official_updater restart=after_install", pending.update.version, pending.update.target, tauri::utils::platform::bundle_type());
    let result = pending.update.install(bytes);
    if let Err(error) = result {
        *host.pending.lock().unwrap() = Some(pending);
        #[cfg(windows)]
        {
            // Launch/UAC failed before on_before_exit: no services were stopped.
            // Keep the signed payload and allow retry without restarting old EXE.
            operations.cancel_shutdown();
            let message = format!("更新安装器未启动：{error}；更新未安装，可重试或手动运行新版安装包。");
            host.publish(&app, |v| { v.phase = "downloaded"; v.message = Some(message.clone()); });
            return Err(message);
        }
        #[cfg(not(windows))]
        return Err(host.restart_required(&app, format!("更新安装器未成功启动或安装失败：{error}")));
    }
    #[cfg(not(windows))]
    app.restart();
    Ok(())
}

/// Dispatch before graphics, windows, backend services and single-instance setup.
/// The plan is inspection only; real installation always stays behind the
/// existing draft preservation, shutdown, signature and official installer path.
pub(crate) fn run_cli(args: &[std::ffi::OsString], config: &tauri::utils::config::Config, version: &str) -> Option<i32> {
    let has = |flag: &str| args.iter().any(|value| value == flag);
    let diagnostic = has("--updater-diagnostics");
    let plan = has("--updater-install-plan");
    if !diagnostic && !plan { return None; }
    let argument = |flag: &str| args.iter().position(|value| value == flag).and_then(|index| args.get(index + 1)).and_then(|value| value.to_str()).map(str::to_owned);
    let result = (|| {
        let updater = config.plugins.0.get("updater").ok_or("尚未配置更新服务。")?;
        let mut urls = configured_endpoints(updater)?;
        // Explicit fault injection for this diagnostic process only. The
        // application configuration and regular update chain are never changed.
        if let Some(primary) = argument("--updater-primary") { urls[0] = validate_https(&primary)?; }
        let bundle = tauri::utils::platform::bundle_type();
        let base = tauri_plugin_updater::target().ok_or("当前平台没有更新目标。")?;
        let target = argument("--updater-target").unwrap_or_else(|| match bundle {
            Some(tauri::utils::config::BundleType::Deb) => format!("{base}-deb"),
            Some(tauri::utils::config::BundleType::AppImage) => format!("{base}-appimage"),
            _ => base,
        });
        let mut report = serde_json::json!({ "currentVersion": version, "target": target, "bundleType": format!("{bundle:?}"),
            "officialUpdaterVersion": "2.10.0", "systemProxyFeature": true,
            "installPlan": {
                "linuxDeb": "signed .deb; official dpkg -i with pkexec / graphical sudo / sudo permission flow",
                "linuxAppImage": "signed AppImage or AppImage tar.gz; official in-place executable replacement",
                "macOS": "signed architecture-specific .app.tar.gz, not DMG; official bundle replacement with administrator permission when needed",
                "windows": "signed NSIS payload; official passive /P /R preserving the existing installation identity",
                "dataCleanup": false, "restart": "only after successful installation", "installationExecuted": false, "restartVerified": false
            }
        });
        if diagnostic { report["network"] = tauri::async_runtime::block_on(network::probe(&urls, &target))?; }
        Ok::<_, String>(report)
    })();
    match result {
        Ok(report) => { println!("{}", serde_json::to_string_pretty(&report).unwrap()); Some(0) }
        Err(error) => { eprintln!("[NOVA_UPDATE_DIAGNOSTIC] result=failed reason={error}"); Some(1) }
    }
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
    #[tokio::test]
    async fn closing_cancels_network_before_late_success_and_blocks_new_checks() {
        let host = UpdateHost::new("0.2.0".into(), PathBuf::new());
        let mut receiver = host.cancel.subscribe();
        host.cancel_for_shutdown();
        assert!(await_or_cancel(&mut receiver, std::future::ready("late update")).await.is_none());
        assert!(host.acquire().is_err());
        host.resume_after_cancelled_shutdown();
        assert!(host.acquire().is_ok());
    }
    #[tokio::test]
    async fn native_close_waiting_cancels_reversible_wait_before_late_installer() {
        let host = UpdateHost::new("0.2.0".into(), PathBuf::new());
        let mut cancel = host.cancel.subscribe();
        let busy = host.acquire().unwrap();
        host.view.lock().unwrap().phase = "waiting";
        assert!(!host.prevents_close());
        let view = host.prepare_close_state().expect("cancellable waiting permits the normal draft close gate");
        assert_eq!(view.phase, "cancelled");
        assert!(view.message.is_none());
        assert!(await_or_cancel(&mut cancel, std::future::ready("late idle-worker result")).await.is_none());
        // Even after changed() consumed the signal, the same phase barrier
        // prevents that old future crossing into irreversible installation.
        assert!(!cancel.has_changed().unwrap());
        assert!(host.commit_installing_after_wait(&cancel).is_none());
        assert_eq!(host.snapshot().phase, "cancelled");
        assert!(host.prepare_close_state().is_some(), "a repeated X does not wait for background workers");
        drop(busy);
        assert!(host.acquire().is_ok(), "a failed draft close can retry normal updater operations");
    }
    #[test]
    fn native_close_waiting_committed_install_preserves_critical_protection() {
        let host = UpdateHost::new("0.2.0".into(), PathBuf::new());
        let cancel = host.cancel.subscribe();
        host.view.lock().unwrap().phase = "waiting";
        assert_eq!(host.commit_installing_after_wait(&cancel).unwrap().phase, "installing");
        assert!(host.prevents_close());
        assert!(host.prepare_close_state().is_none());
        assert!(!cancel.has_changed().unwrap(), "X must not interrupt an actual install");
        assert_eq!(host.snapshot().phase, "installing");
    }
    #[test]
    fn native_close_waiting_cancel_signal_blocks_empty_worker_fast_path() {
        let host = UpdateHost::new("0.2.0".into(), PathBuf::new());
        let cancel = host.cancel.subscribe();
        host.view.lock().unwrap().phase = "waiting";
        host.cancel.send_modify(|value| *value += 1);
        assert!(host.commit_installing_after_wait(&cancel).is_none(), "no active children does not bypass cancellation");
        assert_eq!(host.snapshot().phase, "waiting");
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

    #[tokio::test]
    async fn closing_cancels_an_actual_loopback_http_body_without_waiting_for_download() {
        // Transport/cancellation fixture only: production HTTPS and updater
        // signature policy remain unchanged. No external site is requested.
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = Arc::clone(&stop);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            stream.set_write_timeout(Some(Duration::from_secs(1))).unwrap();
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).unwrap();
            assert!(std::str::from_utf8(&request[..read]).unwrap().starts_with("GET /update-fixture "));
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1048576\r\nConnection: close\r\n\r\n").unwrap();
            stream.write_all(&[7_u8; 1024]).unwrap();
            let _ = started_tx.send(());
            let mut sent = 1024;
            for _ in 0..64 {
                if server_stop.load(Ordering::Acquire) { break; }
                std::thread::sleep(Duration::from_millis(10));
                if stream.write_all(&[7_u8; 1024]).is_err() { break; }
                sent += 1024;
            }
            let _ = stream.shutdown(std::net::Shutdown::Both);
            sent
        });
        let host = UpdateHost::new("0.2.0".into(), PathBuf::new());
        let mut receiver = host.cancel.subscribe();
        let network = tokio::spawn(async move {
            await_or_cancel(&mut receiver, async {
                let response = reqwest::Client::new().get(format!("http://{address}/update-fixture")).send().await.unwrap();
                response.bytes().await
            }).await
        });
        tokio::time::timeout(Duration::from_secs(2), started_rx).await.unwrap().unwrap();
        let close_started = Instant::now();
        host.cancel_for_shutdown();
        assert!(tokio::time::timeout(Duration::from_secs(1), network).await.unwrap().unwrap().is_none());
        assert!(host.acquire().is_err());
        assert!(host.pending.lock().unwrap().is_none(), "incomplete download is not an installed or verified package");
        stop.store(true, Ordering::Release);
        let sent = server.join().unwrap();
        assert!(sent < 1048576);
        println!("[NOVA_LIFECYCLE_PROFILE] case=download transport=loopback_http body_cancel_ms={} server_bytes={} incomplete=true retained_update_bytes=0 new_requests=0 window_response=UNMEASURED", close_started.elapsed().as_millis(), sent);
    }
}
