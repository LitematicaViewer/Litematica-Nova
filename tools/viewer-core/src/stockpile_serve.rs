use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use zip::ZipArchive;

use crate::item_icons::StockpileIconPayload;
use crate::runtime_paths;
use crate::stockpile::StockpileMaterialsData;
use crate::stockpile_zip::StockpileZipPayload;

#[derive(Debug, Clone, Serialize)]
pub struct StockpileServeSummary {
    pub bind: String,
    pub zip: PathBuf,
    pub database: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParticipantState {
    pub user_id: String,
    pub first_seen_at: u64,
    pub last_seen_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClaimState {
    pub material_id: String,
    pub user_id: String,
    pub status: String,
    pub quantity: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialSyncState {
    pub material_id: String,
    pub required_count: u64,
    pub preparing_count: u64,
    pub done_count: u64,
    pub remaining_count: u64,
    pub overfilled_count: u64,
    pub participants: Vec<String>,
    pub overall_status: String,
    pub claims: Vec<ClaimState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StockpileSyncState {
    pub updated_at: u64,
    pub participants: Vec<ParticipantState>,
    pub materials: BTreeMap<String, MaterialSyncState>,
}

#[derive(Debug, Clone, Deserialize)]
struct ParticipantRequest {
    user_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ClaimRequest {
    status: String,
    quantity: u64,
}

pub fn serve_stockpile_zip(zip_path: &Path, bind: &str) -> Result<StockpileServeSummary> {
    let zip_path = absolutize(zip_path)?;
    let project = load_project_from_zip(&zip_path)?;
    let db_path = session_db_path(&zip_path)?;
    init_db(&db_path)?;
    let listener =
        TcpListener::bind(bind).with_context(|| format!("bind stockpile server failed: {bind}"))?;
    let summary = StockpileServeSummary {
        bind: bind.to_string(),
        zip: zip_path.clone(),
        database: db_path.clone(),
    };
    eprintln!(
        "stockpile server listening on http://{} using {}",
        bind,
        db_path.display()
    );
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = handle_connection(stream, &zip_path, &db_path, &project) {
                    eprintln!("stockpile serve request failed: {error:#}");
                }
            }
            Err(error) => eprintln!("stockpile serve accept failed: {error}"),
        }
    }
    Ok(summary)
}

fn handle_connection(
    mut stream: TcpStream,
    zip_path: &Path,
    db_path: &Path,
    project: &StockpileZipPayload,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let request = read_request(&mut stream)?;
    let response = route_request(&request, zip_path, db_path, project);
    write_response(&mut stream, response)
}

fn route_request(
    request: &HttpRequest,
    zip_path: &Path,
    db_path: &Path,
    project: &StockpileZipPayload,
) -> HttpResponse {
    match route_request_inner(request, zip_path, db_path, project) {
        Ok(response) => response,
        Err(error) => json_response(
            500,
            json!({ "error": error.to_string() }),
            "Internal Server Error",
        ),
    }
}

fn route_request_inner(
    request: &HttpRequest,
    zip_path: &Path,
    db_path: &Path,
    project: &StockpileZipPayload,
) -> Result<HttpResponse> {
    let path = request.path.split('?').next().unwrap_or("/");
    match (request.method.as_str(), path) {
        ("GET", "/api/project") => return Ok(json_response(200, project, "OK")),
        ("GET", "/api/state") => {
            let state = build_state(db_path, &project.materials)?;
            return Ok(json_response(200, state, "OK"));
        }
        ("POST", "/api/participants") => {
            let body: ParticipantRequest = serde_json::from_slice(&request.body)
                .context("parse participant request failed")?;
            upsert_participant(db_path, &body.user_id)?;
            let state = build_state(db_path, &project.materials)?;
            return Ok(json_response(200, state, "OK"));
        }
        _ => {}
    }

    if request.method == "PUT" || request.method == "DELETE" {
        if let Some((material_id, user_id)) = parse_claim_path(path)? {
            if request.method == "PUT" {
                let body: ClaimRequest =
                    serde_json::from_slice(&request.body).context("parse claim request failed")?;
                put_claim(db_path, &material_id, &user_id, &body.status, body.quantity)?;
            } else {
                delete_claim(db_path, &material_id, &user_id)?;
            }
            let state = build_state(db_path, &project.materials)?;
            return Ok(json_response(200, state, "OK"));
        }
    }

    if request.method == "GET" {
        return serve_zip_asset(zip_path, path);
    }

    Ok(json_response(
        404,
        json!({ "error": "not found" }),
        "Not Found",
    ))
}

fn parse_claim_path(path: &str) -> Result<Option<(String, String)>> {
    let Some(rest) = path.strip_prefix("/api/materials/") else {
        return Ok(None);
    };
    let Some((material_id, user_id)) = rest.split_once("/claims/") else {
        return Ok(None);
    };
    Ok(Some((
        percent_decode(material_id)?,
        percent_decode(user_id)?,
    )))
}

pub(crate) fn init_db(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("create stockpile session dir failed: {}", parent.display())
        })?;
    }
    let conn = Connection::open(path)
        .with_context(|| format!("open stockpile session db failed: {}", path.display()))?;
    init_db_conn(&conn)
}

