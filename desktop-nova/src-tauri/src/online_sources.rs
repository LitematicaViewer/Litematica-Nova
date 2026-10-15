//! Read-only observed website adapters. No cookies, login automation or JavaScript execution.
use crate::application::operations::ApplicationOperations;
use reqwest::{
    blocking::{Client, Request, RequestBuilder, Response},
    header::HeaderMap,
    StatusCode, Url,
};
use scraper::{Html, Selector};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex, OnceLock,
    },
    time::{Duration, Instant, SystemTime},
};

const GAP: Duration = Duration::from_secs(2);
const CACHE_TTL: Duration = Duration::from_secs(30);
const CACHE_LIMIT: usize = 64;
static TICKET: AtomicU64 = AtomicU64::new(1);
#[derive(Default)]
struct LaneState {
    busy: bool,
    next: Option<Instant>,
    cooldown: Option<Instant>,
    queue: VecDeque<u64>,
}
#[derive(Default)]
struct Lane {
    state: Mutex<LaneState>,
    changed: Condvar,
}
struct Permit(Arc<Lane>);
impl Drop for Permit {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.busy = false;
            state.next = Some(Instant::now() + GAP);
            self.0.changed.notify_all();
        }
    }
}
type Cache = VecDeque<(String, Instant, Value)>;
struct Http {
    client: Client,
    lanes: Mutex<HashMap<String, Arc<Lane>>>,
    cancelled: Mutex<HashMap<String, Arc<AtomicBool>>>,
    cache: Mutex<Cache>,
    items: Mutex<VecDeque<(String, Value)>>,
}
static HTTP: OnceLock<Result<Http, String>> = OnceLock::new();
static CLOSING: AtomicBool = AtomicBool::new(false);
fn http() -> Result<&'static Http, String> {
    HTTP.get_or_init(|| {
        Client::builder()
            .cookie_store(true)
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Litematica-Nova/0.2 limited read-only library")
            .build()
            .map(|client| Http {
                client,
                lanes: Mutex::new(HashMap::new()),
                cancelled: Mutex::new(HashMap::new()),
                cache: Mutex::new(VecDeque::new()),
                items: Mutex::new(VecDeque::new()),
            })
            .map_err(|error| error.to_string())
    })
    .as_ref()
    .map_err(Clone::clone)
}
fn token(id: Option<&str>) -> Result<Arc<AtomicBool>, String> {
    if let Some(id) = id {
        let mut tokens = http()?
            .cancelled
            .lock()
            .map_err(|error| error.to_string())?;
        if tokens.len() > 256 {
            tokens.retain(|_, value| Arc::strong_count(value) > 1);
        }
        Ok(tokens
            .entry(id.to_owned())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone())
    } else {
        Ok(Arc::new(AtomicBool::new(false)))
    }
}
fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if CLOSING.load(Ordering::Acquire) || cancel.load(Ordering::Acquire) {
        Err("请求已取消，未新增网站请求".into())
    } else {
        Ok(())
    }
}
pub(crate) fn cancel_for_shutdown() {
    CLOSING.store(true, Ordering::Release);
    // Do not initialize a networking client just to shut it down.
    if let Some(Ok(http)) = HTTP.get() {
        if let Ok(tokens) = http.cancelled.lock() {
            for token in tokens.values() { token.store(true, Ordering::Release); }
        }
        if let Ok(lanes) = http.lanes.lock() {
            for lane in lanes.values() { lane.changed.notify_all(); }
        }
    }
}
pub(crate) fn resume_after_cancelled_shutdown() { CLOSING.store(false, Ordering::Release); }
fn acquire(lane: Arc<Lane>, cancel: &AtomicBool) -> Result<Permit, String> {
    let ticket = TICKET.fetch_add(1, Ordering::Relaxed);
    let mut state = lane.state.lock().map_err(|error| error.to_string())?;
    state.queue.push_back(ticket);
    loop {
        let now = Instant::now();
        let failure = check_cancel(cancel).err().or_else(|| {
            state.cooldown.filter(|until| *until > now).map(|until| {
                format!(
                    "站点限流冷却中，请至少 {} 秒后重试",
                    until.duration_since(now).as_secs() + 1
                )
            })
        });
        if let Some(error) = failure {
            state.queue.retain(|value| *value != ticket);
            lane.changed.notify_all();
            return Err(error);
        }
        if !state.busy
            && state.queue.front() == Some(&ticket)
            && state.next.is_none_or(|until| until <= now)
        {
            state.queue.pop_front();
            state.busy = true;
            drop(state);
            return Ok(Permit(lane));
        }
        state = lane
            .changed
            .wait_timeout(state, Duration::from_millis(50))
            .map_err(|error| error.to_string())?
            .0;
    }
}
fn retry_delay(value: Option<&str>, now: SystemTime) -> Duration {
    value
        .and_then(|value| {
            value
                .parse::<u64>()
                .ok()
                .map(Duration::from_secs)
                .or_else(|| {
                    httpdate::parse_http_date(value)
                        .ok()
                        .and_then(|date| date.duration_since(now).ok())
                })
        })
        .unwrap_or(Duration::from_secs(60))
        .max(Duration::from_secs(60))
}
fn lane_for(url: &Url) -> Result<Arc<Lane>, String> {
    let host = url.host_str().ok_or("网站URL缺少主机")?;
    if !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("网站URL无效".into());
    }
    Ok(http()?
        .lanes
        .lock()
        .map_err(|error| error.to_string())?
        .entry(host.to_owned())
        .or_default()
        .clone())
}
fn execute(
    request: Request,
    permit: Permit,
    cancel: Arc<AtomicBool>,
) -> Result<LimitedResponse, String> {
    check_cancel(&cancel)?;
    let response = http()?
        .client
        .execute(request)
        .map_err(|error| format!("网站请求失败：{}", error.without_url()))?;
    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        let delay = retry_delay(
            response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok()),
            SystemTime::now(),
        );
        permit
            .0
            .state
            .lock()
            .map_err(|error| error.to_string())?
            .cooldown = Instant::now().checked_add(delay);
    }
    Ok(LimitedResponse {
        response: Some(response),
        _permit: permit,
        cancel,
    })
}
pub(crate) struct LimitedResponse {
    response: Option<Response>,
    _permit: Permit,
    cancel: Arc<AtomicBool>,
}
impl LimitedResponse {
    pub(crate) fn status(&self) -> StatusCode {
        self.response.as_ref().unwrap().status()
    }
    pub(crate) fn headers(&self) -> &HeaderMap {
        self.response.as_ref().unwrap().headers()
    }
    pub(crate) fn text(mut self) -> Result<String, String> {
        let text = self
            .response
            .take()
            .unwrap()
            .text()
            .map_err(|error| error.without_url().to_string())?;
        check_cancel(&self.cancel)?;
        Ok(text)
    }
    pub(crate) fn bytes(mut self) -> Result<Vec<u8>, String> {
        let bytes = self
            .response
            .take()
            .unwrap()
            .bytes()
            .map_err(|error| error.without_url().to_string())?;
        check_cancel(&self.cancel)?;
        Ok(bytes.to_vec())
    }
}
pub(crate) fn send_request(
    builder: RequestBuilder,
    id: Option<&str>,
) -> Result<LimitedResponse, String> {
    let request = builder
        .build()
        .map_err(|error| error.without_url().to_string())?;
    if request.method() != reqwest::Method::GET {
        return Err("在线来源仅接受只读GET请求".into());
    }
    let cancel = token(id)?;
    let permit = acquire(lane_for(request.url())?, &cancel)?;
    execute(request, permit, cancel)
}

