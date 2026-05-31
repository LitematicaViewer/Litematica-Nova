use super::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WikiBlockIconSlot {
    MaterialList,
    Layering,
}

impl WikiBlockIconSlot {
    fn output_size(self) -> (u32, u32) {
        match self {
            Self::Layering => (16, 16),
            Self::MaterialList => (32, 32),
        }
    }

    fn resize_filter(self) -> image::imageops::FilterType {
        match self {
            Self::Layering => image::imageops::FilterType::Nearest,
            Self::MaterialList => image::imageops::FilterType::Lanczos3,
        }
    }

    fn preserve_aspect_ratio(self) -> bool {
        matches!(self, Self::MaterialList)
    }
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WikiBlockIconSource {
    Wiki,
    WikiZh,
}

impl WikiBlockIconSource {
    fn tag(self) -> &'static str {
        match self {
            Self::Wiki => "wiki",
            Self::WikiZh => "wiki_zh",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Wiki => "Minecraft Wiki",
            Self::WikiZh => "中文 Minecraft 维基百科",
        }
    }

    fn layering_parse_api_url(self) -> &'static str {
        match self {
            Self::Wiki => {
                "https://minecraft.wiki/api.php?action=parse&format=json&page=Template:BlockSprite/doc&prop=text"
            }
            Self::WikiZh => {
                "https://zh.minecraft.wiki/api.php?action=parse&format=json&page=Template:BlockSprite/doc&prop=text"
            }
        }
    }

    fn layering_doc_page_url(self) -> &'static str {
        match self {
            Self::Wiki => "https://minecraft.wiki/w/Template:BlockSprite/doc",
            Self::WikiZh => "https://zh.minecraft.wiki/w/Template:BlockSprite/doc",
        }
    }

    fn layering_fallback_page_url(self) -> &'static str {
        match self {
            Self::Wiki => "https://minecraft.wiki/w/Template:BlockLink",
            Self::WikiZh => "https://zh.minecraft.wiki/w/Template:BlockLink",
        }
    }

    fn material_page_url(self) -> &'static str {
        match self {
            Self::Wiki => WIKI_ENUM_BLOCKS_URL,
            Self::WikiZh => "https://zh.minecraft.wiki/w/Java%E7%89%88%E6%95%B0%E6%8D%AE%E5%80%BC/%E6%96%B9%E5%9D%97ID",
        }
    }

    fn icon_header_candidates(self) -> &'static [&'static str] {
        match self {
            Self::Wiki => &["icon"],
            Self::WikiZh => &["图标", "图示", "图像", "icon"],
        }
    }

    fn resource_header_candidates(self) -> &'static [&'static str] {
        match self {
            Self::Wiki => &["resource location"],
            Self::WikiZh => &["命名空间id", "名称空间id", "命名空间 id", "名称空间 id"],
        }
    }

    fn root_relpath(self, slot: WikiBlockIconSlot) -> &'static str {
        match (self, slot) {
            (Self::Wiki, WikiBlockIconSlot::Layering) => "minecraft-assets/block_2d/wiki",
            (Self::Wiki, WikiBlockIconSlot::MaterialList) => "minecraft-assets/block_icon/wiki",
            (Self::WikiZh, WikiBlockIconSlot::Layering) => "minecraft-assets/block_2d/wiki_zh",
            (Self::WikiZh, WikiBlockIconSlot::MaterialList) => {
                "minecraft-assets/block_icon/wiki_zh"
            }
        }
    }
}

#[derive(Clone)]
struct WikiBlockRow {
    resource_location: String,
    image_url: String,
}

#[derive(Clone)]
struct WikiLayeringIconGroup {
    names: Vec<String>,
    image_url: String,
}

const WIKI_ICON_DOWNLOAD_CONCURRENCY: usize = 6;

enum WikiDownloadEvent {
    Started {
        display_name: String,
    },
    Finished {
        task_total: usize,
        available_count: usize,
    },
}

fn worker_count_for(total_tasks: usize) -> usize {
    total_tasks.min(WIKI_ICON_DOWNLOAD_CONCURRENCY).max(1)
}

fn emit_wiki_block_icon_progress(
    app: &AppHandle,
    current: usize,
    total: usize,
    downloaded: usize,
    status: impl Into<String>,
) {
    let _ = app.emit(
        "vault-block-icons-download-progress",
        VaultBlockIconProgress {
            current,
            total,
            downloaded,
            status: status.into(),
        },
    );
}