fn init_db_conn(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        CREATE TABLE IF NOT EXISTS participants (
            user_id TEXT PRIMARY KEY NOT NULL,
            first_seen_at INTEGER NOT NULL,
            last_seen_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS material_claims (
            material_id TEXT NOT NULL,
            user_id TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('preparing', 'done')),
            quantity INTEGER NOT NULL CHECK(quantity >= 0),
            updated_at INTEGER NOT NULL,
            PRIMARY KEY(material_id, user_id)
        );
        "#,
    )
    .context("initialize stockpile session db failed")?;
    Ok(())
}

pub(crate) fn upsert_participant(path: &Path, user_id: &str) -> Result<()> {
    validate_user_id(user_id)?;
    let conn = Connection::open(path)?;
    init_db_conn(&conn)?;
    let now = current_unix_timestamp()?;
    conn.execute(
        r#"
        INSERT INTO participants(user_id, first_seen_at, last_seen_at)
        VALUES(?1, ?2, ?2)
        ON CONFLICT(user_id) DO UPDATE SET last_seen_at = excluded.last_seen_at
        "#,
        params![user_id, now],
    )?;
    Ok(())
}

pub(crate) fn put_claim(
    path: &Path,
    material_id: &str,
    user_id: &str,
    status: &str,
    quantity: u64,
) -> Result<()> {
    validate_material_id(material_id)?;
    validate_user_id(user_id)?;
    if !matches!(status, "preparing" | "done") {
        bail!("claim status must be preparing or done");
    }
    let conn = Connection::open(path)?;
    init_db_conn(&conn)?;
    let now = current_unix_timestamp()?;
    conn.execute(
        r#"
        INSERT INTO participants(user_id, first_seen_at, last_seen_at)
        VALUES(?1, ?2, ?2)
        ON CONFLICT(user_id) DO UPDATE SET last_seen_at = excluded.last_seen_at
        "#,
        params![user_id, now],
    )?;
    conn.execute(
        r#"
        INSERT INTO material_claims(material_id, user_id, status, quantity, updated_at)
        VALUES(?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(material_id, user_id)
        DO UPDATE SET status = excluded.status, quantity = excluded.quantity, updated_at = excluded.updated_at
        "#,
        params![material_id, user_id, status, quantity as i64, now],
    )?;
    Ok(())
}

pub(crate) fn delete_claim(path: &Path, material_id: &str, user_id: &str) -> Result<()> {
    validate_material_id(material_id)?;
    validate_user_id(user_id)?;
    let conn = Connection::open(path)?;
    init_db_conn(&conn)?;
    conn.execute(
        "DELETE FROM material_claims WHERE material_id = ?1 AND user_id = ?2",
        params![material_id, user_id],
    )?;
    Ok(())
}

pub(crate) fn build_state(
    path: &Path,
    materials: &StockpileMaterialsData,
) -> Result<StockpileSyncState> {
    let conn = Connection::open(path)?;
    init_db_conn(&conn)?;
    let participants = load_participants(&conn)?;
    let claims = load_claims(&conn)?;
    Ok(aggregate_state(
        materials,
        participants,
        claims,
        current_unix_timestamp()?,
    ))
}

fn aggregate_state(
    materials: &StockpileMaterialsData,
    participants: Vec<ParticipantState>,
    claims: Vec<ClaimState>,
    updated_at: u64,
) -> StockpileSyncState {
    let mut claims_by_material = BTreeMap::<String, Vec<ClaimState>>::new();
    for claim in claims {
        claims_by_material
            .entry(claim.material_id.clone())
            .or_default()
            .push(claim);
    }
    let mut material_states = BTreeMap::<String, MaterialSyncState>::new();
    for material in &materials.materials {
        let material_claims = claims_by_material
            .remove(&material.namespace_id)
            .unwrap_or_default();
        let preparing_count = material_claims
            .iter()
            .filter(|claim| claim.status == "preparing")
            .map(|claim| claim.quantity)
            .sum::<u64>();
        let done_count = material_claims
            .iter()
            .filter(|claim| claim.status == "done")
            .map(|claim| claim.quantity)
            .sum::<u64>();
        let total_claimed = preparing_count.saturating_add(done_count);
        let remaining_count = material.required_count.saturating_sub(total_claimed);
        let overfilled_count = total_claimed.saturating_sub(material.required_count);
        let participants_for_material = material_claims
            .iter()
            .map(|claim| claim.user_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let overall_status = if total_claimed == 0 {
            "not_started"
        } else if overfilled_count > 0 {
            "overfilled"
        } else if done_count >= material.required_count {
            "done"
        } else if done_count > 0 {
            "partial_done"
        } else {
            "preparing"
        }
        .to_string();
        material_states.insert(
            material.namespace_id.clone(),
            MaterialSyncState {
                material_id: material.namespace_id.clone(),
                required_count: material.required_count,
                preparing_count,
                done_count,
                remaining_count,
                overfilled_count,
                participants: participants_for_material,
                overall_status,
                claims: material_claims,
            },
        );
    }
    StockpileSyncState {
        updated_at,
        participants,
        materials: material_states,
    }
}

fn load_participants(conn: &Connection) -> Result<Vec<ParticipantState>> {
    let mut stmt = conn.prepare(
        "SELECT user_id, first_seen_at, last_seen_at FROM participants ORDER BY last_seen_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ParticipantState {
            user_id: row.get(0)?,
            first_seen_at: i64_to_u64(row.get(1)?),
            last_seen_at: i64_to_u64(row.get(2)?),
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("load participants failed")
}

fn load_claims(conn: &Connection) -> Result<Vec<ClaimState>> {
    let mut stmt = conn.prepare(
        "SELECT material_id, user_id, status, quantity, updated_at FROM material_claims ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ClaimState {
            material_id: row.get(0)?,
            user_id: row.get(1)?,
            status: row.get(2)?,
            quantity: i64_to_u64(row.get(3)?),
            updated_at: i64_to_u64(row.get(4)?),
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("load claims failed")
}

pub(crate) fn load_project_from_zip(zip_path: &Path) -> Result<StockpileZipPayload> {
    let mut zip = ZipArchive::new(
        File::open(zip_path)
            .with_context(|| format!("open stockpile zip failed: {}", zip_path.display()))?,
    )
    .context("open stockpile zip archive failed")?;
    Ok(StockpileZipPayload {
        manifest: read_json_entry(&mut zip, "data/manifest.json")?,
        materials: read_json_entry(&mut zip, "data/materials.json")?,
        recipe_status: read_json_entry(&mut zip, "data/recipe_status.json")?,
        recipe_trees: read_json_entry(&mut zip, "data/recipe_trees.json")?,
        icons: read_json_entry(&mut zip, "data/icons.json").unwrap_or_else(|_| {
            StockpileIconPayload {
                manifest: None,
                by_key: BTreeMap::new(),
            }
        }),
        i18n: read_json_entry(&mut zip, "data/i18n.json").unwrap_or_else(|_| json!({})),
        icon_files: Vec::new(),
    })
}

fn read_json_entry<T: for<'de> Deserialize<'de>>(
    zip: &mut ZipArchive<File>,
    name: &str,
) -> Result<T> {
    let mut entry = zip
        .by_name(name)
        .with_context(|| format!("zip missing {name}"))?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .with_context(|| format!("read zip entry failed: {name}"))?;
    serde_json::from_str(&text).with_context(|| format!("parse zip JSON failed: {name}"))
}

fn serve_zip_asset(zip_path: &Path, path: &str) -> Result<HttpResponse> {
    let name = if path == "/" || path == "/index.html" {
        "index.html".to_string()
    } else {
        path.trim_start_matches('/').replace('\\', "/")
    };
    if name.contains("..") || name.starts_with('/') {
        return Ok(json_response(
            400,
            json!({ "error": "invalid path" }),
            "Bad Request",
        ));
    }
    let mut zip = ZipArchive::new(File::open(zip_path)?)?;
    let Ok(mut entry) = zip.by_name(&name) else {
        return Ok(json_response(
            404,
            json!({ "error": "not found" }),
            "Not Found",
        ));
    };
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    Ok(HttpResponse {
        status: 200,
        reason: "OK".to_string(),
        content_type: content_type(&name).to_string(),
        body: bytes,
    })
}

fn session_db_path(zip_path: &Path) -> Result<PathBuf> {
    let stem = zip_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("stockpile");
    Ok(runtime_paths::stockpile_sessions_root()?
        .join(format!("{}.sqlite", safe_session_name(stem))))
}

fn safe_session_name(value: &str) -> String {
    let safe = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if safe.is_empty() {
        "stockpile".to_string()
    } else {
        safe
    }
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(runtime_paths::app_root()?.join(path))
}

fn validate_user_id(value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("user_id is required");
    }
    if value.len() > 128 {
        bail!("user_id is too long");
    }
    Ok(())
}

fn validate_material_id(value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("material_id is required");
    }
    Ok(())
}

fn i64_to_u64(value: i64) -> u64 {
    value.max(0) as u64
}

fn current_unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time is before unix epoch")?
        .as_secs())
}

struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

struct HttpResponse {
    status: u16,
    reason: String,
    content_type: String,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    if request_line.trim().is_empty() {
        bail!("empty HTTP request");
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut content_length = 0_usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed
            .strip_prefix("Content-Length:")
            .or_else(|| trimmed.strip_prefix("content-length:"))
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0_u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }
    Ok(HttpRequest { method, path, body })
}

fn write_response(stream: &mut TcpStream, response: HttpResponse) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
        response.status,
        response.reason,
        response.content_type,
        response.body.len()
    )?;
    stream.write_all(&response.body)?;
    stream.flush()?;
    Ok(())
}

fn json_response<T: Serialize>(status: u16, value: T, reason: &str) -> HttpResponse {
    HttpResponse {
        status,
        reason: reason.to_string(),
        content_type: "application/json; charset=utf-8".to_string(),
        body: serde_json::to_vec(&value).unwrap_or_else(|_| b"{\"error\":\"serialize\"}".to_vec()),
    }
}

fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

fn percent_decode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3])?;
                output.push(
                    u8::from_str_radix(hex, 16).map_err(|_| anyhow!("bad percent encoding"))?,
                );
                index += 3;
            }
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            value => {
                output.push(value);
                index += 1;
            }
        }
    }
    String::from_utf8(output).context("decoded path is not UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stockpile::{
        StockpileMaterialItem, StockpileMaterialsData, StockpileProjectInfo, StockpileSummary,
    };
    use rusqlite::OptionalExtension;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn sqlite_initializes_participants_and_claims() {
        let db = temp_db("init");
        init_db(&db).expect("init db");
        let conn = Connection::open(&db).expect("open db");
        let participants: Option<String> = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='participants'",
                [],
                |row| row.get(0),
            )
            .optional()
            .expect("participants table");
        assert_eq!(participants.as_deref(), Some("participants"));
        let _ = std::fs::remove_file(db);
    }

    #[test]
    fn participant_upsert_updates_last_seen() {
        let db = temp_db("participant");
        init_db(&db).expect("init db");
        upsert_participant(&db, "alex").expect("insert");
        upsert_participant(&db, "alex").expect("update");
        let state = build_state(&db, &fixture_materials()).expect("state");
        assert_eq!(state.participants.len(), 1);
        assert_eq!(state.participants[0].user_id, "alex");
        let _ = std::fs::remove_file(db);
    }

    #[test]
    fn claim_put_delete_and_multi_user_same_material() {
        let db = temp_db("claims");
        init_db(&db).expect("init db");
        put_claim(&db, "minecraft:stone", "alex", "preparing", 3).expect("claim alex");
        put_claim(&db, "minecraft:stone", "sam", "done", 4).expect("claim sam");
        let state = build_state(&db, &fixture_materials()).expect("state");
        let stone = state.materials.get("minecraft:stone").expect("stone");
        assert_eq!(stone.preparing_count, 3);
        assert_eq!(stone.done_count, 4);
        assert_eq!(stone.participants.len(), 2);
        delete_claim(&db, "minecraft:stone", "alex").expect("delete");
        let state = build_state(&db, &fixture_materials()).expect("state");
        let stone = state.materials.get("minecraft:stone").expect("stone");
        assert_eq!(stone.preparing_count, 0);
        assert_eq!(stone.done_count, 4);
        let _ = std::fs::remove_file(db);
    }

    #[test]
    fn aggregate_statuses_cover_core_cases() {
        let materials = fixture_materials();
        let claims = vec![
            claim("minecraft:stone", "a", "preparing", 2),
            claim("minecraft:glass", "b", "done", 2),
            claim("minecraft:rail", "c", "done", 10),
            claim("minecraft:chest", "d", "done", 8),
        ];
        let state = aggregate_state(&materials, Vec::new(), claims, 1);
        assert_eq!(
            state.materials["minecraft:stone"].overall_status,
            "preparing"
        );
        assert_eq!(
            state.materials["minecraft:glass"].overall_status,
            "partial_done"
        );
        assert_eq!(state.materials["minecraft:rail"].overall_status, "done");
        assert_eq!(
            state.materials["minecraft:chest"].overall_status,
            "overfilled"
        );
        assert_eq!(
            state.materials["minecraft:empty"].overall_status,
            "not_started"
        );
    }

    #[test]
    fn claim_rejects_invalid_status() {
        let db = temp_db("bad_status");
        init_db(&db).expect("init db");
        assert!(put_claim(&db, "minecraft:stone", "alex", "paused", 1).is_err());
        let _ = std::fs::remove_file(db);
    }

    #[test]
    fn serve_loads_project_from_stockpile_zip() {
        let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("stats_water_fixture.litematic");
        let output = crate::runtime_paths::stockpile_exports_root()
            .expect("exports root")
            .join(format!(
                "stockpile_serve_project_{}.zip",
                std::process::id()
            ));
        crate::stockpile_zip::export_stockpile_zip(&input, Some(&output), false, Some("1.21.10"))
            .expect("export zip");
        let project = load_project_from_zip(&output).expect("load project");
        assert!(!project.materials.materials.is_empty());
        assert!(project.i18n.get("zh-CN").is_some());
        assert!(project.icons.by_key.contains_key("__fallback"));
        let _ = std::fs::remove_file(output);
    }

    fn fixture_materials() -> StockpileMaterialsData {
        StockpileMaterialsData {
            project: StockpileProjectInfo {
                source_file: "fixture.litematic".to_string(),
                created_at: 1,
                data_version: 3953,
                regions: vec!["main".to_string()],
            },
            summary: StockpileSummary {
                total_blocks: 29,
                unique_materials: 5,
                total_stacks: 5,
                estimated_shulker_boxes: 1,
            },
            materials: vec![
                material("minecraft:stone", 10),
                material("minecraft:glass", 5),
                material("minecraft:rail", 10),
                material("minecraft:chest", 4),
                material("minecraft:empty", 1),
            ],
        }
    }

    fn material(id: &str, required_count: u64) -> StockpileMaterialItem {
        StockpileMaterialItem {
            id: id.split(':').next_back().unwrap().to_string(),
            namespace_id: id.to_string(),
            display_name: id.to_string(),
            required_count,
            stack_size: 64,
            stacks: 0,
            remainder: required_count,
            shulker_boxes: 1,
            category: "Other".to_string(),
            category_icon: "minecraft:barrier".to_string(),
            item_icon_key: id.to_string(),
            icon_path: String::new(),
            icon_available: false,
            source_regions: vec!["main".to_string()],
            recipe_status: "missing".to_string(),
            craft_complexity: 0,
        }
    }

    fn claim(material_id: &str, user_id: &str, status: &str, quantity: u64) -> ClaimState {
        ClaimState {
            material_id: material_id.to_string(),
            user_id: user_id.to_string(),
            status: status.to_string(),
            quantity,
            updated_at: 1,
        }
    }

    fn temp_db(name: &str) -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "lba_stockpile_serve_{name}_{}_{}.sqlite",
            std::process::id(),
            id
        ))
    }
}
