//! Persistent multi-source demand inside the existing desktop task database.
use super::*;

fn all_scope() -> Value {
    json!({"kind":"all","value":null})
}
pub(super) fn normalize_scope(scope: &Value) -> Result<Value> {
    match scope["kind"].as_str() {
        Some("all") => Ok(all_scope()),
        Some("region") => Ok(
            json!({"kind":"region","value":scope["value"].as_str().context("A region scope requires its exact region name")?}),
        ),
        Some("layer") => {
            let text = match &scope["value"] {
                Value::String(value) => value.clone(),
                Value::Number(value) => value.to_string(),
                _ => bail!("A layer scope requires an integer layer"),
            };
            let value = text
                .trim()
                .parse::<i32>()
                .context("A layer scope must fit a signed 32-bit integer")?;
            Ok(json!({"kind":"layer","value":value.to_string()}))
        }
        _ => bail!("Unknown projection source scope"),
    }
}
fn one() -> u64 {
    1
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct SourceInput {
    pub source_id: Option<String>,
    pub source_path: PathBuf,
    pub project_id: Option<String>,
    #[serde(default = "all_scope")]
    pub scope: Value,
    #[serde(default)]
    pub include_container_items: bool,
    #[serde(default = "one")]
    pub copies: u64,
    pub source_revision: Option<String>,
    pub snapshot: Option<crate::stats_api::StableStructureOutput>,
    pub context: Option<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TaskSource {
    pub source_id: String,
    pub source_path: PathBuf,
    pub project_id: String,
    pub scope: Value,
    pub include_container_items: bool,
    pub copies: u64,
    pub minecraft_version: String,
    pub resource_identity: String,
    pub source_revision: String,
    pub status: String,
    pub context: Value,
    pub label: String,
    pub base_materials: StockpileMaterialsData,
    pub material_semantics: String,
    pub icon_images: BTreeMap<String, String>,
    pub item_names: BTreeMap<String, BTreeMap<String, String>>,
    pub source_scopes: BTreeMap<String, Vec<crate::stats_api::MaterialScopeCount>>,
    pub source_min_y: i32,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SourceView {
    source_id: String,
    source_path: PathBuf,
    project_id: String,
    scope: Value,
    include_container_items: bool,
    copies: u64,
    minecraft_version: String,
    resource_identity: String,
    source_revision: String,
    status: String,
    context: Value,
    label: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DemandSource {
    source_id: String,
    source_path: PathBuf,
    label: String,
    copies: u64,
    per_copy_count: u64,
    required_count: u64,
    scope: Value,
}
fn path_key(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}
fn selector(path: &Path, scope: &Value, containers: bool) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(
            path_key(path),
            normalize_scope(scope)?,
            containers
        ))?)
    ))
}
fn matches(source: &TaskSource, input: &SourceInput) -> bool {
    let (Ok(left), Ok(right)) = (
        normalize_scope(&source.scope),
        normalize_scope(&input.scope),
    ) else {
        return false;
    };
    desktop_task_source_matches(&source.source_path, &input.source_path)
        && left == right
        && source.include_container_items == input.include_container_items
}
fn resource(context: &Value, semantics: &str) -> String {
    context["resourceIdentity"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| semantics.rsplit(':').next().unwrap_or(semantics).into())
}
pub(super) fn legacy(record: &DesktopTaskRecord) -> Result<TaskSource> {
    let scope = normalize_scope(
        &record
            .context
            .get("scope")
            .cloned()
            .unwrap_or_else(all_scope),
    )?;
    let containers = record.context["includeContainerItems"]
        .as_bool()
        .unwrap_or(false);
    let mut context = record.context.clone();
    context["scope"] = scope.clone();
    context["includeContainerItems"] = containers.into();
    Ok(TaskSource {
        source_id: selector(&record.source_path, &scope, containers)?,
        source_path: record.source_path.clone(),
        project_id: record.project_id.clone(),
        scope,
        include_container_items: containers,
        copies: record.multiplier,
        minecraft_version: record.minecraft_version.clone(),
        resource_identity: resource(&record.context, &record.material_semantics),
        source_revision: record.source_revision.clone(),
        status: "current".into(),
        context,
        label: record
            .source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&record.name)
            .into(),
        base_materials: record.base_materials.clone(),
        material_semantics: record.material_semantics.clone(),
        icon_images: record.icon_images.clone(),
        item_names: record.item_names.clone(),
        source_scopes: record.source_scopes.clone(),
        source_min_y: record.source_min_y,
    })
}
pub(super) fn migrate(record: &mut DesktopTaskRecord) -> Result<()> {
    if record.sources.is_empty() && !record.sources_initialized {
        record.sources = vec![legacy(record)?];
        record.sources_initialized = true;
    }
    for source in &mut record.sources {
        source.scope = normalize_scope(&source.scope)?;
        source.context["scope"] = source.scope.clone();
    }
    if record.material_keys.is_empty() {
        for source in &record.sources {
            for item in &source.base_materials.materials {
                record
                    .material_keys
                    .entry(semantic_key(source, item))
                    .or_insert_with(|| item.namespace_id.clone());
                record
                    .material_contexts
                    .entry(item.namespace_id.clone())
                    .or_insert_with(|| {
                        (
                            source.minecraft_version.clone(),
                            source.material_semantics.clone(),
                        )
                    });
            }
        }
    }
    Ok(())
}
pub(super) fn views(record: &DesktopTaskRecord) -> Vec<SourceView> {
    record
        .sources
        .iter()
        .map(|s| SourceView {
            source_id: s.source_id.clone(),
            source_path: s.source_path.clone(),
            project_id: s.project_id.clone(),
            scope: s.scope.clone(),
            include_container_items: s.include_container_items,
            copies: s.copies,
            minecraft_version: s.minecraft_version.clone(),
            resource_identity: s.resource_identity.clone(),
            source_revision: s.source_revision.clone(),
            status: s.status.clone(),
            context: s.context.clone(),
            label: s.label.clone(),
        })
        .collect()
}
pub(super) fn counts(record: &DesktopTaskRecord) -> (usize, u64) {
    (
        record.sources.len(),
        record.sources.iter().map(|s| s.copies).sum(),
    )
}
pub(super) fn create_id(request: &DesktopTaskRequest) -> Result<Option<String>> {
    request
        .request_id
        .as_deref()
        .map(|id| {
            validate_request_id(id)?;
            Ok(format!(
                "{:x}",
                Sha256::digest(format!("stockpile-create:{id}"))
            ))
        })
        .transpose()
}
fn validate_request_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 128 {
        bail!("Invalid source change request identity");
    }
    Ok(())
}
pub(super) fn receipt(conn: &Connection, request: &DesktopTaskRequest) -> Result<bool> {
    let Some(id) = request.request_id.as_deref() else {
        return Ok(false);
    };
    validate_request_id(id)?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS desktop_source_receipt(request_id TEXT PRIMARY KEY,action TEXT NOT NULL,intent TEXT NOT NULL);")?;
    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT action,intent FROM desktop_source_receipt WHERE request_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((action, intent)) = existing {
        if action != request.action || intent != intent_hash(request)? {
            bail!("Source request identity was already used for a different operation");
        }
        return Ok(true);
    }
    Ok(false)
}
fn intent_hash(request: &DesktopTaskRequest) -> Result<String> {
    let sources=request.sources.as_deref().unwrap_or_default().iter().map(|s|json!({"sourceId":s.source_id,"path":path_key(&s.source_path),"scope":s.scope,"containers":s.include_container_items,"copies":s.copies})).collect::<Vec<_>>();
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(
            request.name.as_ref(),
            sources,
            request.multiplier
        ))?)
    ))
}
pub(super) fn save_receipt(conn: &Connection, request: &DesktopTaskRequest) -> Result<()> {
    if let Some(id) = request.request_id.as_deref() {
        conn.execute(
            "INSERT INTO desktop_source_receipt(request_id,action,intent) VALUES(?1,?2,?3)",
            params![id, request.action, intent_hash(request)?],
        )?;
    }
    Ok(())
}
fn prepared(
    input: &SourceInput,
    old: Option<&TaskSource>,
    request: &DesktopTaskRequest,
) -> Result<TaskSource> {
    desktop_task_multiplier(input.copies)?;
    if !input.scope["kind"].is_string() {
        bail!("Source scope must identify its kind");
    }
    if input.source_path.as_os_str().is_empty() {
        bail!("Source path is required");
    }
    let mut source = if input.snapshot.is_some() {
        let mut single: DesktopTaskRequest = serde_json::from_value(json!({}))?;
        single.project_id = input
            .project_id
            .clone()
            .context("Prepared source has no projection identity")?;
        single.source_path = Some(input.source_path.clone());
        single.source_revision = input.source_revision.clone();
        single.snapshot = input.snapshot.clone();
        single.context = input.context.clone();
        single.ui_locale = request.ui_locale.clone();
        single.language_map = request.language_map.clone();
        let record = desktop_task_record_from_request(&single, new_desktop_task_id())?;
        legacy(&record)?
    } else {
        let old =
            old.context("Source requirements are unavailable; explicitly synchronize this source")?;
        if !matches(old, input)
            || input
                .project_id
                .as_ref()
                .is_some_and(|id| id != &old.project_id)
        {
            bail!("Changed source options require a new confirmed requirement snapshot");
        }
        if input
            .source_revision
            .as_ref()
            .is_some_and(|revision| revision != &old.source_revision)
            || input
                .context
                .as_ref()
                .and_then(|c| c["resourceIdentity"].as_str())
                .is_some_and(|identity| identity != old.resource_identity)
        {
            bail!("Changed source revision or resources require a freshly prepared snapshot");
        }
        old.clone()
    };
    source.source_id = input
        .source_id
        .clone()
        .or_else(|| old.map(|s| s.source_id.clone()))
        .unwrap_or(selector(
            &input.source_path,
            &input.scope,
            input.include_container_items,
        )?);
    if source.source_id.is_empty() || source.source_id.len() > 128 {
        bail!("Invalid source identity");
    }
    source.source_path = input.source_path.clone();
    source.scope = normalize_scope(&input.scope)?;
    source.include_container_items = input.include_container_items;
    source.copies = input.copies;
    if let Some(context) = input.context.as_ref() {
        source.context = context.clone();
    }
    source.context["scope"] = source.scope.clone();
    source.context["includeContainerItems"] = source.include_container_items.into();
    source.resource_identity = resource(&source.context, &source.material_semantics);
    Ok(source)
}
pub(super) fn replace(
    record: &mut DesktopTaskRecord,
    request: &DesktopTaskRequest,
    append: bool,
    partial: bool,
) -> Result<bool> {
    migrate(record)?;
    let inputs = request
        .sources
        .as_ref()
        .context("A source list is required")?;
    let previous = record.sources.clone();
    let mut result = if append || partial {
        previous.clone()
    } else {
        Vec::new()
    };
    let mut touched = BTreeSet::new();
    for input in inputs {
        normalize_scope(&input.scope)?;
        if partial
            && request
                .source_ids
                .as_ref()
                .is_some_and(|ids| input.source_id.as_ref().is_none_or(|id| !ids.contains(id)))
        {
            bail!("Synchronized source is not selected by this request");
        }
        let old = if let Some(id) = input.source_id.as_ref() {
            previous.iter().find(|s| &s.source_id == id)
        } else {
            previous.iter().find(|s| matches(s, input))
        };
        if partial && old.is_none() {
            bail!("Synchronized source no longer belongs to this task");
        }
        let mut source = prepared(input, old, request)?;
        if !touched.insert(source.source_id.clone()) && partial {
            bail!("Duplicate synchronized source");
        }
        let same = result.iter().position(|s| matches(s, input));
        let by_id = result.iter().position(|s| s.source_id == source.source_id);
        if let Some(index) = if partial { by_id } else { same.or(by_id) } {
            if partial {
                result[index] = source;
            } else {
                if result[index].source_revision != source.source_revision {
                    bail!(
                        "Repeated source has a different snapshot; synchronize it explicitly first"
                    );
                }
                source.source_id = result[index].source_id.clone();
                source.copies = result[index]
                    .copies
                    .checked_add(source.copies)
                    .context("Source copies overflow")?;
                desktop_task_multiplier(source.copies)?;
                result[index] = source;
            }
        } else {
            result.push(source);
        }
    }
    let mut total = 0_u64;
    for source in &result {
        total = total
            .checked_add(source.copies)
            .context("Total source copies overflow")?;
    }
    if total > i64::MAX as u64 {
        bail!("Total source copies exceed the supported database quantity");
    }
    let stamp = |sources: &[TaskSource]| -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(
            &sources
                .iter()
                .map(|s| {
                    (
                        &s.source_id,
                        &s.source_path,
                        &s.source_revision,
                        s.copies,
                        &s.scope,
                        s.include_container_items,
                        &s.resource_identity,
                        &s.context,
                    )
                })
                .collect::<Vec<_>>(),
        )?)
    };
    let revision = stamp(&result)?;
    if revision == stamp(&record.sources)? {
        return Ok(false);
    }
    record.sources = result;
    record.sources_initialized = true;
    record.multiplier = 1;
    aggregate(record)?;
    record.source_revision = format!("{:x}", Sha256::digest(revision));
    record.recipe_trees = Some(BTreeMap::new());
    Ok(true)
}
pub(super) fn from_request(
    request: &DesktopTaskRequest,
    task_id: String,
) -> Result<DesktopTaskRecord> {
    let input = request
        .sources
        .as_ref()
        .and_then(|s| s.first())
        .context("Select at least one projection")?;
    let first = prepared(input, None, request)?;
    let mut record = DesktopTaskRecord {
        task_id,
        project_id: first.project_id.clone(),
        name: desktop_task_name(request.name.as_deref().unwrap_or(&first.label))?,
        source_path: first.source_path.clone(),
        source_revision: first.source_revision.clone(),
        multiplier: 1,
        created_at: current_unix_timestamp()?,
        updated_at: current_unix_timestamp()?,
        last_opened_at: 0,
        context: first.context.clone(),
        preferences: match request.preferences.as_ref() {
            Some(p) => desktop_task_preferences_patch(&DesktopTaskPreferences::default(), p)?,
            None => DesktopTaskPreferences::default(),
        },
        minecraft_version: first.minecraft_version.clone(),
        material_semantics: first.material_semantics.clone(),
        base_materials: first.base_materials.clone(),
        recipe_trees: Some(BTreeMap::new()),
        icon_images: first.icon_images.clone(),
        item_names: first.item_names.clone(),
        source_scopes: first.source_scopes.clone(),
        source_min_y: first.source_min_y,
        sources: Vec::new(),
        sources_initialized: true,
        material_keys: BTreeMap::new(),
        material_history: BTreeMap::new(),
        material_contexts: BTreeMap::new(),
    };
    replace(&mut record, request, false, false)?;
    Ok(record)
}
fn semantic_key(source: &TaskSource, item: &crate::stockpile::StockpileMaterialItem) -> String {
    let family = source
        .material_semantics
        .split(':')
        .next()
        .unwrap_or(&source.material_semantics);
    let plain = item.namespace_id == item.item_icon_key
        && item.namespace_id.starts_with("minecraft:")
        && family.starts_with("minecraft-inventory-v");
    // NBT/components and unknown/mod identities are never inferred equivalent
    // across versions or resource contexts merely from their display name.
    let domain = if plain {
        family.to_string()
    } else {
        format!(
            "{family}:{}:{}",
            source.minecraft_version, source.resource_identity
        )
    };
    format!("{domain}|{}|{}", item.namespace_id, item.stack_size)
}
pub(super) fn aggregate(record: &mut DesktopTaskRecord) -> Result<()> {
    for item in &record.base_materials.materials {
        record
            .material_history
            .insert(item.namespace_id.clone(), item.clone());
    }
    let mut materials = record.base_materials.clone();
    materials.materials.clear();
    materials.diagnostics.clear();
    let mut rows = BTreeMap::<String, crate::stockpile::StockpileMaterialItem>::new();
    let mut active_owners = BTreeSet::new();
    let mut used = record
        .material_keys
        .values()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut scopes = BTreeMap::<String, Vec<crate::stats_api::MaterialScopeCount>>::new();
    for source in &record.sources {
        materials
            .diagnostics
            .extend(source.base_materials.diagnostics.clone());
        record.icon_images.extend(source.icon_images.clone());
        record.item_names.extend(source.item_names.clone());
        for item in &source.base_materials.materials {
            let key = semantic_key(source, item);
            let id = if let Some(id) = record.material_keys.get(&key) {
                id.clone()
            } else {
                let id = if used.contains(&item.namespace_id) {
                    format!(
                        "{}#source-{:x}",
                        item.namespace_id,
                        Sha256::digest(key.as_bytes())
                    )
                } else {
                    item.namespace_id.clone()
                };
                used.insert(id.clone());
                record.material_keys.insert(key, id.clone());
                id
            };
            if id != item.namespace_id {
                materials.diagnostics.push(crate::stockpile::StockpileDiagnostic{kind:"separate_source_semantics".into(),source:source.source_id.clone(),target:id.clone(),message:"Item state, stack semantics or version equivalence is not established; demand remains separate".into()});
            }
            let count = item
                .required_count
                .checked_mul(source.copies)
                .filter(|n| *n <= i64::MAX as u64)
                .with_context(|| {
                    format!(
                        "Requirement overflow for {} in {}",
                        item.namespace_id, source.label
                    )
                })?;
            if active_owners.insert(id.clone()) {
                record.material_contexts.insert(
                    id.clone(),
                    (
                        source.minecraft_version.clone(),
                        source.material_semantics.clone(),
                    ),
                );
            }
            let row = rows.entry(id.clone()).or_insert_with(|| {
                let mut row = item.clone();
                row.id = id.clone();
                row.namespace_id = id.clone();
                row.required_count = 0;
                row
            });
            row.required_count = row
                .required_count
                .checked_add(count)
                .filter(|n| *n <= i64::MAX as u64)
                .with_context(|| format!("Combined requirement overflow for {id}"))?;
            for region in &item.source_regions {
                if !row.source_regions.contains(region) {
                    row.source_regions.push(region.clone());
                }
            }
            for mut scope in source
                .source_scopes
                .get(&item.namespace_id)
                .cloned()
                .unwrap_or_default()
            {
                scope.block_count = scope
                    .block_count
                    .checked_mul(source.copies)
                    .context("Source block quantity overflow")?;
                scope.container_item_count = scope
                    .container_item_count
                    .checked_mul(source.copies)
                    .context("Source container quantity overflow")?;
                scope.layer = scope
                    .layer
                    .map(|layer| {
                        source
                            .source_min_y
                            .checked_add(layer)
                            .context("Source layer coordinate overflow")
                    })
                    .transpose()?;
                scope.region = format!("{} / {}", source.label, scope.region);
                scopes.entry(id.clone()).or_default().push(scope);
            }
        }
    }
    materials.materials = rows.into_values().collect();
    materials.summary.total_blocks =
        materials.materials.iter().try_fold(0_u64, |total, item| {
            total
                .checked_add(item.required_count)
                .context("Task quantity overflow")
        })?;
    materials.summary.unique_materials = materials.materials.len();
    desktop_task_update_units(&mut materials)?;
    record.base_materials = materials;
    record.source_scopes = scopes;
    record.source_min_y = 0;
    Ok(())
}
pub(super) fn include_history(
    conn: &Connection,
    record: &DesktopTaskRecord,
    materials: &mut StockpileMaterialsData,
) -> Result<()> {
    let mut ids = load_claims_for_user(conn, "desktop-local")?
        .into_iter()
        .map(|c| c.material_id)
        .collect::<BTreeSet<_>>();
    ids.extend(load_material_notes(conn)?.into_keys());
    for id in ids {
        if materials
            .materials
            .iter()
            .any(|item| item.namespace_id == id)
        {
            continue;
        }
        if let Some(item) = record.material_history.get(&id) {
            let mut item = item.clone();
            item.required_count = 0;
            materials.materials.push(item);
        }
    }
    materials.summary.unique_materials = materials.materials.len();
    desktop_task_update_units(materials)
}
pub(super) fn demand(record: &DesktopTaskRecord, id: Option<&str>) -> Result<Vec<DemandSource>> {
    let Some(id) = id else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for s in &record.sources {
        let mut per_copy = 0_u64;
        for item in &s.base_materials.materials {
            if record
                .material_keys
                .get(&semantic_key(s, item))
                .is_some_and(|key| key == id)
            {
                per_copy = per_copy
                    .checked_add(item.required_count)
                    .context("Source requirement overflow")?;
            }
        }
        if per_copy > 0 {
            result.push(DemandSource {
                source_id: s.source_id.clone(),
                source_path: s.source_path.clone(),
                label: s.label.clone(),
                copies: s.copies,
                per_copy_count: per_copy,
                required_count: per_copy
                    .checked_mul(s.copies)
                    .context("Source requirement overflow")?,
                scope: s.scope.clone(),
            });
        }
    }
    Ok(result)
}
pub(super) fn details(
    record: &mut DesktopTaskRecord,
    needed: &StockpileMaterialsData,
) -> Result<()> {
    for item in &needed.materials {
        let (version, semantics) = record
            .material_contexts
            .get(&item.namespace_id)
            .context("Material source snapshot is unavailable")?;
        let active = crate::game_data::active_context()?;
        if !semantics.ends_with(&format!(":{}", active.content_id)) {
            bail!("This material uses a different resource snapshot; select its source game data before updating recipe details");
        }
        let mut one = needed.clone();
        one.materials = vec![item.clone()];
        let resolved = desktop_task_recipe_trees(version, &one)?;
        let names =
            crate::item_names::resolve_stockpile_item_names_cached(version, &mut one, &resolved)?;
        let icons =
            crate::item_icons::resolve_stockpile_icons_cached(version, &mut one, &resolved)?;
        let files = icons
            .files
            .into_iter()
            .map(|file| {
                (
                    file.path,
                    format!(
                        "data:image/png;base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(file.bytes)
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        record.icon_images.extend(
            icons
                .payload
                .by_key
                .into_iter()
                .filter_map(|(key, icon)| files.get(&icon.path).cloned().map(|value| (key, value))),
        );
        record.item_names.extend(names.names);
        record
            .recipe_trees
            .get_or_insert_with(BTreeMap::new)
            .extend(resolved);
    }
    Ok(())
}