fn normalize_resource_location(value: &str) -> String {
    value
        .trim()
        .trim_matches('`')
        .trim()
        .strip_prefix("minecraft:")
        .unwrap_or(value.trim().trim_matches('`').trim())
        .to_string()
}

fn normalize_layering_icon_name(value: &str) -> Option<String> {
    let normalized = value
        .trim()
        .trim_matches('`')
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .replace(' ', "_");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_media_url(raw: &str, page_url: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Some(trimmed.to_string());
    }
    if trimmed.starts_with("//") {
        return Some(format!("https:{trimmed}"));
    }
    reqwest::Url::parse(page_url)
        .ok()?
        .join(trimmed)
        .ok()
        .map(|url| url.to_string())
}

fn normalize_srcset_url(raw: &str, page_url: &str) -> Option<String> {
    raw.split(',')
        .next()
        .and_then(|part| part.split_whitespace().next())
        .and_then(|url| normalize_media_url(url, page_url))
}

fn element_text_lower(cell: &ElementRef<'_>) -> String {
    element_text(cell).to_ascii_lowercase()
}

fn header_index_any(labels: &[String], candidates: &[&str]) -> Option<usize> {
    labels.iter().position(|label| {
        let normalized = label.trim();
        candidates
            .iter()
            .any(|candidate| normalized.eq_ignore_ascii_case(candidate))
    })
}

fn cell_first_code_value(cell: &ElementRef<'_>, code_selector: &Selector) -> Option<String> {
    extract_code_values_from_cell(cell, code_selector)
        .into_iter()
        .next()
        .map(|value| normalize_resource_location(&value))
        .filter(|value| !value.is_empty())
}

fn cell_first_image_url(
    cell: &ElementRef<'_>,
    img_selector: &Selector,
    page_url: &str,
) -> Option<String> {
    let image = cell.select(img_selector).next()?;
    image
        .value()
        .attr("src")
        .and_then(|raw| normalize_media_url(raw, page_url))
        .or_else(|| {
            image
                .value()
                .attr("data-src")
                .and_then(|raw| normalize_media_url(raw, page_url))
        })
        .or_else(|| {
            image
                .value()
                .attr("srcset")
                .and_then(|raw| normalize_srcset_url(raw, page_url))
        })
}

fn extract_wiki_block_rows(
    html: &str,
    source: WikiBlockIconSource,
) -> Result<Vec<WikiBlockRow>, String> {
    let table_selector = Selector::parse("table").map_err(|e| e.to_string())?;
    let row_selector = Selector::parse("tr").map_err(|e| e.to_string())?;
    let cell_selector = Selector::parse("th, td").map_err(|e| e.to_string())?;
    let code_selector = Selector::parse("code").map_err(|e| e.to_string())?;
    let img_selector = Selector::parse("img").map_err(|e| e.to_string())?;
    let document = Html::parse_document(html);
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    let page_url = source.material_page_url();

    for table in document.select(&table_selector) {
        let rows: Vec<_> = table.select(&row_selector).collect();
        let mut icon_column = None;
        let mut resource_column = None;
        let mut data_start = 0usize;

        for (row_index, row) in rows.iter().enumerate() {
            let cells: Vec<_> = row.select(&cell_selector).collect();
            let labels: Vec<_> = cells.iter().map(element_text_lower).collect();
            let Some(resource_idx) = header_index_any(&labels, source.resource_header_candidates())
            else {
                continue;
            };
            let Some(icon_idx) = header_index_any(&labels, source.icon_header_candidates()) else {
                continue;
            };
            icon_column = Some(icon_idx);
            resource_column = Some(resource_idx);
            data_start = row_index + 1;
            break;
        }

        let (Some(icon_column), Some(resource_column)) = (icon_column, resource_column) else {
            continue;
        };

        for row in rows.iter().skip(data_start) {
            let cells: Vec<_> = row.select(&cell_selector).collect();
            let (Some(icon_cell), Some(resource_cell)) =
                (cells.get(icon_column), cells.get(resource_column))
            else {
                continue;
            };
            let Some(resource_location) = cell_first_code_value(resource_cell, &code_selector)
            else {
                continue;
            };
            if resource_location.is_empty() || !seen.insert(resource_location.clone()) {
                continue;
            }
            let Some(image_url) = cell_first_image_url(icon_cell, &img_selector, page_url) else {
                continue;
            };
            output.push(WikiBlockRow {
                resource_location,
                image_url,
            });
        }

        if !output.is_empty() {
            return Ok(output);
        }
    }

    Err(format!(
        "无法从{}方块数据页解析 3D 图标列表。",
        source.label()
    ))
}

