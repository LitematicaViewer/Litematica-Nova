use super::*;
use std::collections::HashMap;

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WikiBlockIconSlot {
    MaterialList,
    Layering,
}

impl WikiBlockIconSlot {
    fn root_relpath(self) -> &'static str {
        match self {
            Self::Layering => "minecraft-assets/block_2d/wiki",
            Self::MaterialList => "minecraft-assets/block_icon/wiki",
        }
    }

    fn output_size(self) -> (u32, u32) {
        match self {
            Self::Layering => (16, 16),
            Self::MaterialList => (32, 32),
        }
    }

    fn resize_filter(self) -> image::imageops::FilterType {
        image::imageops::FilterType::Nearest
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

fn header_index(labels: &[String], keyword: &str) -> Option<usize> {
    labels
        .iter()
        .position(|label| label.trim().eq_ignore_ascii_case(keyword))
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

fn normalize_media_url(raw: &str) -> Option<String> {
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
    reqwest::Url::parse(WIKI_ENUM_BLOCKS_URL)
        .ok()?
        .join(trimmed)
        .ok()
        .map(|url| url.to_string())
}

fn normalize_srcset_url(raw: &str) -> Option<String> {
    raw.split(',')
        .next()
        .and_then(|part| part.split_whitespace().next())
        .and_then(normalize_media_url)
}

fn cell_first_code_value(cell: &ElementRef<'_>, code_selector: &Selector) -> Option<String> {
    extract_code_values_from_cell(cell, code_selector)
        .into_iter()
        .next()
        .map(|value| normalize_resource_location(&value))
        .filter(|value| !value.is_empty())
}

fn cell_first_image_url(cell: &ElementRef<'_>, img_selector: &Selector) -> Option<String> {
    let image = cell.select(img_selector).next()?;
    image
        .value()
        .attr("src")
        .and_then(normalize_media_url)
        .or_else(|| image.value().attr("data-src").and_then(normalize_media_url))
        .or_else(|| image.value().attr("srcset").and_then(normalize_srcset_url))
}

fn extract_wiki_block_rows(html: &str) -> Result<Vec<WikiBlockRow>, String> {
    let table_selector = Selector::parse("table").map_err(|e| e.to_string())?;
    let row_selector = Selector::parse("tr").map_err(|e| e.to_string())?;
    let cell_selector = Selector::parse("th, td").map_err(|e| e.to_string())?;
    let code_selector = Selector::parse("code").map_err(|e| e.to_string())?;
    let img_selector = Selector::parse("img").map_err(|e| e.to_string())?;
    let document = Html::parse_document(html);
    let mut output = Vec::new();
    let mut seen = HashSet::new();

    for table in document.select(&table_selector) {
        let rows: Vec<_> = table.select(&row_selector).collect();
        let mut icon_column = None;
        let mut resource_column = None;
        let mut data_start = 0usize;

        for (row_index, row) in rows.iter().enumerate() {
            let cells: Vec<_> = row.select(&cell_selector).collect();
            let labels: Vec<_> = cells
                .iter()
                .map(|cell| element_text(cell).to_ascii_lowercase())
                .collect();
            let Some(resource_idx) = header_index(&labels, "resource location") else {
                continue;
            };
            let Some(icon_idx) = header_index(&labels, "icon") else {
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
            let Some(image_url) = cell_first_image_url(icon_cell, &img_selector) else {
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

    Err("无法从 Minecraft Wiki 方块数据页解析图标表。".to_string())
}

fn extract_wiki_layering_icon_groups(html: &str) -> Result<Vec<WikiLayeringIconGroup>, String> {
    let box_selector = Selector::parse(".spritedoc-box").map_err(|e| e.to_string())?;
    let image_selector = Selector::parse(".spritedoc-image img").map_err(|e| e.to_string())?;
    let name_selector = Selector::parse(".spritedoc-names li").map_err(|e| e.to_string())?;
    let document = Html::parse_document(html);
    let mut output = Vec::new();

    for box_node in document.select(&box_selector) {
        let Some(image_node) = box_node.select(&image_selector).next() else {
            continue;
        };
        let Some(image_url) = image_node
            .value()
            .attr("src")
            .and_then(normalize_media_url)
            .or_else(|| {
                image_node
                    .value()
                    .attr("data-src")
                    .and_then(normalize_media_url)
            })
            .or_else(|| {
                image_node
                    .value()
                    .attr("srcset")
                    .and_then(normalize_srcset_url)
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
        return Err("无法从 Template:BlockLink 页面解析 2D 图标列表。".to_string());
    }

    Ok(output)
}

fn fetch_wiki_layering_html(client: &reqwest::blocking::Client) -> Result<String, String> {
    let parse_url = "https://minecraft.wiki/api.php?action=parse&format=json&page=Template:BlockSprite/doc&prop=text";
    let response = client
        .get(parse_url)
        .header(reqwest::header::ACCEPT, "application/json,text/plain,*/*")
        .send()
        .map_err(|e| format!("请求 2D 图标目录失败: {e}"))?;
    let status = response.status();
    let body = response.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("请求 2D 图标目录失败: HTTP {status}"));
    }
    let value: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("2D 图标目录响应解析失败: {e}"))?;
    value
        .get("parse")
        .and_then(|parse| parse.get("text"))
        .and_then(|text| text.get("*"))
        .and_then(|html| html.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| "2D 图标目录缺少可解析 HTML。".to_string())
}

fn fetch_and_resize_png(
    client: &reqwest::blocking::Client,
    image_url: &str,
    slot: WikiBlockIconSlot,
) -> Result<Vec<u8>, String> {
    let response = client
        .get(image_url)
        .header(
            reqwest::header::ACCEPT,
            "image/avif,image/webp,image/png,image/*,*/*",
        )
        .send()
        .map_err(|e| format!("下载图标失败: {image_url} {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("下载图标失败: {image_url} HTTP {status}"));
    }
    let bytes = response.bytes().map_err(|e| e.to_string())?;
    let mut image =
        image::load_from_memory(&bytes).map_err(|e| format!("图标解码失败: {image_url} {e}"))?;
    let target_size = slot.output_size();
    if image.dimensions() != target_size {
        image = image.resize_exact(target_size.0, target_size.1, slot.resize_filter());
    }
    let mut output = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut output), image::ImageFormat::Png)
        .map_err(|e| format!("图标编码失败: {image_url} {e}"))?;
    Ok(output)
}

fn write_icon_bytes(
    target_dir: &Path,
    resource_location: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let path = target_dir.join(format!("{resource_location}.png"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

fn download_layering_icons(
    app: &AppHandle,
    client: &reqwest::blocking::Client,
    groups: &[WikiLayeringIconGroup],
    target_dir: &Path,
    total: usize,
) -> Result<usize, String> {
    let mut cache = HashMap::<String, Vec<u8>>::new();
    let mut downloaded = 0usize;
    for group in groups {
        let bytes = if let Some(cached) = cache.get(&group.image_url) {
            cached.clone()
        } else {
            let display_name = group
                .names
                .first()
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());
            emit_wiki_block_icon_progress(
                app,
                downloaded.saturating_add(1),
                total,
                downloaded,
                format!("正在下载{display_name}.png"),
            );
            let downloaded_bytes =
                fetch_and_resize_png(client, &group.image_url, WikiBlockIconSlot::Layering)?;
            cache.insert(group.image_url.clone(), downloaded_bytes.clone());
            downloaded_bytes
        };
        for name in &group.names {
            emit_wiki_block_icon_progress(
                app,
                downloaded.saturating_add(1),
                total,
                downloaded,
                format!("正在下载{name}.png"),
            );
            write_icon_bytes(target_dir, name, &bytes)?;
            downloaded += 1;
        }
    }
    Ok(downloaded)
}

fn download_material_icons(
    app: &AppHandle,
    client: &reqwest::blocking::Client,
    rows: &[WikiBlockRow],
    target_dir: &Path,
    total: usize,
) -> Result<usize, String> {
    let mut cache = HashMap::<String, Vec<u8>>::new();
    let mut downloaded = 0usize;
    for row in rows {
        emit_wiki_block_icon_progress(
            app,
            downloaded.saturating_add(1),
            total,
            downloaded,
            format!("正在下载{}.png", row.resource_location),
        );
        let bytes = if let Some(cached) = cache.get(&row.image_url) {
            cached.clone()
        } else {
            let downloaded_bytes =
                fetch_and_resize_png(client, &row.image_url, WikiBlockIconSlot::MaterialList)?;
            cache.insert(row.image_url.clone(), downloaded_bytes.clone());
            downloaded_bytes
        };
        write_icon_bytes(target_dir, &row.resource_location, &bytes)?;
        downloaded += 1;
    }
    Ok(downloaded)
}

fn download_minecraft_wiki_block_icons_sync(
    app: AppHandle,
    slot: WikiBlockIconSlot,
) -> Result<VaultBlockIconDownloadOutput, String> {
    let client = wiki_client()?;
    let html = match slot {
        WikiBlockIconSlot::Layering => fetch_wiki_layering_html(&client)?,
        WikiBlockIconSlot::MaterialList => fetch_wiki_html(&client, WIKI_ENUM_BLOCKS_URL)?,
    };

    let target_dir = current_user_config_dir()?.join(slot.root_relpath());
    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;
    clear_directory_contents(&target_dir)?;

    let (total, downloaded) = match slot {
        WikiBlockIconSlot::Layering => {
            let groups = extract_wiki_layering_icon_groups(&html)?;
            let total = groups.iter().map(|group| group.names.len()).sum::<usize>();
            let downloaded = download_layering_icons(&app, &client, &groups, &target_dir, total)?;
            (total, downloaded)
        }
        WikiBlockIconSlot::MaterialList => {
            let rows = extract_wiki_block_rows(&html)?;
            let total = rows.len();
            let downloaded = download_material_icons(&app, &client, &rows, &target_dir, total)?;
            (total, downloaded)
        }
    };
    if total == 0 || downloaded == 0 {
        return Err("未成功下载任何 Minecraft Wiki 方块图标。".to_string());
    }
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
        root_relpath: slot.root_relpath().to_string(),
    })
}

#[tauri::command]
pub async fn download_minecraft_wiki_block_icons(
    app: AppHandle,
    slot: WikiBlockIconSlot,
) -> Result<VaultBlockIconDownloadOutput, String> {
    tauri::async_runtime::spawn_blocking(move || {
        download_minecraft_wiki_block_icons_sync(app, slot)
    })
    .await
    .map_err(|e| e.to_string())?
}
