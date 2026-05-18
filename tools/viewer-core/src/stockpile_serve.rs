use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::item_icons::StockpileIconPayload;
use crate::item_names::StockpileItemNamesPayload;
use crate::runtime_paths;
use crate::stockpile::StockpileMaterialsData;
use crate::stockpile_schema::{
    STOCKPILE_I18N_SCHEMA_VERSION, STOCKPILE_ICONS_SCHEMA_VERSION,
    STOCKPILE_ITEM_NAMES_SCHEMA_VERSION, STOCKPILE_MATERIALS_SCHEMA_VERSION,
    STOCKPILE_RECIPE_TREES_SCHEMA_VERSION, STOCKPILE_SQLITE_SCHEMA_VERSION,
    STOCKPILE_ZIP_SCHEMA_VERSION,
};
use crate::stockpile_zip::StockpileZipPayload;

#[derive(Debug, Clone, Serialize)]
pub struct StockpileServeSummary {
    pub bind: String,
    pub zip: PathBuf,
    pub database: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionMeta {
    pub schema_version: u32,
    pub zip_hash: String,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfoOutput {
    pub zip_path: PathBuf,
    pub session_db: PathBuf,
    pub schema_version: u32,
    pub zip_hash: String,
    pub participants_count: u64,
    pub claims_count: u64,
    pub materials_count: u64,
    pub done_count: u64,
    pub preparing_count: u64,
    pub remaining_count: u64,
    pub overfilled_count: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionExportData {
    pub schema_version: u32,
    pub zip_path: String,
    pub zip_hash: String,
    pub participants: Vec<ParticipantState>,
    pub material_claims: Vec<ClaimState>,
    pub summary: SessionInfoSummary,
    pub exported_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfoSummary {
    pub participants_count: u64,
    pub claims_count: u64,
    pub materials_count: u64,
    pub done_count: u64,
    pub preparing_count: u64,
    pub remaining_count: u64,
    pub overfilled_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionResetOutput {
    pub zip_path: PathBuf,
    pub session_db: PathBuf,
    pub reset: bool,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionImportOutput {
    pub zip_path: PathBuf,
    pub session_db: PathBuf,
    pub imported_participants: usize,
    pub imported_claims: usize,
    pub replace: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StockpileConfig {
    pub mode: String,
    pub admin_page_enabled: bool,
    pub access_password_enabled: bool,
    pub admin_password_enabled: bool,
    pub whitelist_enabled: bool,
    pub allow_guest_readonly: bool,
    pub default_language: String,
    pub poll_interval_ms: u32,
    pub show_advanced_recipe_tree: bool,
    pub show_unresolved_recipes: bool,
    pub show_icon_fallback_badge: bool,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigShowOutput {
    pub zip_path: PathBuf,
    pub session_db: PathBuf,
    pub schema_version: u32,
    pub config: StockpileConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigResetOutput {
    pub zip_path: PathBuf,
    pub session_db: PathBuf,
    pub reset: bool,
    pub config: StockpileConfig,
    pub warning: Option<String>,
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

fn default_config(now: u64) -> StockpileConfig {
    StockpileConfig {
        mode: "multi".to_string(),
        admin_page_enabled: false,
        access_password_enabled: false,
        admin_password_enabled: false,
        whitelist_enabled: false,
        allow_guest_readonly: false,
        default_language: "auto".to_string(),
        poll_interval_ms: 3000,
        show_advanced_recipe_tree: true,
        show_unresolved_recipes: true,
        show_icon_fallback_badge: false,
        updated_at: now,
    }
}

pub fn serve_stockpile_zip(zip_path: &Path, bind: &str) -> Result<StockpileServeSummary> {
    let zip_path = absolutize(zip_path)?;
    let project = load_project_from_zip(&zip_path)?;
    let db_path = session_db_path(&zip_path)?;
    ensure_session_db(&db_path, &zip_hash(&zip_path)?)?;
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
        ("GET", "/api/config") => {
            let config = load_config(db_path)?;
            return Ok(json_response(200, config, "OK"));
        }
        ("PUT", "/api/config") => {
            let body: Value =
                serde_json::from_slice(&request.body).context("parse config request failed")?;
            let config = update_config_from_value(db_path, &body)?;
            return Ok(json_response(200, config, "OK"));
        }
        ("POST", "/api/participants") => {
            let body: ParticipantRequest = serde_json::from_slice(&request.body)
                .context("parse participant request failed")?;
            upsert_participant(db_path, &body.user_id)?;
            eprintln!("stockpile participant upserted: {}", body.user_id);
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
        CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS stockpile_config (
            id INTEGER PRIMARY KEY CHECK(id = 1),
            mode TEXT NOT NULL CHECK(mode IN ('single', 'multi')),
            admin_page_enabled INTEGER NOT NULL CHECK(admin_page_enabled IN (0, 1)),
            access_password_enabled INTEGER NOT NULL CHECK(access_password_enabled IN (0, 1)),
            admin_password_enabled INTEGER NOT NULL CHECK(admin_password_enabled IN (0, 1)),
            whitelist_enabled INTEGER NOT NULL CHECK(whitelist_enabled IN (0, 1)),
            allow_guest_readonly INTEGER NOT NULL CHECK(allow_guest_readonly IN (0, 1)),
            default_language TEXT NOT NULL CHECK(default_language IN ('auto', 'zh-CN', 'en-US')),
            poll_interval_ms INTEGER NOT NULL CHECK(poll_interval_ms >= 2000 AND poll_interval_ms <= 10000),
            show_advanced_recipe_tree INTEGER NOT NULL CHECK(show_advanced_recipe_tree IN (0, 1)),
            show_unresolved_recipes INTEGER NOT NULL CHECK(show_unresolved_recipes IN (0, 1)),
            show_icon_fallback_badge INTEGER NOT NULL CHECK(show_icon_fallback_badge IN (0, 1)),
            updated_at INTEGER NOT NULL
        );
        "#,
    )
    .context("initialize stockpile session db failed")?;
    let now = current_unix_timestamp()?;
    if meta_value(conn, "schema_version")?.is_none() {
        set_meta(
            conn,
            "schema_version",
            &STOCKPILE_SQLITE_SCHEMA_VERSION.to_string(),
        )?;
        set_meta(conn, "zip_hash", "")?;
        set_meta(conn, "created_at", &now.to_string())?;
        set_meta(conn, "updated_at", &now.to_string())?;
    }
    insert_default_config_if_missing(conn, now)?;
    Ok(())
}

fn ensure_session_db(path: &Path, expected_zip_hash: &str) -> Result<()> {
    init_db(path)?;
    let conn = Connection::open(path)?;
    let schema_version = meta_value(&conn, "schema_version")?
        .unwrap_or_default()
        .parse::<u32>()
        .unwrap_or(0);
    if schema_version < STOCKPILE_SQLITE_SCHEMA_VERSION {
        migrate_sqlite_schema(&conn, schema_version)?;
    } else if schema_version != STOCKPILE_SQLITE_SCHEMA_VERSION {
        bail!(
            "unsupported stockpile sqlite schema_version {}; supported {}; export/reset/import session state",
            schema_version,
            STOCKPILE_SQLITE_SCHEMA_VERSION
        );
    }
    let existing_hash = meta_value(&conn, "zip_hash")?.unwrap_or_default();
    if existing_hash.is_empty() {
        set_meta(&conn, "zip_hash", expected_zip_hash)?;
    } else if existing_hash != expected_zip_hash {
        bail!(
            "session database zip_hash does not match stockpile zip; use session-export/reset/import"
        );
    }
    touch_meta(&conn)?;
    Ok(())
}

fn migrate_sqlite_schema(conn: &Connection, schema_version: u32) -> Result<()> {
    match schema_version {
        2 => {
            insert_default_config_if_missing(conn, current_unix_timestamp()?)?;
            set_meta(
                conn,
                "schema_version",
                &STOCKPILE_SQLITE_SCHEMA_VERSION.to_string(),
            )?;
            touch_meta(conn)?;
            Ok(())
        }
        _ => bail!(
            "unsupported stockpile sqlite schema_version {}; supported {}; export/reset/import session state",
            schema_version,
            STOCKPILE_SQLITE_SCHEMA_VERSION
        ),
    }
}

fn insert_default_config_if_missing(conn: &Connection, now: u64) -> Result<()> {
    let config = default_config(now);
    conn.execute(
        r#"
        INSERT OR IGNORE INTO stockpile_config(
            id,
            mode,
            admin_page_enabled,
            access_password_enabled,
            admin_password_enabled,
            whitelist_enabled,
            allow_guest_readonly,
            default_language,
            poll_interval_ms,
            show_advanced_recipe_tree,
            show_unresolved_recipes,
            show_icon_fallback_badge,
            updated_at
        )
        VALUES(1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        "#,
        params![
            config.mode,
            bool_to_i64(config.admin_page_enabled),
            bool_to_i64(config.access_password_enabled),
            bool_to_i64(config.admin_password_enabled),
            bool_to_i64(config.whitelist_enabled),
            bool_to_i64(config.allow_guest_readonly),
            config.default_language,
            config.poll_interval_ms as i64,
            bool_to_i64(config.show_advanced_recipe_tree),
            bool_to_i64(config.show_unresolved_recipes),
            bool_to_i64(config.show_icon_fallback_badge),
            config.updated_at as i64,
        ],
    )
    .context("initialize stockpile config failed")?;
    Ok(())
}

fn meta_value(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
    let mut rows = stmt.query(params![key])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn touch_meta(conn: &Connection) -> Result<()> {
    set_meta(conn, "updated_at", &current_unix_timestamp()?.to_string())
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
    touch_meta(&conn)?;
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
    touch_meta(&conn)?;
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

pub fn session_info(zip_path: &Path) -> Result<SessionInfoOutput> {
    let zip_path = absolutize(zip_path)?;
    let project = load_project_from_zip(&zip_path)?;
    let db_path = session_db_path(&zip_path)?;
    let zip_hash = zip_hash(&zip_path)?;
    ensure_session_db(&db_path, &zip_hash)?;
    let state = build_state(&db_path, &project.materials)?;
    let summary = summarize_state(&state);
    Ok(SessionInfoOutput {
        zip_path,
        session_db: db_path,
        schema_version: STOCKPILE_SQLITE_SCHEMA_VERSION,
        zip_hash,
        participants_count: summary.participants_count,
        claims_count: summary.claims_count,
        materials_count: summary.materials_count,
        done_count: summary.done_count,
        preparing_count: summary.preparing_count,
        remaining_count: summary.remaining_count,
        overfilled_count: summary.overfilled_count,
        updated_at: state.updated_at,
    })
}

pub fn session_reset(zip_path: &Path, yes: bool) -> Result<SessionResetOutput> {
    let zip_path = absolutize(zip_path)?;
    let db_path = session_db_path(&zip_path)?;
    init_db(&db_path)?;
    if !yes {
        return Ok(SessionResetOutput {
            zip_path,
            session_db: db_path,
            reset: false,
            warning: Some("session-reset requires --yes to clear material_claims".to_string()),
        });
    }
    let conn = Connection::open(&db_path)?;
    let schema_version = meta_value(&conn, "schema_version")?
        .unwrap_or_default()
        .parse::<u32>()
        .unwrap_or(0);
    if schema_version < STOCKPILE_SQLITE_SCHEMA_VERSION {
        migrate_sqlite_schema(&conn, schema_version)?;
    } else if schema_version != STOCKPILE_SQLITE_SCHEMA_VERSION {
        bail!(
            "unsupported stockpile sqlite schema_version {}; supported {}; export/reset/import session state",
            schema_version,
            STOCKPILE_SQLITE_SCHEMA_VERSION
        );
    }
    conn.execute("DELETE FROM material_claims", [])?;
    set_meta(&conn, "zip_hash", &zip_hash(&zip_path)?)?;
    touch_meta(&conn)?;
    Ok(SessionResetOutput {
        zip_path,
        session_db: db_path,
        reset: true,
        warning: None,
    })
}

pub fn config_show(zip_path: &Path) -> Result<ConfigShowOutput> {
    let (zip_path, db_path) = ensure_zip_session(zip_path)?;
    let config = load_config(&db_path)?;
    Ok(ConfigShowOutput {
        zip_path,
        session_db: db_path,
        schema_version: STOCKPILE_SQLITE_SCHEMA_VERSION,
        config,
    })
}

pub fn config_set(zip_path: &Path, key: &str, value: &str) -> Result<ConfigShowOutput> {
    let (zip_path, db_path) = ensure_zip_session(zip_path)?;
    let mut changes = BTreeMap::new();
    changes.insert(key.to_string(), Value::String(value.to_string()));
    let config = update_config_map(&db_path, &changes)?;
    Ok(ConfigShowOutput {
        zip_path,
        session_db: db_path,
        schema_version: STOCKPILE_SQLITE_SCHEMA_VERSION,
        config,
    })
}

pub fn config_reset(zip_path: &Path, yes: bool) -> Result<ConfigResetOutput> {
    let (zip_path, db_path) = ensure_zip_session(zip_path)?;
    if !yes {
        return Ok(ConfigResetOutput {
            zip_path,
            session_db: db_path.clone(),
            reset: false,
            config: load_config(&db_path)?,
            warning: Some("config-reset requires --yes to restore default config".to_string()),
        });
    }
    let conn = Connection::open(&db_path)?;
    let config = default_config(current_unix_timestamp()?);
    save_config_conn(&conn, &config)?;
    touch_meta(&conn)?;
    Ok(ConfigResetOutput {
        zip_path,
        session_db: db_path,
        reset: true,
        config,
        warning: None,
    })
}

fn ensure_zip_session(zip_path: &Path) -> Result<(PathBuf, PathBuf)> {
    let zip_path = absolutize(zip_path)?;
    load_project_from_zip(&zip_path)?;
    let db_path = session_db_path(&zip_path)?;
    ensure_session_db(&db_path, &zip_hash(&zip_path)?)?;
    Ok((zip_path, db_path))
}

fn load_config(path: &Path) -> Result<StockpileConfig> {
    init_db(path)?;
    let conn = Connection::open(path)?;
    load_config_conn(&conn)
}

fn load_config_conn(conn: &Connection) -> Result<StockpileConfig> {
    conn.query_row(
        r#"
        SELECT mode,
               admin_page_enabled,
               access_password_enabled,
               admin_password_enabled,
               whitelist_enabled,
               allow_guest_readonly,
               default_language,
               poll_interval_ms,
               show_advanced_recipe_tree,
               show_unresolved_recipes,
               show_icon_fallback_badge,
               updated_at
        FROM stockpile_config
        WHERE id = 1
        "#,
        [],
        |row| {
            Ok(StockpileConfig {
                mode: row.get(0)?,
                admin_page_enabled: i64_to_bool(row.get(1)?),
                access_password_enabled: i64_to_bool(row.get(2)?),
                admin_password_enabled: i64_to_bool(row.get(3)?),
                whitelist_enabled: i64_to_bool(row.get(4)?),
                allow_guest_readonly: i64_to_bool(row.get(5)?),
                default_language: row.get(6)?,
                poll_interval_ms: i64_to_u64(row.get(7)?) as u32,
                show_advanced_recipe_tree: i64_to_bool(row.get(8)?),
                show_unresolved_recipes: i64_to_bool(row.get(9)?),
                show_icon_fallback_badge: i64_to_bool(row.get(10)?),
                updated_at: i64_to_u64(row.get(11)?),
            })
        },
    )
    .context("load stockpile config failed")
}

fn update_config_from_value(path: &Path, value: &Value) -> Result<StockpileConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("config update body must be a JSON object"))?;
    let changes = object
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    update_config_map(path, &changes)
}

fn update_config_map(path: &Path, changes: &BTreeMap<String, Value>) -> Result<StockpileConfig> {
    init_db(path)?;
    let conn = Connection::open(path)?;
    let mut config = load_config_conn(&conn)?;
    for (key, value) in changes {
        apply_config_value(&mut config, key, value)?;
    }
    config.updated_at = current_unix_timestamp()?;
    save_config_conn(&conn, &config)?;
    touch_meta(&conn)?;
    Ok(config)
}

fn apply_config_value(config: &mut StockpileConfig, key: &str, value: &Value) -> Result<()> {
    match key {
        "mode" => {
            let value = string_value(key, value)?;
            if !matches!(value, "single" | "multi") {
                bail!("mode must be single or multi");
            }
            config.mode = value.to_string();
        }
        "admin_page_enabled" => config.admin_page_enabled = bool_value(key, value)?,
        "access_password_enabled" => config.access_password_enabled = bool_value(key, value)?,
        "admin_password_enabled" => config.admin_password_enabled = bool_value(key, value)?,
        "whitelist_enabled" => config.whitelist_enabled = bool_value(key, value)?,
        "allow_guest_readonly" => config.allow_guest_readonly = bool_value(key, value)?,
        "default_language" => {
            let value = string_value(key, value)?;
            if !matches!(value, "auto" | "zh-CN" | "en-US") {
                bail!("default_language must be auto, zh-CN, or en-US");
            }
            config.default_language = value.to_string();
        }
        "poll_interval_ms" => {
            let value = u32_value(key, value)?;
            if !(2000..=10000).contains(&value) {
                bail!("poll_interval_ms must be between 2000 and 10000");
            }
            config.poll_interval_ms = value;
        }
        "show_advanced_recipe_tree" => config.show_advanced_recipe_tree = bool_value(key, value)?,
        "show_unresolved_recipes" => config.show_unresolved_recipes = bool_value(key, value)?,
        "show_icon_fallback_badge" => config.show_icon_fallback_badge = bool_value(key, value)?,
        other => bail!("unsupported stockpile config key: {other}"),
    }
    Ok(())
}

fn save_config_conn(conn: &Connection, config: &StockpileConfig) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO stockpile_config(
            id,
            mode,
            admin_page_enabled,
            access_password_enabled,
            admin_password_enabled,
            whitelist_enabled,
            allow_guest_readonly,
            default_language,
            poll_interval_ms,
            show_advanced_recipe_tree,
            show_unresolved_recipes,
            show_icon_fallback_badge,
            updated_at
        )
        VALUES(1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ON CONFLICT(id) DO UPDATE SET
            mode = excluded.mode,
            admin_page_enabled = excluded.admin_page_enabled,
            access_password_enabled = excluded.access_password_enabled,
            admin_password_enabled = excluded.admin_password_enabled,
            whitelist_enabled = excluded.whitelist_enabled,
            allow_guest_readonly = excluded.allow_guest_readonly,
            default_language = excluded.default_language,
            poll_interval_ms = excluded.poll_interval_ms,
            show_advanced_recipe_tree = excluded.show_advanced_recipe_tree,
            show_unresolved_recipes = excluded.show_unresolved_recipes,
            show_icon_fallback_badge = excluded.show_icon_fallback_badge,
            updated_at = excluded.updated_at
        "#,
        params![
            config.mode,
            bool_to_i64(config.admin_page_enabled),
            bool_to_i64(config.access_password_enabled),
            bool_to_i64(config.admin_password_enabled),
            bool_to_i64(config.whitelist_enabled),
            bool_to_i64(config.allow_guest_readonly),
            config.default_language,
            config.poll_interval_ms as i64,
            bool_to_i64(config.show_advanced_recipe_tree),
            bool_to_i64(config.show_unresolved_recipes),
            bool_to_i64(config.show_icon_fallback_badge),
            config.updated_at as i64,
        ],
    )
    .context("save stockpile config failed")?;
    Ok(())
}

fn string_value<'a>(key: &str, value: &'a Value) -> Result<&'a str> {
    value
        .as_str()
        .ok_or_else(|| anyhow!("{key} must be a string"))
}

fn bool_value(key: &str, value: &Value) -> Result<bool> {
    if let Some(value) = value.as_bool() {
        return Ok(value);
    }
    match value.as_str() {
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        _ => bail!("{key} must be true or false"),
    }
}

fn u32_value(key: &str, value: &Value) -> Result<u32> {
    if let Some(value) = value.as_u64() {
        return u32::try_from(value).with_context(|| format!("{key} is too large"));
    }
    let value = string_value(key, value)?;
    value
        .parse::<u32>()
        .with_context(|| format!("{key} must be an integer"))
}

pub fn session_export(zip_path: &Path, output: &Path) -> Result<SessionExportData> {
    let zip_path = absolutize(zip_path)?;
    let project = load_project_from_zip(&zip_path)?;
    let db_path = session_db_path(&zip_path)?;
    let zip_hash = zip_hash(&zip_path)?;
    ensure_session_db(&db_path, &zip_hash)?;
    let conn = Connection::open(&db_path)?;
    let participants = load_participants(&conn)?;
    let claims = load_claims(&conn)?;
    let state = aggregate_state(
        &project.materials,
        participants.clone(),
        claims.clone(),
        current_unix_timestamp()?,
    );
    let data = SessionExportData {
        schema_version: crate::stockpile_schema::STOCKPILE_SESSION_EXPORT_SCHEMA_VERSION,
        zip_path: zip_path.display().to_string(),
        zip_hash,
        participants,
        material_claims: claims,
        summary: summarize_state(&state),
        exported_at: current_unix_timestamp()?,
    };
    let output = absolutize(output)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create session export dir failed: {}", parent.display()))?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&data)?)
        .with_context(|| format!("write session export failed: {}", output.display()))?;
    Ok(data)
}

pub fn session_import(zip_path: &Path, input: &Path, replace: bool) -> Result<SessionImportOutput> {
    let zip_path = absolutize(zip_path)?;
    let input = absolutize(input)?;
    let project = load_project_from_zip(&zip_path)?;
    let db_path = session_db_path(&zip_path)?;
    let expected_hash = zip_hash(&zip_path)?;
    ensure_session_db(&db_path, &expected_hash)?;
    let data: SessionExportData = serde_json::from_slice(
        &fs::read(&input)
            .with_context(|| format!("read session import failed: {}", input.display()))?,
    )
    .with_context(|| format!("parse session import failed: {}", input.display()))?;
    if data.schema_version != crate::stockpile_schema::STOCKPILE_SESSION_EXPORT_SCHEMA_VERSION {
        bail!(
            "unsupported session export schema_version {}",
            data.schema_version
        );
    }
    if data.zip_hash != expected_hash {
        bail!("session export zip_hash does not match target stockpile zip");
    }
    let valid_materials = project
        .materials
        .materials
        .iter()
        .map(|material| material.namespace_id.as_str())
        .collect::<BTreeSet<_>>();
    let conn = Connection::open(&db_path)?;
    if replace {
        conn.execute("DELETE FROM material_claims", [])?;
    }
    for participant in &data.participants {
        upsert_participant(&db_path, &participant.user_id)?;
    }
    let mut imported_claims = 0_usize;
    for claim in &data.material_claims {
        if valid_materials.contains(claim.material_id.as_str()) {
            put_claim(
                &db_path,
                &claim.material_id,
                &claim.user_id,
                &claim.status,
                claim.quantity,
            )?;
            imported_claims += 1;
        }
    }
    touch_meta(&conn)?;
    Ok(SessionImportOutput {
        zip_path,
        session_db: db_path,
        imported_participants: data.participants.len(),
        imported_claims,
        replace,
    })
}

fn summarize_state(state: &StockpileSyncState) -> SessionInfoSummary {
    let claims_count = state
        .materials
        .values()
        .map(|material| material.claims.len() as u64)
        .sum();
    SessionInfoSummary {
        participants_count: state.participants.len() as u64,
        claims_count,
        materials_count: state.materials.len() as u64,
        done_count: state
            .materials
            .values()
            .map(|material| material.done_count)
            .sum(),
        preparing_count: state
            .materials
            .values()
            .map(|material| material.preparing_count)
            .sum(),
        remaining_count: state
            .materials
            .values()
            .map(|material| material.remaining_count)
            .sum(),
        overfilled_count: state
            .materials
            .values()
            .map(|material| material.overfilled_count)
            .sum(),
    }
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
    let payload = StockpileZipPayload {
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
        item_names: read_json_entry(&mut zip, "data/item_names.json").unwrap_or_else(|_| {
            StockpileItemNamesPayload {
                schema_version: STOCKPILE_ITEM_NAMES_SCHEMA_VERSION,
                status: "missing".to_string(),
                manifest: None,
                names: BTreeMap::new(),
                warning: Some("stockpile zip has no item_names payload".to_string()),
            }
        }),
        i18n: read_json_entry(&mut zip, "data/i18n.json").unwrap_or_else(|_| json!({})),
        icon_files: Vec::new(),
    };
    validate_project_schema(&payload)?;
    Ok(payload)
}

fn validate_project_schema(project: &StockpileZipPayload) -> Result<()> {
    let manifest = &project.manifest;
    if manifest.schema_version != STOCKPILE_ZIP_SCHEMA_VERSION {
        bail!(
            "unsupported stockpile zip schema_version {}; supported {}",
            manifest.schema_version,
            STOCKPILE_ZIP_SCHEMA_VERSION
        );
    }
    if manifest.materials_schema_version != STOCKPILE_MATERIALS_SCHEMA_VERSION
        || project.materials.schema_version != STOCKPILE_MATERIALS_SCHEMA_VERSION
    {
        bail!("unsupported stockpile materials schema version");
    }
    if manifest.recipe_trees_schema_version != STOCKPILE_RECIPE_TREES_SCHEMA_VERSION {
        bail!("unsupported stockpile recipe_trees schema version");
    }
    if manifest.icons_schema_version != STOCKPILE_ICONS_SCHEMA_VERSION {
        bail!("unsupported stockpile icons schema version");
    }
    if manifest.i18n_schema_version != STOCKPILE_I18N_SCHEMA_VERSION {
        bail!("unsupported stockpile i18n schema version");
    }
    if manifest.item_names_schema_version != STOCKPILE_ITEM_NAMES_SCHEMA_VERSION
        || project.item_names.schema_version != STOCKPILE_ITEM_NAMES_SCHEMA_VERSION
    {
        bail!("unsupported stockpile item_names schema version");
    }
    Ok(())
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

pub(crate) fn session_db_path(zip_path: &Path) -> Result<PathBuf> {
    let stem = zip_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("stockpile");
    Ok(runtime_paths::stockpile_sessions_root()?
        .join(format!("{}.sqlite", safe_session_name(stem))))
}

pub(crate) fn zip_hash(zip_path: &Path) -> Result<String> {
    let bytes =
        fs::read(zip_path).with_context(|| format!("read zip failed: {}", zip_path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(to_hex(&hasher.finalize()))
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

fn i64_to_bool(value: i64) -> bool {
    value != 0
}

fn bool_to_i64(value: bool) -> i64 {
    i64::from(value)
}

fn current_unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time is before unix epoch")?
        .as_secs())
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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
        let config = load_config(&db).expect("default config");
        assert_eq!(config.mode, "multi");
        assert_eq!(config.default_language, "auto");
        assert_eq!(config.poll_interval_ms, 3000);
        assert!(config.show_advanced_recipe_tree);
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

    #[test]
    fn session_info_reset_export_import_roundtrip() {
        let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("stats_water_fixture.litematic");
        let output = crate::runtime_paths::stockpile_exports_root()
            .expect("exports root")
            .join(format!(
                "stockpile_session_roundtrip_{}.zip",
                std::process::id()
            ));
        crate::stockpile_zip::export_stockpile_zip(&input, Some(&output), false, Some("1.21.10"))
            .expect("export zip");
        let db = session_db_path(&output).expect("session db");
        let _ = std::fs::remove_file(&db);
        put_claim(&db, "minecraft:stone", "alex", "preparing", 2).expect("claim");
        let info = session_info(&output).expect("info");
        assert_eq!(info.claims_count, 1);
        assert_eq!(info.zip_hash, zip_hash(&output).expect("zip hash"));
        let state_path = crate::runtime_paths::stockpile_sessions_root()
            .expect("sessions")
            .join(format!(
                "stockpile_session_roundtrip_{}.json",
                std::process::id()
            ));
        let exported = session_export(&output, &state_path).expect("export session");
        assert_eq!(exported.material_claims.len(), 1);
        let dry = session_reset(&output, false).expect("dry reset");
        assert!(!dry.reset);
        let reset = session_reset(&output, true).expect("reset");
        assert!(reset.reset);
        assert_eq!(session_info(&output).expect("post reset").claims_count, 0);
        let imported = session_import(&output, &state_path, true).expect("import");
        assert_eq!(imported.imported_claims, 1);
        assert_eq!(session_info(&output).expect("post import").claims_count, 1);
        let _ = std::fs::remove_file(output);
        let _ = std::fs::remove_file(db);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn config_show_returns_default_config() {
        let fixture = exported_fixture("config_show");
        let db = session_db_path(&fixture).expect("session db");
        let _ = std::fs::remove_file(&db);

        let output = config_show(&fixture).expect("config show");
        assert_eq!(output.config.mode, "multi");
        assert_eq!(output.config.default_language, "auto");
        assert_eq!(output.config.poll_interval_ms, 3000);

        cleanup_fixture(&fixture, &db, Path::new(""));
    }

    #[test]
    fn config_set_accepts_bool_and_enum_values() {
        let fixture = exported_fixture("config_set");
        let db = session_db_path(&fixture).expect("session db");
        let _ = std::fs::remove_file(&db);

        let output = config_set(&fixture, "admin_page_enabled", "true").expect("set bool");
        assert!(output.config.admin_page_enabled);
        let output = config_set(&fixture, "mode", "single").expect("set mode");
        assert_eq!(output.config.mode, "single");
        let output = config_set(&fixture, "default_language", "zh-CN").expect("set language");
        assert_eq!(output.config.default_language, "zh-CN");

        cleanup_fixture(&fixture, &db, Path::new(""));
    }

    #[test]
    fn config_set_validates_poll_interval_range() {
        let fixture = exported_fixture("config_poll");
        let db = session_db_path(&fixture).expect("session db");
        let _ = std::fs::remove_file(&db);

        let output = config_set(&fixture, "poll_interval_ms", "5000").expect("set interval");
        assert_eq!(output.config.poll_interval_ms, 5000);
        assert!(config_set(&fixture, "poll_interval_ms", "1000").is_err());
        assert!(config_set(&fixture, "poll_interval_ms", "12000").is_err());

        cleanup_fixture(&fixture, &db, Path::new(""));
    }

    #[test]
    fn config_set_rejects_illegal_key_and_values() {
        let fixture = exported_fixture("config_illegal");
        let db = session_db_path(&fixture).expect("session db");
        let _ = std::fs::remove_file(&db);

        assert!(config_set(&fixture, "unknown", "true").is_err());
        assert!(config_set(&fixture, "mode", "coop").is_err());
        assert!(config_set(&fixture, "default_language", "fr-FR").is_err());
        assert!(config_set(&fixture, "whitelist_enabled", "yes").is_err());

        cleanup_fixture(&fixture, &db, Path::new(""));
    }

    #[test]
    fn config_reset_requires_confirmation_and_restores_defaults() {
        let fixture = exported_fixture("config_reset");
        let db = session_db_path(&fixture).expect("session db");
        let _ = std::fs::remove_file(&db);

        config_set(&fixture, "mode", "single").expect("set mode");
        let dry = config_reset(&fixture, false).expect("dry reset");
        assert!(!dry.reset);
        assert_eq!(dry.config.mode, "single");
        let reset = config_reset(&fixture, true).expect("reset");
        assert!(reset.reset);
        assert_eq!(reset.config.mode, "multi");
        assert_eq!(reset.config.poll_interval_ms, 3000);

        cleanup_fixture(&fixture, &db, Path::new(""));
    }

    #[test]
    fn old_sqlite_schema_auto_adds_config_table() {
        let fixture = exported_fixture("config_migrate");
        let db = session_db_path(&fixture).expect("session db");
        let _ = std::fs::remove_file(&db);
        let conn = Connection::open(&db).expect("open db");
        conn.execute_batch(
            r#"
            CREATE TABLE participants (
                user_id TEXT PRIMARY KEY NOT NULL,
                first_seen_at INTEGER NOT NULL,
                last_seen_at INTEGER NOT NULL
            );
            CREATE TABLE material_claims (
                material_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                status TEXT NOT NULL CHECK(status IN ('preparing', 'done')),
                quantity INTEGER NOT NULL CHECK(quantity >= 0),
                updated_at INTEGER NOT NULL,
                PRIMARY KEY(material_id, user_id)
            );
            CREATE TABLE meta (
                key TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
            );
            INSERT INTO meta(key, value) VALUES('schema_version', '2');
            INSERT INTO meta(key, value) VALUES('zip_hash', '');
            INSERT INTO meta(key, value) VALUES('created_at', '1');
            INSERT INTO meta(key, value) VALUES('updated_at', '1');
            "#,
        )
        .expect("seed v2 db");

        ensure_session_db(&db, &zip_hash(&fixture).expect("zip hash")).expect("migrate db");
        let migrated = load_config(&db).expect("load migrated config");
        assert_eq!(migrated.mode, "multi");
        assert_eq!(
            meta_value(
                &Connection::open(&db).expect("open migrated"),
                "schema_version"
            )
            .expect("schema"),
            Some(STOCKPILE_SQLITE_SCHEMA_VERSION.to_string())
        );

        cleanup_fixture(&fixture, &db, Path::new(""));
    }

    #[test]
    fn session_export_import_does_not_overwrite_config() {
        let fixture = exported_fixture("config_export_import");
        let db = session_db_path(&fixture).expect("session db");
        let state_path = temp_session_export("config_export_import");
        let _ = std::fs::remove_file(&db);

        config_set(&fixture, "mode", "single").expect("set mode");
        put_claim(&db, "minecraft:stone", "alex", "preparing", 2).expect("claim");
        session_export(&fixture, &state_path).expect("export");
        config_set(&fixture, "poll_interval_ms", "5000").expect("set interval");
        session_import(&fixture, &state_path, true).expect("import");
        let config = config_show(&fixture).expect("config").config;
        assert_eq!(config.mode, "single");
        assert_eq!(config.poll_interval_ms, 5000);

        cleanup_fixture(&fixture, &db, &state_path);
    }

    #[test]
    fn config_api_get_and_put_update_config() {
        let fixture = exported_fixture("config_api");
        let db = session_db_path(&fixture).expect("session db");
        let _ = std::fs::remove_file(&db);
        ensure_session_db(&db, &zip_hash(&fixture).expect("zip hash")).expect("session");
        let project = load_project_from_zip(&fixture).expect("project");

        let get = route_request_inner(&request("GET", "/api/config", b""), &fixture, &db, &project)
            .expect("get config");
        assert_eq!(get.status, 200);
        let config: StockpileConfig = serde_json::from_slice(&get.body).expect("config json");
        assert_eq!(config.mode, "multi");

        let put = route_request_inner(
            &request(
                "PUT",
                "/api/config",
                br#"{"mode":"single","poll_interval_ms":5000,"show_icon_fallback_badge":true}"#,
            ),
            &fixture,
            &db,
            &project,
        )
        .expect("put config");
        assert_eq!(put.status, 200);
        let config: StockpileConfig = serde_json::from_slice(&put.body).expect("updated config");
        assert_eq!(config.mode, "single");
        assert_eq!(config.poll_interval_ms, 5000);
        assert!(config.show_icon_fallback_badge);

        cleanup_fixture(&fixture, &db, Path::new(""));
    }

    #[test]
    fn config_api_invalid_put_returns_error() {
        let fixture = exported_fixture("config_api_bad");
        let db = session_db_path(&fixture).expect("session db");
        let _ = std::fs::remove_file(&db);
        ensure_session_db(&db, &zip_hash(&fixture).expect("zip hash")).expect("session");
        let project = load_project_from_zip(&fixture).expect("project");

        let response = route_request(
            &request("PUT", "/api/config", br#"{"poll_interval_ms":1000}"#),
            &fixture,
            &db,
            &project,
        );
        assert_ne!(response.status, 200);

        cleanup_fixture(&fixture, &db, Path::new(""));
    }

    #[test]
    fn load_project_rejects_bad_zip() {
        let path = crate::runtime_paths::stockpile_exports_root()
            .expect("exports root")
            .join(format!("stockpile_bad_zip_{}.zip", std::process::id()));
        std::fs::write(&path, b"not a zip").expect("bad zip");
        assert!(load_project_from_zip(&path).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn session_import_merges_by_default() {
        let fixture = exported_fixture("merge");
        let db = session_db_path(&fixture).expect("session db");
        let state_path = temp_session_export("merge");
        let _ = std::fs::remove_file(&db);
        put_claim(&db, "minecraft:stone", "alex", "preparing", 2).expect("seed claim");
        session_export(&fixture, &state_path).expect("export session");
        put_claim(&db, "minecraft:glass", "sam", "done", 5).expect("extra claim");

        let imported = session_import(&fixture, &state_path, false).expect("merge import");
        assert_eq!(imported.imported_claims, 1);
        let info = session_info(&fixture).expect("info");
        assert_eq!(info.claims_count, 2);

        cleanup_fixture(&fixture, &db, &state_path);
    }

    #[test]
    fn session_import_replace_clears_existing_claims() {
        let fixture = exported_fixture("replace");
        let db = session_db_path(&fixture).expect("session db");
        let state_path = temp_session_export("replace");
        let _ = std::fs::remove_file(&db);
        put_claim(&db, "minecraft:stone", "alex", "preparing", 2).expect("seed claim");
        session_export(&fixture, &state_path).expect("export session");
        put_claim(&db, "minecraft:glass", "sam", "done", 5).expect("extra claim");

        let imported = session_import(&fixture, &state_path, true).expect("replace import");
        assert_eq!(imported.imported_claims, 1);
        let state = build_state(
            &db,
            &load_project_from_zip(&fixture).expect("project").materials,
        )
        .expect("state");
        assert!(state.materials["minecraft:glass"].claims.is_empty());
        assert_eq!(session_info(&fixture).expect("info").claims_count, 1);

        cleanup_fixture(&fixture, &db, &state_path);
    }

    #[test]
    fn session_import_rejects_zip_hash_mismatch() {
        let fixture = exported_fixture("hash_mismatch");
        let db = session_db_path(&fixture).expect("session db");
        let state_path = temp_session_export("hash_mismatch");
        let _ = std::fs::remove_file(&db);
        put_claim(&db, "minecraft:stone", "alex", "preparing", 2).expect("seed claim");
        let mut exported = session_export(&fixture, &state_path).expect("export session");
        exported.zip_hash = "not-the-right-zip".to_string();
        fs::write(
            &state_path,
            serde_json::to_vec_pretty(&exported).expect("serialize export"),
        )
        .expect("rewrite export");

        assert!(session_import(&fixture, &state_path, false).is_err());

        cleanup_fixture(&fixture, &db, &state_path);
    }

    #[test]
    fn session_reset_yes_rebinds_changed_zip_hash() {
        let fixture = exported_fixture("reset_rebind");
        let db = session_db_path(&fixture).expect("session db");
        let _ = std::fs::remove_file(&db);
        ensure_session_db(&db, "old-hash").expect("seed old hash");
        put_claim(&db, "minecraft:stone", "alex", "preparing", 2).expect("claim");

        let reset = session_reset(&fixture, true).expect("reset changed zip");
        assert!(reset.reset);
        let conn = Connection::open(&db).expect("open db");
        assert_eq!(
            meta_value(&conn, "zip_hash").expect("zip hash"),
            Some(zip_hash(&fixture).expect("fixture hash"))
        );
        assert_eq!(session_info(&fixture).expect("info").claims_count, 0);

        cleanup_fixture(&fixture, &db, Path::new(""));
    }

    #[test]
    fn load_project_rejects_unsupported_zip_schema() {
        let fixture = exported_fixture("unsupported_schema");
        let mut project = load_project_from_zip(&fixture).expect("load project");
        project.manifest.schema_version = 999;

        assert!(validate_project_schema(&project).is_err());

        let db = session_db_path(&fixture).expect("session db");
        cleanup_fixture(&fixture, &db, Path::new(""));
    }

    #[test]
    fn ensure_session_db_rejects_schema_mismatch() {
        let fixture = exported_fixture("sqlite_schema");
        let db = session_db_path(&fixture).expect("session db");
        let _ = std::fs::remove_file(&db);
        init_db(&db).expect("init db");
        let conn = Connection::open(&db).expect("open db");
        set_meta(&conn, "schema_version", "999").expect("set schema");

        assert!(ensure_session_db(&db, &zip_hash(&fixture).expect("zip hash")).is_err());

        cleanup_fixture(&fixture, &db, Path::new(""));
    }

    fn fixture_materials() -> StockpileMaterialsData {
        StockpileMaterialsData {
            schema_version: crate::stockpile_schema::STOCKPILE_MATERIALS_SCHEMA_VERSION,
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
            display_names: BTreeMap::new(),
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

    fn exported_fixture(name: &str) -> PathBuf {
        let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("stats_water_fixture.litematic");
        let output = temp_zip_path(name);
        crate::stockpile_zip::export_stockpile_zip(&input, Some(&output), false, Some("1.21.10"))
            .expect("export zip");
        output
    }

    fn temp_zip_path(name: &str) -> PathBuf {
        crate::runtime_paths::stockpile_exports_root()
            .expect("exports root")
            .join(format!("stockpile_serve_{name}_{}.zip", std::process::id()))
    }

    fn temp_session_export(name: &str) -> PathBuf {
        crate::runtime_paths::stockpile_sessions_root()
            .expect("sessions root")
            .join(format!(
                "stockpile_serve_{name}_{}.json",
                std::process::id()
            ))
    }

    fn cleanup_fixture(zip: &Path, db: &Path, extra: &Path) {
        let _ = std::fs::remove_file(zip);
        let _ = std::fs::remove_file(db);
        if !extra.as_os_str().is_empty() {
            let _ = std::fs::remove_file(extra);
        }
    }

    fn request(method: &str, path: &str, body: &[u8]) -> HttpRequest {
        HttpRequest {
            method: method.to_string(),
            path: path.to_string(),
            body: body.to_vec(),
        }
    }
}