fn extract_wiki_layering_icon_groups(
    html: &str,
    source: WikiBlockIconSource,
) -> Result<Vec<WikiLayeringIconGroup>, String> {
    let box_selector = Selector::parse(".spritedoc-box").map_err(|e| e.to_string())?;
    let image_selector = Selector::parse(".spritedoc-image img").map_err(|e| e.to_string())?;
    let name_selector = Selector::parse(".spritedoc-names li").map_err(|e| e.to_string())?;
    let document = Html::parse_document(html);
    let mut output = Vec::new();
    let page_url = source.layering_doc_page_url();

    for box_node in document.select(&box_selector) {
        let Some(image_node) = box_node.select(&image_selector).next() else {
            continue;
        };
        let Some(image_url) = image_node
            .value()
            .attr("src")
            .and_then(|raw| normalize_media_url(raw, page_url))
            .or_else(|| {
                image_node
                    .value()
                    .attr("data-src")
                    .and_then(|raw| normalize_media_url(raw, page_url))
            })
            .or_else(|| {
                image_node
                    .value()
                    .attr("srcset")
                    .and_then(|raw| normalize_srcset_url(raw, page_url))
            })
        else {
            continue;
        };
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        for name_node in box_node.select(&name_selector) {
            let raw = element_text(&name_node);
            let Some(name) = normalize_layering_icon_name(&raw) else {
                continue;
            };
            if seen.insert(name.clone()) {
                names.push(name);
            }
        }
        if names.is_empty() {
            continue;
        }
        output.push(WikiLayeringIconGroup { names, image_url });
    }

    if output.is_empty() {
        return Err(format!("无法从{} 2D 图标页面解析列表。", source.label()));
    }

    Ok(output)
}

fn fetch_wiki_layering_html(
    client: &reqwest::blocking::Client,
    source: WikiBlockIconSource,
) -> Result<String, String> {
    let parse_url = source.layering_parse_api_url();
    let response = client
        .get(parse_url)
        .header(reqwest::header::ACCEPT, "application/json,text/plain,*/*")
        .send();
    if let Ok(response) = response {
        let status = response.status();
        let body = response.text().map_err(|e| e.to_string())?;
        if status.is_success() {
            let value: serde_json::Value =
                serde_json::from_str(&body).map_err(|e| format!("2D 图标目录响应解析失败: {e}"))?;
            if let Some(html) = value
                .get("parse")
                .and_then(|parse| parse.get("text"))
                .and_then(|text| text.get("*"))
                .and_then(|html| html.as_str())
            {
                return Ok(html.to_string());
            }
        }
    }

    for url in [
        source.layering_doc_page_url(),
        source.layering_fallback_page_url(),
    ] {
        if let Ok(html) = fetch_wiki_html(client, url) {
            return Ok(html);
        }
    }

    Err(format!("无法获取{} 2D 图标目录页面。", source.label()))
}

fn icon_file_path(target_dir: &Path, icon_name: &str) -> PathBuf {
    target_dir.join(format!("{icon_name}.png"))
}

fn icon_exists(target_dir: &Path, icon_name: &str) -> bool {
    icon_file_path(target_dir, icon_name).is_file()
}

fn all_icons_exist(target_dir: &Path, icon_names: &[String]) -> bool {
    !icon_names.is_empty() && icon_names.iter().all(|name| icon_exists(target_dir, name))
}