/// The existing resource source issues one public-session token POST. It shares
/// the same host lane, cancellation and Retry-After handling as every GET.
pub(crate) fn send_resource_request(builder: RequestBuilder, id: Option<&str>) -> Result<LimitedResponse, String> {
    let request = builder.build().map_err(|error| error.without_url().to_string())?;
    let url = request.url();
    let token_post = request.method() == reqwest::Method::POST
        && url.scheme() == "https" && url.host_str() == Some("ccvaults.com")
        && url.port_or_known_default() == Some(443) && url.path() == "/api/token"
        && url.query().is_none() && url.username().is_empty() && url.password().is_none();
    if request.method() != reqwest::Method::GET && !token_post {
        return Err("资源请求仅接受只读GET或原站会话接口".into());
    }
    let cancel = token(id)?;
    let permit = acquire(lane_for(url)?, &cancel)?;
    execute(request, permit, cancel)
}
fn cache_get(key: &str) -> Result<Option<Value>, String> {
    let mut cache = http()?.cache.lock().map_err(|error| error.to_string())?;
    cache.retain(|(_, at, _)| at.elapsed() < CACHE_TTL);
    Ok(cache
        .iter()
        .find(|(name, _, _)| name == key)
        .map(|(_, _, value)| value.clone()))
}
pub(crate) fn get_json(url: Url, id: Option<&str>) -> Result<Value, String> {
    let cancel = token(id)?;
    check_cancel(&cancel)?;
    let key = url.to_string();
    if let Some(value) = cache_get(&key)? {
        return Ok(value);
    }
    let permit = acquire(lane_for(&url)?, &cancel)?;
    // A previous queued query may have filled the cache. Do not send it twice.
    if let Some(value) = cache_get(&key)? {
        return Ok(value);
    }
    let response = execute(
        http()?
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .build()
            .map_err(|error| error.without_url().to_string())?,
        permit,
        cancel,
    )?;
    let status = response.status();
    let body = response.text()?;
    if !status.is_success() {
        return Err(format!(
            "站点返回HTTP {status}；没有自动重试，认证/验证请在原站完成"
        ));
    }
    let value: Value = serde_json::from_str(&body)
        .map_err(|_| "站点未返回可解析JSON（可能处于维护或验证页面）".to_string())?;
    let mut cache = http()?.cache.lock().map_err(|error| error.to_string())?;
    if cache.len() >= CACHE_LIMIT {
        cache.pop_front();
    }
    cache.push_back((key, Instant::now(), value.clone()));
    Ok(value)
}
fn get_html(url: Url, id: &str) -> Result<String, String> {
    let response = send_request(http()?.client.get(url), Some(id))?;
    let status = response.status();
    let body = response.text()?;
    if !status.is_success() {
        return Err(format!("原站返回HTTP {status}，请在原站完成验证"));
    }
    Ok(body)
}
fn url(base: &str, params: &[(&str, &str)]) -> Result<Url, String> {
    Url::parse_with_params(base, params).map_err(|error| error.to_string())
}
fn selector(value: &str) -> Result<Selector, String> {
    Selector::parse(value).map_err(|_| "站点HTML选择器异常".into())
}
fn scalar(value: &Value) -> String {
    value.as_str().map(str::to_owned).unwrap_or_else(|| {
        value
            .as_u64()
            .map(|value| value.to_string())
            .unwrap_or_default()
    })
}
fn version_names(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_str().or_else(|| row["name"].as_str()))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
fn mcodex_detail(key: &str, row: &Value) -> Value {
    let summary = Html::parse_fragment(
        row["translation"]["description_html"]
            .as_str()
            .unwrap_or(""),
    )
    .root_element()
    .text()
    .collect::<String>();
    json!({"key":key,"name":row["translation"]["title"],"summary":summary,"versions":version_names(&row["versions"]),"author":{"username":row["owner"]["display_name"]},"pageUrl":format!("https://mcodex.org/posts/{key}"),"downloadListAvailable":true,"sourceId":"mcodex"})
}
fn mcodex_cdn(path: &str) -> Result<Url, String> {
    let target = Url::parse("https://cdn.mcodex.org/")
        .unwrap()
        .join(path)
        .map_err(|error| error.to_string())?;
    if target.scheme() != "https"
        || target.host_str() != Some("cdn.mcodex.org")
        || !target.username().is_empty()
        || target.password().is_some()
    {
        return Err("MCodex附件地址与真实CDN链路不符".into());
    }
    Ok(target)
}
fn remember(source: &str, item: Value) -> Result<Value, String> {
    let key = format!("{source}:{}", scalar(&item["key"]));
    let mut items = http()?.items.lock().map_err(|error| error.to_string())?;
    items.retain(|(name, _)| name != &key);
    if items.len() >= 128 {
        items.pop_front();
    }
    items.push_back((key, item.clone()));
    Ok(item)
}
fn recalled(source: &str, id: &str) -> Result<Value, String> {
    http()?
        .items
        .lock()
        .map_err(|error| error.to_string())?
        .iter()
        .find(|(name, _)| name == &format!("{source}:{id}"))
        .map(|(_, item)| item.clone())
        .ok_or_else(|| "请先搜索并选择该来源中的结果".into())
}
fn native_list(source: &str, rows: &[Value]) -> Result<Vec<Value>, String> {
    rows.iter().map(|row| {
        let item = match source {
            "projection-workshop" => json!({"key":scalar(&row["postId"]),"name":row["title"],"summary":row["summary"],"author":{"username":row["authorName"]},"pageUrl":format!("https://www.mcimg.com/forum-post/{}.html",scalar(&row["postId"])),"sourceId":source}),
            "minemev" => {
                let page = row["url_redirect"].as_str().filter(|value| !value.is_empty()).map(str::to_owned).unwrap_or_else(|| format!("https://www.minemev.com/p/{}/{}", scalar(&row["vendor"]), scalar(&row["uuid"])));
                let page=Url::parse(&page).map_err(|_|"Minemev原页地址无效")?;
                if page.scheme()!="https" || !page.username().is_empty() || page.password().is_some() { return Err("Minemev原页未提供有效HTTPS地址".into()); }
                json!({"key":scalar(&row["uuid"]),"name":row["post_name"],"summary":row["description"],"versions":row["versions"],"downloads":row["downloads"],"author":{"username":row["User"]},"pageUrl":page.to_string(),"sourceId":source})
            },
            "mcodex" => json!({"key":scalar(&row["id"]),"name":row["title"],"summary":row["description"],"versions":version_names(&row["versions"]),"author":{"username":row["owner"]["display_name"]},"pageUrl":format!("https://mcodex.org/posts/{}",scalar(&row["id"])),"sourceId":source}),
            _ => { let mut item = row.clone(); item["sourceId"] = source.into(); item["pageUrl"] = format!("https://redenmc.com/litematica/{}",scalar(&row["key"])).into(); item },
        }; remember(source, item)
    }).collect()
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SearchInput {
    source_id: String,
    query: String,
    request_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DetailInput {
    source_id: String,
    item_id: String,
    request_id: String,
    #[serde(default)]
    download_files: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DownloadInput {
    source_id: String,
    item_id: String,
    attachment_index: usize,
    request_id: String,
    sizes: Option<Value>,
}
fn search(input: SearchInput) -> Result<Value, String> {
    let query = input.query.trim();
    if query.chars().count() > 128 {
        return Err("搜索关键词过长".into());
    }
    let id = Some(input.request_id.as_str());
    let (raw, field) = match input.source_id.as_str() {
        "reden" => (
            get_json(
                url(
                    "https://redenmc.com/api/mc-services/litematica/search",
                    &[("q", query), ("lang", "zh_cn"), ("pageSize", "24")],
                )?,
                id,
            )?,
            "d",
        ),
        "projection-workshop" => (
            get_json(
                url(
                    "https://www.mcimg.com/shimu/api/forum/post/list",
                    &[
                        ("keyword", query),
                        ("searchType", "all"),
                        ("pageNum", "1"),
                        ("pageSize", "20"),
                        ("sort", "new"),
                    ],
                )?,
                id,
            )?,
            "rows",
        ),
        "minemev" => (
            get_json(
                url(
                    "https://www.minemev.com/api/v2/search",
                    &[("search", query), ("page", "1"), ("pagesize", "24")],
                )?,
                id,
            )?,
            "posts",
        ),
        "mcodex" => (
            get_json(
                url(
                    "https://api.mcodex.org/api/posts",
                    &[
                        ("search", query),
                        ("page", "1"),
                        ("sort", "latest"),
                        ("otherLang", "true"),
                    ],
                )?,
                id,
            )?,
            "data",
        ),
        "abfielder" => {
            let body = get_html(
                url(
                    "https://abfielder.com/Products/BrowseProducts.php",
                    &[("search", query), ("page", "1")],
                )?,
                &input.request_id,
            )?;
            let document = Html::parse_document(&body);
            let links = selector("a[href*='ProductDetails.php?id=']")?;
            let title = selector("h5.card-title")?;
            let mut items = Vec::new();
            for link in document.select(&links) {
                let target = Url::parse("https://abfielder.com/")
                    .unwrap()
                    .join(link.value().attr("href").unwrap_or(""))
                    .map_err(|error| error.to_string())?;
                let Some(key) = target
                    .query_pairs()
                    .find(|(name, _)| name == "id")
                    .map(|(_, value)| value.into_owned())
                else {
                    continue;
                };
                if items.iter().any(|item: &Value| item["key"] == key) {
                    continue;
                }
                let card = link
                    .ancestors()
                    .filter_map(scraper::ElementRef::wrap)
                    .find(|parent| parent.value().classes().any(|value| value == "card"));
                let name = card
                    .and_then(|card| card.select(&title).next())
                    .map(|item| item.text().collect::<String>())
                    .unwrap_or_else(|| link.text().collect());
                if name.trim().is_empty() {
                    continue;
                }
                items.push(remember("abfielder", json!({"key":key,"name":name.trim(),"pageUrl":target.to_string(),"sourceId":"abfielder"}))?);
                if items.len() == 24 {
                    break;
                }
            }
            return Ok(json!({"d":items,"count":items.len()}));
        }
        _ => return Err("该来源当前实际响应受维护/验证条件限制，请打开原站；未猜测接口".into()),
    };
    let rows = raw[field]
        .as_array()
        .ok_or("站点响应结构已变化，未伪造搜索结果")?;
    let items = native_list(&input.source_id, rows)?;
    Ok(
        json!({"d":items,"count":raw.get("count").or_else(|| raw.get("total")).or_else(|| raw.get("total_items")).cloned().unwrap_or(json!(items.len()))}),
    )
}
fn detail(input: &DetailInput) -> Result<Value, String> {
    let key = input.item_id.as_str();
    if key.is_empty() || key.len() > 160 || key.contains('/') || key.contains('?') {
        return Err("来源条目标识无效".into());
    }
    match input.source_id.as_str() {
        "reden" => {
            let raw = get_json(
                url(
                    &format!("https://redenmc.com/api/mc-services/yisibite/{key}/info/en"),
                    &[],
                )?,
                Some(&input.request_id),
            )?;
            let mut item = raw["d"].get(0).ok_or("Reden未返回详情")?.clone();
            item["key"] = key.into();
            item["sourceId"] = "reden".into();
            item["pageUrl"] = format!("https://redenmc.com/litematica/{key}").into();
            remember("reden", item)
        }
        "projection-workshop" => {
            let raw = get_json(
                url(
                    &format!("https://www.mcimg.com/shimu/api/forum/post/{key}"),
                    &[],
                )?,
                Some(&input.request_id),
            )?;
            if raw["code"] != 200 {
                return Err("投影工坊详情需要原站验证或不可访问".into());
            }
            let row = &raw["data"];
            let document = Html::parse_fragment(row["content"].as_str().unwrap_or(""));
            let mut attachments = Vec::new();
            for file in document.select(&selector(".re-file[data-name]")?) {
                let name = file.value().attr("data-name").unwrap_or("");
                if !name.to_lowercase().ends_with(".litematic") {
                    continue;
                }
                // Never use data-url to bypass the official resource/login/paid gate.
                attachments.push(json!({"name":name,"size":file.value().attr("data-size").and_then(|value|value.parse::<u64>().ok()),"url":url(&format!("https://www.mcimg.com/shimu/api/forum/post/{key}/resource"), &[("name",name)])?.to_string()}));
            }
            remember(
                "projection-workshop",
                json!({"key":key,"name":row["title"],"summary":document.root_element().text().collect::<String>(),"author":{"username":row["authorName"]},"pageUrl":format!("https://www.mcimg.com/forum-post/{key}.html"),"attachments":attachments,"sourceId":"projection-workshop"}),
            )
        }
        "mcodex" => {
            let raw = get_json(
                url(
                    &format!("https://api.mcodex.org/api/posts/{key}"),
                    &[("lang", "en")],
                )?,
                Some(&input.request_id),
            )?;
            if raw["success"] != true {
                return Err("MCodex未返回可访问详情".into());
            }
            let row = &raw["data"];
            let mut item = mcodex_detail(key, row);
            if input.download_files {
                // The website also keeps this query disabled until the user clicks download.
                let files = get_json(
                    url(
                        &format!("https://api.mcodex.org/api/posts/downloads/{key}"),
                        &[],
                    )?,
                    Some(&input.request_id),
                )?;
                if files["success"] != true {
                    return Err("MCodex未返回可访问附件列表".into());
                }
                let paths = files["data"]["schem_urls"]
                    .as_array()
                    .ok_or("MCodex附件结构已变化")?;
                let attachments=paths.iter().filter_map(Value::as_str).map(|path|Ok(json!({"name":path.split('-').skip(1).collect::<Vec<_>>().join("-"),"url":mcodex_cdn(path)?.to_string()}))).collect::<Result<Vec<Value>,String>>()?;
                item["attachments"] = json!(attachments);
                item["downloadListAvailable"] = false.into();
            }
            remember("mcodex", item)
        }
        "abfielder" => {
            let target = url(
                "https://abfielder.com/Products/ProductDetails.php",
                &[("id", key)],
            )?;
            let body = get_html(target.clone(), &input.request_id)?;
            let document = Html::parse_document(&body);
            let mut item = recalled("abfielder", key)?;
            let attachments: Vec<Value> = document.select(&selector("a[href]")?).filter_map(|link| {
                let href = link.value().attr("href")?; if !href.to_ascii_lowercase().contains("download") { return None; }
                let target = target.join(href).ok()?; if target.host_str() != Some("abfielder.com") { return None; }
                Some(json!({"name":link.text().collect::<String>().trim(),"url":target.to_string()}))
            }).collect();
            item["attachments"] = json!(attachments);
            remember("abfielder", item)
        }
        "minemev" => recalled("minemev", key),
        _ => Err("当前来源详情需要在原站完成".into()),
    }
}
fn download(
    input: DownloadInput,
    operations: ApplicationOperations,
) -> Result<crate::RedenDownloadOutput, String> {
    let source = input.source_id.as_str();
    let id = input.item_id.as_str();
    if source == "minemev" {
        let item = recalled(source, id)?;
        return Ok(crate::RedenDownloadOutput::ExternalLink {
            url: scalar(&item["pageUrl"]),
            provider: "minemev",
        });
    }
    if source == "reden" {
        let target = if let Some(sizes) = &input.sizes {
            let mut target = url(
                &format!("https://redenmc.com/api/mc-services/yisibite/{id}"),
                &[],
            )?;
            for field in ["xSize", "ySize", "zSize"] {
                if let Some(value) = sizes[field].as_u64() {
                    target
                        .query_pairs_mut()
                        .append_pair(field, &value.to_string());
                }
            }
            target
        } else {
            url(
                &format!(
                    "https://redenmc.com/api/mc-services/yisibite/{id}/download/{}",
                    input.attachment_index + 1
                ),
                &[],
            )?
        };
        return crate::reden_fetch_download(
            target,
            &format!("{id}.litematic"),
            &operations,
            Some(&input.request_id),
        );
    }
    let mut target = if source == "mcodex" {
        let raw = get_json(
            url(
                &format!("https://api.mcodex.org/api/posts/downloads/{id}"),
                &[],
            )?,
            Some(&input.request_id),
        )?;
        if raw["success"] != true {
            return Err("MCodex未授权下载该附件".into());
        }
        let path = raw["data"]["schem_urls"]
            .get(input.attachment_index)
            .and_then(Value::as_str)
            .ok_or("MCodex没有可用投影附件")?;
        mcodex_cdn(path)?
    } else {
        let item = detail(&DetailInput {
            source_id: input.source_id.clone(),
            item_id: input.item_id.clone(),
            request_id: input.request_id.clone(),
            download_files: false,
        })?;
        url(
            item["attachments"]
                .get(input.attachment_index)
                .and_then(|item| item["url"].as_str())
                .ok_or("原站未提供可用直接附件，请打开原页")?,
            &[],
        )?
    };
    let home = match source {
        "projection-workshop" => "https://www.mcimg.com/",
        "mcodex" => "https://mcodex.org/",
        "abfielder" => "https://abfielder.com/",
        _ => return Err("该站下载需在原站完成".into()),
    };
    for _ in 0..6 {
        if let Some(provider) = crate::reden_external_drive(&target) {
            return Ok(crate::RedenDownloadOutput::ExternalLink {
                url: target.to_string(),
                provider,
            });
        }
        if target.scheme() != "https" {
            return Err("下载跳转未提供HTTPS附件".into());
        }
        let response = send_request(
            http()?
                .client
                .get(target.clone())
                .header(reqwest::header::REFERER, home),
            Some(&input.request_id),
        )?;
        let status = response.status();
        if status.is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or("下载跳转缺少地址")?;
            target = target.join(location).map_err(|error| error.to_string())?;
            continue;
        }
        let headers = response.headers().clone();
        let mime = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let bytes = response.bytes()?;
        if !status.is_success() {
            return Err(format!(
                "原站下载返回HTTP {status}；登录/付费/验证请在原站完成，没有自动重试"
            ));
        }
        if crate::reden_looks_html(mime, &bytes) && source == "abfielder" {
            let document = Html::parse_document(
                std::str::from_utf8(&bytes).map_err(|_| "下载页不是UTF8 HTML")?,
            );
            let href = document
                .select(&selector("#downloadLink")?)
                .next()
                .and_then(|link| link.value().attr("href"))
                .ok_or("原站下载页需要登录/验证，未获取附件")?;
            target = target.join(href).map_err(|error| error.to_string())?;
            continue;
        }
        if crate::reden_looks_html(mime, &bytes) || bytes.is_empty() {
            return Err("原站返回网页或空内容，不会当作投影入库".into());
        }
        let name = crate::file_name_from_headers_or_url(
            &headers,
            &target,
            &format!("{source}-{id}.litematic"),
        );
        return crate::reden_save_projection(
            &crate::reden_download_dir()?,
            &name,
            &bytes,
            |temporary| {
                operations
                    .inspect_project(crate::application::operations::InspectProjectInput {
                        source_path: temporary.to_string_lossy().into(),
                    })
                    .map(|_| ())
            },
        );
    }
    Err("下载重定向未完成；没有继续请求".into())
}
#[tauri::command]
pub(crate) async fn application_online_search(input: SearchInput) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || search(input))
        .await
        .map_err(|error| error.to_string())?
}
#[tauri::command]
pub(crate) async fn application_online_detail(input: DetailInput) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || detail(&input))
        .await
        .map_err(|error| error.to_string())?
}
#[tauri::command]
pub(crate) async fn application_online_download(
    input: DownloadInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<crate::RedenDownloadOutput, String> {
    let operations = operations.inner().clone();
    tauri::async_runtime::spawn_blocking(move || download(input, operations))
        .await
        .map_err(|error| error.to_string())?
}
#[tauri::command]
pub(crate) fn application_online_cancel(request_id: String) -> Result<(), String> {
    token(Some(&request_id))?.store(true, Ordering::Release);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_after_seconds_and_http_date_are_respected() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        assert_eq!(retry_delay(Some("120"), now), Duration::from_secs(120));
        assert_eq!(retry_delay(Some("1"), now), Duration::from_secs(60));
        let date = httpdate::fmt_http_date(now + Duration::from_secs(300));
        assert_eq!(retry_delay(Some(&date), now), Duration::from_secs(300));
    }
    #[test]
    fn cancelled_and_cooling_waiters_never_take_network_permit() {
        let lane = Arc::new(Lane::default());
        assert!(acquire(lane.clone(), &AtomicBool::new(true)).is_err());
        assert!(lane.state.lock().unwrap().queue.is_empty());
        lane.state.lock().unwrap().cooldown = Some(Instant::now() + Duration::from_secs(60));
        assert!(acquire(lane.clone(), &AtomicBool::new(false)).is_err());
        assert!(!lane.state.lock().unwrap().busy);
    }
    #[test]
    fn host_queue_is_single_concurrency_and_cache_is_bounded() {
        let lane = Arc::new(Lane::default());
        let permit = acquire(lane.clone(), &AtomicBool::new(false)).unwrap();
        assert!(lane.state.lock().unwrap().busy);
        drop(permit);
        assert!(!lane.state.lock().unwrap().busy);
        assert!(lane.state.lock().unwrap().next.unwrap() > Instant::now());
        assert_eq!(CACHE_LIMIT, 64);
    }
    #[test]
    fn busy_host_waiter_is_cancelled_without_another_permit() {
        let lane = Arc::new(Lane::default());
        let held = acquire(lane.clone(), &AtomicBool::new(false)).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let waiter_lane = lane.clone();
        let waiter_cancel = cancel.clone();
        let waiter = std::thread::spawn(move || acquire(waiter_lane, &waiter_cancel));
        for _ in 0..100 {
            if !lane.state.lock().unwrap().queue.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(lane.state.lock().unwrap().queue.len(), 1);
        cancel.store(true, Ordering::Release);
        lane.changed.notify_all();
        assert!(waiter.join().unwrap().is_err());
        assert!(lane.state.lock().unwrap().queue.is_empty());
        assert!(lane.state.lock().unwrap().busy);
        drop(held);
    }
    #[test]
    fn mcodex_observed_fields_and_original_route_are_preserved() {
        let row = json!({"translation":{"title":"Bartering Farm","description_html":"<p>Real <b>description</b></p>"},"versions":[{"id":87,"name":"1.21"}],"owner":{"display_name":"Carrie"}});
        let item = mcodex_detail("observed-id", &row);
        assert_eq!(item["name"], "Bartering Farm");
        assert_eq!(item["summary"], "Real description");
        assert_eq!(item["versions"], json!(["1.21"]));
        assert_eq!(item["pageUrl"], "https://mcodex.org/posts/observed-id");
        assert!(item.get("attachments").is_none());
        assert_eq!(item["downloadListAvailable"], true);
        assert_eq!(
            mcodex_cdn("scene.litematic").unwrap().host_str(),
            Some("cdn.mcodex.org")
        );
        assert!(mcodex_cdn("https://other.invalid/scene.litematic").is_err());
    }
}