fn write_icon_bytes(target_dir: &Path, icon_name: &str, bytes: &[u8]) -> Result<(), String> {
    let path = icon_file_path(target_dir, icon_name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

fn fetch_and_resize_png(
    client: &reqwest::blocking::Client,
    image_url: &str,
    slot: WikiBlockIconSlot,
) -> Result<Vec<u8>, String> {
    let mut last_error = String::new();
    for _ in 0..3 {
        let response = client
            .get(image_url)
            .header(
                reqwest::header::ACCEPT,
                "image/avif,image/webp,image/png,image/*,*/*",
            )
            .send();
        let response = match response {
            Ok(value) => value,
            Err(error) => {
                last_error = format!(
                    "下载图标失败: {image_url} error sending request for url ({image_url}) {error}"
                );
                std::thread::sleep(Duration::from_millis(300));
                continue;
            }
        };
        let status = response.status();
        if !status.is_success() {
            last_error = format!("下载图标失败: {image_url} HTTP {status}");
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }
        let bytes = response.bytes().map_err(|e| e.to_string())?;
        let mut image = image::load_from_memory(&bytes)
            .map_err(|e| format!("图标解码失败: {image_url} {e}"))?;
        let target_size = slot.output_size();
        let current_size = image.dimensions();
        if slot.preserve_aspect_ratio() {
            let longest_edge = current_size.0.max(current_size.1);
            if longest_edge != target_size.0 {
                image = image.resize(target_size.0, target_size.1, slot.resize_filter());
            }
        } else if current_size != target_size {
            image = image.resize_exact(target_size.0, target_size.1, slot.resize_filter());
        }
        let mut output = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut output), image::ImageFormat::Png)
            .map_err(|e| format!("图标编码失败: {image_url} {e}"))?;
        return Ok(output);
    }
    Err(if last_error.is_empty() {
        format!("下载图标失败: {image_url}")
    } else {
        last_error
    })
}

fn download_layering_icons(
    app: &AppHandle,
    client: &reqwest::blocking::Client,
    groups: &[WikiLayeringIconGroup],
    target_dir: &Path,
    total: usize,
) -> Result<usize, String> {
    let task_count = groups.len();
    if task_count == 0 {
        return Ok(0);
    }
    let worker_count = worker_count_for(task_count);
    let mut buckets: Vec<Vec<WikiLayeringIconGroup>> =
        (0..worker_count).map(|_| Vec::new()).collect();
    for (index, group) in groups.iter().cloned().enumerate() {
        buckets[index % worker_count].push(group);
    }

    let target_dir = target_dir.to_path_buf();
    let (sender, receiver) = std::sync::mpsc::channel::<WikiDownloadEvent>();
    let mut handles = Vec::new();
    for bucket in buckets {
        let sender = sender.clone();
        let client = client.clone();
        let target_dir = target_dir.clone();
        handles.push(std::thread::spawn(move || {
            for group in bucket {
                let display_name = group
                    .names
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let _ = sender.send(WikiDownloadEvent::Started {
                    display_name: format!("{display_name}.png"),
                });

                let available_count = if all_icons_exist(&target_dir, &group.names) {
                    group.names.len()
                } else {
                    let bytes = fetch_and_resize_png(
                        &client,
                        &group.image_url,
                        WikiBlockIconSlot::Layering,
                    )
                    .ok();
                    for name in &group.names {
                        if let Some(bytes) = bytes.as_ref() {
                            let _ = write_icon_bytes(&target_dir, name, bytes);
                        }
                    }
                    group
                        .names
                        .iter()
                        .filter(|name| icon_exists(&target_dir, name))
                        .count()
                };

                let _ = sender.send(WikiDownloadEvent::Finished {
                    task_total: group.names.len(),
                    available_count,
                });
            }
        }));
    }
    drop(sender);

    let mut completed = 0usize;
    let mut available = 0usize;
    for event in receiver {
        match event {
            WikiDownloadEvent::Started { display_name } => {
                emit_wiki_block_icon_progress(
                    app,
                    completed.saturating_add(1),
                    total,
                    available,
                    format!("正在下载{display_name}"),
                );
            }
            WikiDownloadEvent::Finished {
                task_total,
                available_count,
            } => {
                completed += task_total;
                available += available_count;
            }
        }
    }
    for handle in handles {
        let _ = handle.join();
    }

    if available == 0 {
        return Err("未成功下载任何 2D 方块图标，且本地没有可复用缓存。".to_string());
    }
    Ok(available)
}

fn download_material_icons(
    app: &AppHandle,
    client: &reqwest::blocking::Client,
    rows: &[WikiBlockRow],
    target_dir: &Path,
    total: usize,
) -> Result<usize, String> {
    let task_count = rows.len();
    if task_count == 0 {
        return Ok(0);
    }
    let worker_count = worker_count_for(task_count);
    let mut buckets: Vec<Vec<WikiBlockRow>> = (0..worker_count).map(|_| Vec::new()).collect();
    for (index, row) in rows.iter().cloned().enumerate() {
        buckets[index % worker_count].push(row);
    }

    let target_dir = target_dir.to_path_buf();
    let (sender, receiver) = std::sync::mpsc::channel::<WikiDownloadEvent>();
    let mut handles = Vec::new();
    for bucket in buckets {
        let sender = sender.clone();
        let client = client.clone();
        let target_dir = target_dir.clone();
        handles.push(std::thread::spawn(move || {
            for row in bucket {
                let _ = sender.send(WikiDownloadEvent::Started {
                    display_name: format!("{}.png", row.resource_location),
                });

                let available_count = if icon_exists(&target_dir, &row.resource_location) {
                    1
                } else {
                    let bytes = fetch_and_resize_png(
                        &client,
                        &row.image_url,
                        WikiBlockIconSlot::MaterialList,
                    )
                    .ok();
                    if let Some(bytes) = bytes.as_ref() {
                        let _ = write_icon_bytes(&target_dir, &row.resource_location, bytes);
                    }
                    usize::from(icon_exists(&target_dir, &row.resource_location))
                };

                let _ = sender.send(WikiDownloadEvent::Finished {
                    task_total: 1,
                    available_count,
                });
            }
        }));
    }
    drop(sender);

    let mut completed = 0usize;
    let mut available = 0usize;
    for event in receiver {
        match event {
            WikiDownloadEvent::Started { display_name } => {
                emit_wiki_block_icon_progress(
                    app,
                    completed.saturating_add(1),
                    total,
                    available,
                    format!("正在下载{display_name}"),
                );
            }
            WikiDownloadEvent::Finished {
                task_total,
                available_count,
            } => {
                completed += task_total;
                available += available_count;
            }
        }
    }
    for handle in handles {
        let _ = handle.join();
    }

    if available == 0 {
        return Err("未成功下载任何 3D 方块图标，且本地没有可复用缓存。".to_string());
    }
    Ok(available)
}

fn wiki_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("Litematica-Nova-block-icons/1.0")
        .build()
        .map_err(|e| e.to_string())
}

fn download_minecraft_wiki_block_icons_sync(
    app: AppHandle,
    slot: WikiBlockIconSlot,
    source: WikiBlockIconSource,
) -> Result<VaultBlockIconDownloadOutput, String> {
    let client = wiki_client()?;
    let html = match slot {
        WikiBlockIconSlot::Layering => fetch_wiki_layering_html(&client, source)?,
        WikiBlockIconSlot::MaterialList => fetch_wiki_html(&client, source.material_page_url())?,
    };

    let target_dir = current_user_config_dir()?.join(source.root_relpath(slot));
    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;

    let (total, downloaded) = match slot {
        WikiBlockIconSlot::Layering => {
            let groups = extract_wiki_layering_icon_groups(&html, source)?;
            let total = groups.iter().map(|group| group.names.len()).sum::<usize>();
            let downloaded = download_layering_icons(&app, &client, &groups, &target_dir, total)?;
            (total, downloaded)
        }
        WikiBlockIconSlot::MaterialList => {
            let rows = extract_wiki_block_rows(&html, source)?;
            let total = rows.len();
            let downloaded = download_material_icons(&app, &client, &rows, &target_dir, total)?;
            (total, downloaded)
        }
    };

    emit_wiki_block_icon_progress(
        &app,
        downloaded,
        total,
        downloaded,
        format!("下载完成：{downloaded}/{total}"),
    );

    Ok(VaultBlockIconDownloadOutput {
        total,
        downloaded,
        target_dir: target_dir.display().to_string(),
        root_relpath: source.root_relpath(slot).to_string(),
    })
}

#[tauri::command]
pub async fn download_minecraft_wiki_block_icons(
    app: AppHandle,
    slot: WikiBlockIconSlot,
    source: WikiBlockIconSource,
) -> Result<VaultBlockIconDownloadOutput, String> {
    tauri::async_runtime::spawn_blocking(move || {
        download_minecraft_wiki_block_icons_sync(app, slot, source)
    })
    .await
    .map_err(|e| e.to_string())?
}
