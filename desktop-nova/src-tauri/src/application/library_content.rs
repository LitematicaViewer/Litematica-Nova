//! Authoritative content identity transforms, shared by import, scan and saved-library convergence.
//! Physical copies remain intact; only their logical content ownership changes.
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
fn values(value: &Value, key: &str) -> Vec<Value> {
    items(value, key).to_vec()
}
fn items<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key].as_array().map(Vec::as_slice).unwrap_or_default()
}
fn key(path: &str) -> String {
    let mut windows = path.replace('/', "\\");
    if windows.to_ascii_lowercase().starts_with("\\\\?\\unc\\") {
        windows = format!("\\\\{}", &windows[8..]);
    } else if windows.starts_with("\\\\?\\") && windows.as_bytes().get(5) == Some(&b':') {
        windows = windows[4..].into();
    }
    let drive =
        windows.as_bytes().get(1) == Some(&b':') && windows.as_bytes().get(2) == Some(&b'\\');
    let unc = windows.starts_with("\\\\")
        && !windows.starts_with("\\\\?\\")
        && !windows.starts_with("\\\\.\\");
    if drive || unc {
        let mut normalized = String::new();
        for ch in windows.chars() {
            if ch != '\\' || !normalized.ends_with('\\') {
                normalized.push(ch);
            }
        }
        if unc {
            normalized.insert(0, '\\');
        }
        if !(drive && normalized.len() == 3) {
            normalized = normalized.trim_end_matches('\\').into();
        }
        format!("windows:{}", normalized.to_lowercase())
    } else {
        format!(
            "native:{}",
            if path == "/" {
                path
            } else {
                path.trim_end_matches('/')
            }
        )
    }
}
fn parent(path: &str) -> String {
    let path = path.trim().trim_end_matches(['/', '\\']);
    let Some(index) = path.rfind(['/', '\\']) else {
        return String::new();
    };
    let parent = &path[..index];
    if parent.ends_with(':') {
        path[..=index].into()
    } else {
        parent.into()
    }
}
fn filename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or("")
}
fn under(path: &str, folder: &Value) -> bool {
    let folder_key = key(text(folder, "path"));
    let file_key = key(path);
    file_key == folder_key
        || if folder["recursive"].as_bool().unwrap_or(false) {
            file_key.starts_with(&(folder_key.clone() + "\\"))
                || file_key.starts_with(&(folder_key + "/"))
        } else {
            key(&parent(path)) == folder_key
        }
}
fn index(entries: &[Value]) -> BTreeMap<String, String> {
    let mut result: BTreeMap<String, String> = BTreeMap::new();
    for entry in entries {
        let checksum = text(entry, "checksum");
        if checksum.is_empty() || text(entry, "status") == "merged" {
            continue;
        }
        let incumbent = result
            .get(checksum)
            .and_then(|id| entries.iter().find(|e| text(e, "id") == id));
        let rank = |e: &Value| {
            (
                text(e, "status") != "active",
                e["createdAt"].as_u64().unwrap_or(0),
                text(e, "id").to_string(),
            )
        };
        if incumbent.is_none_or(|old| rank(entry) < rank(old)) {
            result.insert(checksum.into(), text(entry, "id").into());
        }
    }
    result
}
fn fresh_entry(entries: &[Value], checksum: &str, replica: &Value, at: u64) -> Value {
    let base = format!(
        "entry:content:{}",
        checksum.chars().take(16).collect::<String>()
    );
    let mut id = base.clone();
    let mut suffix = 0;
    while entries.iter().any(|entry| text(entry, "id") == id) {
        suffix += 1;
        id = format!("{base}:{suffix}");
    }
    let name = if text(replica, "fileName").is_empty() {
        filename(text(replica, "path"))
    } else {
        text(replica, "fileName")
    };
    json!({"id":id,"createdAt":at,"checksum":checksum,"status":"active","categoryIds":[],"displayName":name,"author":"","description":"","tags":[],"preview":{},
        "stats":{"totalBlocks":0,"totalVolume":0,"regionCount":0,"enclosingSize":{"x":0,"y":0,"z":0},"minecraftDataVersion":0},
        "sortIndex":entries.len(),"lastAnalyzedAt":replica["lastAnalyzedAt"],"lastError":replica["lastError"]})
}
fn relocate(
    entries: &mut Vec<Value>,
    lookup: &mut BTreeMap<String, String>,
    replica: &Value,
    at: u64,
) -> String {
    let checksum = text(replica, "checksum");
    if let Some(id) = lookup.get(checksum) {
        let id = id.clone();
        if let Some(entry) = entries.iter_mut().find(|e| text(e, "id") == id) {
            if text(entry, "status") == "retired" {
                entry["status"] = "active".into();
            }
        }
        return id;
    }
    let entry = fresh_entry(entries, checksum, replica, at);
    let id = text(&entry, "id").to_string();
    entries.push(entry);
    lookup.insert(checksum.into(), id.clone());
    id
}
fn retire(entries: &mut [Value], replicas: &[Value], candidates: &BTreeSet<String>) {
    for entry in entries {
        if candidates.contains(text(entry, "id"))
            && text(entry, "status") == "active"
            && !replicas
                .iter()
                .any(|r| text(r, "entryId") == text(entry, "id") && text(r, "status") == "active")
        {
            entry["status"] = "retired".into();
        }
    }
}
fn reconcile(state: &mut Value) -> Vec<Value> {
    let mut entries = values(state, "entries");
    let mut replicas = values(state, "replicas");
    let mut lookup = index(&entries);
    let mut moved = Vec::new();
    let mut orphans = BTreeSet::new();
    for replica in &mut replicas {
        let mut seen = BTreeSet::new();
        let mut owner = text(replica, "entryId").to_string();
        while seen.insert(owner.clone()) {
            let next = items(state, "redirects")
                .iter()
                .find(|r| text(r, "fromId") == owner);
            let Some(next) = next else { break };
            owner = text(next, "toId").into();
        }
        replica["entryId"] = owner.into();
        if text(replica, "status") != "active" || text(replica, "checksum").is_empty() {
            continue;
        }
        let Some(source) = entries
            .iter()
            .find(|e| text(e, "id") == text(replica, "entryId"))
            .cloned()
        else {
            continue;
        };
        if text(&source, "checksum").is_empty() || source["checksum"] == replica["checksum"] {
            continue;
        }
        let target = relocate(
            &mut entries,
            &mut lookup,
            replica,
            source["createdAt"].as_u64().unwrap_or(0),
        );
        if target == text(&source, "id") {
            continue;
        }
        moved.push(json!({"entryId":source["id"],"replicaId":replica["id"],"fromChecksum":source["checksum"],"toChecksum":replica["checksum"],"toEntryId":target}));
        replica["entryId"] = target.into();
        orphans.insert(text(&source, "id").to_string());
    }
    retire(&mut entries, &replicas, &orphans);
    state["entries"] = entries.into();
    state["replicas"] = replicas.into();
    moved
}
fn merge(state: &mut Value, keep_id: &str, discard_id: &str, at: u64) -> bool {
    if keep_id == discard_id {
        return false;
    }
    let mut entries = values(state, "entries");
    let Some(keep) = entries
        .iter()
        .position(|e| text(e, "id") == keep_id && text(e, "status") != "merged")
    else {
        return false;
    };
    let Some(discard) = entries
        .iter()
        .position(|e| text(e, "id") == discard_id && text(e, "status") != "merged")
    else {
        return false;
    };
    // Names are not identity, even when an explicit merge is requested.
    if text(&entries[keep], "checksum").is_empty()
        || entries[keep]["checksum"] != entries[discard]["checksum"]
    {
        return false;
    }
    for field in ["categoryIds", "tags"] {
        let mut union = values(&entries[keep], field);
        for item in values(&entries[discard], field) {
            if !union.contains(&item) {
                union.push(item);
            }
        }
        entries[keep][field] = union.into();
    }
    if entries[keep]["preview"]
        .as_object()
        .is_some_and(|value| value.is_empty())
        && entries[discard]["preview"].is_object()
    {
        entries[keep]["preview"] = entries[discard]["preview"].clone();
    }
    let mut replicas = values(state, "replicas");
    for replica in &mut replicas {
        if text(replica, "entryId") == discard_id {
            replica["entryId"] = keep_id.into();
        }
    }
    if replicas
        .iter()
        .any(|r| text(r, "entryId") == keep_id && text(r, "status") == "active")
    {
        entries[keep]["status"] = "active".into();
    }
    entries[discard]["status"] = "merged".into();
    let mut redirects = values(state, "redirects");
    redirects.push(json!({"fromId":discard_id,"toId":keep_id,"at":at}));
    state["entries"] = entries.into();
    state["replicas"] = replicas.into();
    state["redirects"] = redirects.into();
    true
}
fn merge_all(state: &mut Value, at: u64) -> Vec<Value> {
    let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for entry in values(state, "entries") {
        let sum = text(&entry, "checksum");
        if !sum.is_empty() && text(&entry, "status") != "merged" {
            groups.entry(sum.into()).or_default().push(entry);
        }
    }
    let mut merges = Vec::new();
    for group in groups.values_mut() {
        group.sort_by_key(|e| {
            (
                e["createdAt"].as_u64().unwrap_or(0),
                text(e, "id").to_string(),
            )
        });
        let keep = text(&group[0], "id");
        for other in group.iter().skip(1) {
            let discard = text(other, "id");
            if merge(state, keep, discard, at) {
                merges.push(json!({"fromId":discard,"toId":keep}));
            }
        }
    }
    merges
}
fn scan(state: &mut Value, files: &[Value], options: &Value) -> Value {
    let mut entries = values(state, "entries");
    let mut replicas = values(state, "replicas");
    let mut lookup = index(&entries);
    let now = options["now"].as_u64().unwrap_or(0);
    let mut by_path: BTreeMap<String, usize> = replicas
        .iter()
        .enumerate()
        .map(|(i, r)| (key(text(r, "path")), i))
        .collect();
    let present: BTreeSet<_> = files.iter().map(|f| key(text(f, "path"))).collect();
    let mut pending = BTreeMap::new();
    let mut added = 0;
    let mut revived = 0;
    for file in files {
        let path = text(file, "path");
        let path_key = key(path);
        let existing = by_path.get(&path_key).copied();
        let old = existing.map(|i| replicas[i].clone());
        if old.is_none()
            && items(options, "noAutoImportFolders")
                .iter()
                .any(|folder| under(path, folder))
        {
            continue;
        }
        let force = options["forceRecheck"].as_bool().unwrap_or(false);
        let cached = options["useCache"].as_bool().unwrap_or(false)
            && !force
            && old.as_ref().is_some_and(|r| {
                !text(r, "checksum").is_empty()
                    && r["fileSize"] == file["fileSize"]
                    && r["mtime"] == file["mtimeMs"]
            });
        let checksum = if cached {
            text(old.as_ref().unwrap(), "checksum")
        } else {
            text(file, "checksum")
        }.to_string();
        if checksum.is_empty() {
            continue;
        }
        let mut created = false;
        let target = if let Some(id) = lookup.get(&checksum) {
            id.clone()
        } else if let Some(entry) = old.as_ref().and_then(|r| {
            entries.iter_mut().find(|e| {
                e["id"] == r["entryId"]
                    && text(e, "status") != "merged"
                    && (e["checksum"].is_null() || text(e, "checksum") == checksum)
            })
        }) {
            entry["checksum"] = checksum.clone().into();
            text(entry, "id").into()
        } else {
            let mut entry = fresh_entry(&entries, &checksum, file, now);
            entry["lastAnalyzedAt"] = now.into();
            entry["lastError"] = "".into();
            let id = text(&entry, "id").to_string();
            entries.insert(0, entry);
            created = true;
            id
        };
        lookup.insert(checksum.clone(), target.clone());
        if let Some(entry) = entries.iter_mut().find(|e| text(e, "id") == target) {
            if text(entry, "status") == "retired" {
                entry["status"] = "active".into();
                revived += 1;
            }
        }
        let changed = old.as_ref().is_some_and(|r| {
            force
                || r["fileSize"] != file["fileSize"]
                || r["mtime"] != file["mtimeMs"]
                || text(r, "checksum") != checksum
                || text(r, "status") == "missing"
        });
        let mut replica = old.unwrap_or_else(|| {
            let base = format!("replica:{now}:{}", replicas.len());
            let mut id = base.clone();
            let mut i = 0;
            while replicas.iter().any(|r| text(r, "id") == id) {
                i += 1;
                id = format!("{base}:{i}");
            }
            json!({"id":id,"lastError":""})
        });
        replica["entryId"] = target.clone().into();
        replica["path"] = path.into();
        replica["fileName"] = if text(file, "fileName").is_empty() {
            filename(path)
        } else {
            text(file, "fileName")
        }
        .into();
        replica["folderPath"] = if text(file, "folderPath").is_empty() {
            parent(path)
        } else {
            text(file, "folderPath").into()
        }
        .into();
        replica["checksum"] = checksum.into();
        replica["fileSize"] = file["fileSize"].clone();
        replica["mtime"] = file["mtimeMs"].clone();
        replica["status"] = "active".into();
        replica["lastAnalyzedAt"] = now.into();
        if created || changed {
            pending.insert(target, path.to_string());
        }
        if let Some(i) = existing {
            replicas[i] = replica;
        } else {
            by_path.insert(path_key, replicas.len());
            replicas.push(replica);
            added += 1;
        }
    }
    let mut missing = Vec::new();
    for replica in &mut replicas {
        if text(replica, "status") == "active"
            && !present.contains(&key(text(replica, "path")))
            && items(options, "coveredFolders")
                .iter()
                .any(|f| under(text(replica, "path"), f))
        {
            replica["status"] = "missing".into();
            missing.push(replica["id"].clone());
        }
    }
    state["entries"] = entries.into();
    state["replicas"] = replicas.into();
    json!({"markedMissing":missing,"newReplicas":added,"revivedEntries":revived,"pendingAnalysis":pending.into_iter().map(|(entry_id,path)|json!({"entryId":entry_id,"path":path})).collect::<Vec<_>>()})
}

/// Read-only UI projection. Filtering and ordering use the same acknowledged state
/// as mutations; the webview only joins returned stable IDs to its display rows.
fn query_view(state: &Value, query: &Value) -> Value {
    let entries = items(state, "entries");
    let categories = items(state, "categories");
    let mut replicas_by_entry = BTreeMap::<&str, Vec<&Value>>::new();
    for replica in items(state, "replicas") {
        if text(replica, "status") != "trashed" {
            replicas_by_entry.entry(text(replica, "entryId")).or_default().push(replica);
        }
    }
    let descendants = |root: &str| {
        let mut ids = BTreeSet::from([root.to_owned()]);
        loop {
            let children: Vec<_> = categories.iter().filter(|c| ids.contains(text(c, "parentId"))).map(|c| text(c, "id").to_owned()).collect();
            let before = ids.len();
            ids.extend(children);
            if before == ids.len() { break; }
        }
        ids
    };
    let category = text(query, "category");
    let selected_categories = descendants(category);
    let category_scopes: Vec<_> = categories.iter().map(|c| (text(c, "id"), descendants(text(c, "id")))).collect();
    let favorites: BTreeSet<_> = items(state, "favorites").iter().filter_map(Value::as_str).collect();
    let recent: BTreeSet<_> = items(state, "recent").iter().filter(|r| text(r, "kind") == "entry").map(|r| text(r, "entryId")).collect();
    let matches = query["matchIds"].as_array().map(|ids| ids.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>());
    let search = text(query, "text").trim().to_lowercase();
    let mut rows = Vec::new();
    let mut paths = Vec::new();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut untagged = 0;
    let mut issues = 0;
    for entry in entries {
        if text(entry, "status") == "merged" { continue; }
        let id = text(entry, "id");
        let Some(copies) = replicas_by_entry.get_mut(id) else { continue };
        copies.sort_by_key(|r| (match text(r, "status") { "active" => 0, "missing" => 1, _ => 2 }, text(r, "fileName").to_lowercase()));
        let Some(&primary) = copies.first() else { continue; };
        let status = match text(primary, "status") { "active" => "ok", "missing" => "missing", _ => "parse_error" };
        if status != "ok" { issues += 1; }
        let memberships: Vec<_> = entry["categoryIds"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        if memberships.is_empty() { untagged += 1; }
        for (category_id, scope) in &category_scopes {
            if memberships.iter().any(|id| scope.contains(*id)) { *counts.entry((*category_id).into()).or_default() += 1; }
        }
        if !search.is_empty() {
            for replica in copies.iter() {
                let haystack = format!("{} {} {}", text(entry, "displayName"), text(replica, "fileName"), text(replica, "path")).to_lowercase();
                if haystack.contains(&search) { paths.push(text(replica, "path").to_owned()); }
            }
        } else { paths.extend(copies.iter().map(|r| text(r, "path").to_owned())); }
        if query["favoritesOnly"].as_bool().unwrap_or(false) && !favorites.contains(id) { continue; }
        if query["recentOnly"].as_bool().unwrap_or(false) && !recent.contains(id) { continue; }
        if !category.is_empty() && !memberships.iter().any(|id| selected_categories.contains(*id)) { continue; }
        if query["untaggedOnly"].as_bool().unwrap_or(false) && !memberships.is_empty() { continue; }
        if !text(query, "status").is_empty() && text(query, "status") != status { continue; }
        if matches.as_ref().is_some_and(|ids| !ids.contains(id)) { continue; }
        rows.push((entry, primary));
    }
    let name = |entry: &Value, replica: &Value| if text(entry, "displayName").is_empty() { text(replica, "fileName").to_lowercase() } else { text(entry, "displayName").to_lowercase() };
    rows.sort_by(|(a, ar), (b, br)| {
        let order = match text(query, "sort") {
            "recent_analyzed" => br["lastAnalyzedAt"].as_u64().unwrap_or(0).cmp(&ar["lastAnalyzedAt"].as_u64().unwrap_or(0)),
            "blocks" => b["stats"]["totalBlocks"].as_u64().unwrap_or(0).cmp(&a["stats"]["totalBlocks"].as_u64().unwrap_or(0)),
            "name" => name(a, ar).cmp(&name(b, br)),
            _ => a["sortIndex"].as_i64().unwrap_or(0).cmp(&b["sortIndex"].as_i64().unwrap_or(0)),
        };
        order.then_with(|| text(a, "id").cmp(text(b, "id")))
    });
    let mut groups = BTreeMap::<String, Value>::new();
    for (entry, _) in &rows {
        let (group_key, label) = match text(query, "group") {
            "author" => { let author = text(entry, "author").trim(); (format!("author:{}", if author.is_empty() { "__none".into() } else { author.to_lowercase() }), author.to_string()) },
            "version" => { let version = entry["stats"]["minecraftDataVersion"].as_u64().unwrap_or(0).to_string(); { let label = text(&query["versionLabels"], &version); (format!("version:{}", if label.is_empty() { "__none" } else { label }), label.to_string()) } },
            "size" => { let size = &entry["stats"]["enclosingSize"]; let value = format!("{}×{}×{}", size["x"].as_i64().unwrap_or(0), size["y"].as_i64().unwrap_or(0), size["z"].as_i64().unwrap_or(0)); (format!("size:{value}"), value) },
            _ => ("all".into(), String::new()),
        };
        let group = groups.entry(group_key.clone()).or_insert_with(|| json!({"key":group_key,"label":label,"entryIds":[]}));
        group["entryIds"].as_array_mut().unwrap().push(entry["id"].clone());
    }
    json!({"entryIds":rows.iter().map(|(e,_)|e["id"].clone()).collect::<Vec<_>>(),"groups":groups.into_values().collect::<Vec<_>>(),"paths":paths,"categoryCounts":counts,"untaggedCount":untagged,"issueCount":issues})
}

pub(crate) fn command(mut request: Value) -> Result<Value, String> {
    let mut state = request.get_mut("state").map(Value::take).unwrap_or_default();
    let action = text(&request, "action");
    if !state.is_object() || !state["entries"].is_array() || !state["replicas"].is_array() {
        return Err("Invalid library state".into());
    }
    let at = request["at"].as_u64().unwrap_or(0);
    match action {
        "query" => Ok(query_view(&state, &request["query"])),
        "index" => Ok(json!(index(items(&state, "entries")))),
        "reconcile" => {
            let moved = reconcile(&mut state);
            Ok(json!({"state":state,"moved":moved}))
        }
        "merge" => {
            merge(
                &mut state,
                text(&request, "keepId"),
                text(&request, "discardId"),
                at,
            );
            Ok(state)
        }
        "mergeAll" => {
            let merges = merge_all(&mut state, at);
            Ok(json!({"state":state,"merges":merges}))
        }
        "normalize" => {
            reconcile(&mut state);
            merge_all(&mut state, at);
            Ok(state)
        }
        "scan" => {
            let stats = scan(&mut state, items(&request, "files"), &request["options"]);
            Ok(json!({"state":state,"stats":stats}))
        }
        "reposition" => {
            let id = text(&request, "replicaId");
            let path = text(&request, "path");
            let checksum = text(&request, "checksum");
            let replicas = values(&state, "replicas");
            let source = replicas
                .iter()
                .find(|r| text(r, "id") == id)
                .ok_or("Library replica is missing")?;
            let occupant = replicas
                .iter()
                .find(|r| text(r, "id") != id && key(text(r, "path")) == key(path));
            let entries = values(&state, "entries");
            let old_checksum = occupant
                .and_then(|r| entries.iter().find(|e| e["id"] == r["entryId"]))
                .map(|e| e["checksum"].clone())
                .unwrap_or(Value::Null);
            if let Some(occupant) = occupant {
                if old_checksum.as_str() != Some(checksum) && request["onConflict"] != "relocate" {
                    return Ok(
                        json!({"kind":"conflict","entryId":source["entryId"],"occupiedByEntryId":occupant["entryId"],"targetChecksum":checksum,"occupantChecksum":old_checksum}),
                    );
                }
            }
            let mut next = replicas.clone();
            for replica in &mut next {
                if text(replica, "id") == id {
                    replica["path"] = path.into();
                    replica["fileName"] = filename(path).into();
                    replica["folderPath"] = parent(path).into();
                    replica["checksum"] = checksum.into();
                    replica["status"] = "active".into();
                } else if occupant.is_some_and(|o| o["id"] == replica["id"]) {
                    replica["checksum"] = checksum.into();
                }
            }
            state["replicas"] = next.into();
            if let Some(entry) = state["entries"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|e| e["id"] == source["entryId"] && e["checksum"].is_null())
            {
                entry["checksum"] = checksum.into();
            }
            reconcile(&mut state);
            merge_all(&mut state, at);
            let target = state["replicas"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| text(r, "id") == id)
                .unwrap()["entryId"]
                .clone();
            Ok(if occupant.is_some() {
                json!({"kind":"merged","state":state,"occupiedByEntryId":target})
            } else {
                json!({"kind":"repositioned","state":state})
            })
        }
        "sync" => {
            let id = text(&request, "entryId");
            let checksum = text(&request, "checksum");
            if checksum.is_empty() {
                return Err("Missing content checksum".into());
            }
            let acknowledged = request["replicaIds"]
                .as_array()
                .ok_or("Missing acknowledged replica identities")?;
            for replica in state["replicas"].as_array_mut().unwrap() {
                if text(replica, "entryId") == id
                    && text(replica, "status") == "active"
                    && acknowledged.contains(&replica["id"])
                {
                    replica["checksum"] = checksum.into();
                }
            }
            if let Some(entry) = state["entries"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|e| text(e, "id") == id)
            {
                entry["checksum"] = checksum.into();
                if text(entry, "status") == "retired" {
                    entry["status"] = "active".into();
                }
            }
            // Failed copies retain their true checksum and split back out at convergence.
            reconcile(&mut state);
            Ok(state)
        }
        "diverge" => {
            let id = text(&request, "replicaId");
            let source = values(&state, "replicas")
                .into_iter()
                .find(|r| text(r, "id") == id);
            let Some(source) = source else {
                return Ok(Value::Null);
            };
            let moved = reconcile(&mut state);
            let Some(change) = moved.iter().find(|m| text(m, "replicaId") == id) else {
                return Ok(Value::Null);
            };
            let retired = values(&state, "entries")
                .iter()
                .any(|e| e["id"] == source["entryId"] && text(e, "status") == "retired");
            Ok(json!({"state":state,"toEntryId":change["toEntryId"],"sourceEntryRetired":retired}))
        }
        _ => Err(format!("Unknown library content action: {action}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(id: &str, checksum: &str) -> Value {
        json!({"id":id,"checksum":checksum,"status":"active","createdAt":1,"categoryIds":[],"tags":[],"preview":{},"displayName":"same.litematic"})
    }
    fn replica(id: &str, owner: &str, checksum: &str) -> Value {
        json!({"id":id,"entryId":owner,"checksum":checksum,"path":format!("C:/lib/{id}.litematic"),"fileName":format!("{id}.litematic"),"status":"active","fileSize":10,"mtime":1})
    }
    fn state(entries: Vec<Value>, replicas: Vec<Value>) -> Value {
        json!({"entries":entries,"replicas":replicas,"redirects":[],"trash":[],"favorites":[],"custom":"preserved"})
    }
    #[test]
    fn query_filters_authoritative_entries_and_paths_without_mutating_state() {
        let mut a = entry("a", "A"); a["displayName"] = "Tower".into(); a["categoryIds"] = json!(["child"]); a["stats"] = json!({"totalBlocks":80,"minecraftDataVersion":3953});
        let mut b = entry("b", "B"); b["displayName"] = "Farm".into(); b["stats"] = json!({"totalBlocks":20,"minecraftDataVersion":3954});
        let mut copy_a = replica("tower", "a", "A"); copy_a["lastAnalyzedAt"] = 9.into();
        let mut copy_b = replica("farm", "b", "B"); copy_b["status"] = "missing".into(); copy_b["lastAnalyzedAt"] = 5.into();
        let mut original = state(vec![a,b], vec![copy_a,copy_b]); original["categories"] = json!([{"id":"parent","parentId":null},{"id":"child","parentId":"parent"}]); original["favorites"] = json!(["a"]);
        let filtered = query_view(&original, &json!({"category":"parent","sort":"blocks","favoritesOnly":true}));
        assert_eq!(filtered["entryIds"], json!(["a"])); assert_eq!(filtered["categoryCounts"]["parent"], 1); assert_eq!(filtered["issueCount"], 1); assert_eq!(filtered["untaggedCount"], 1);
        let search = query_view(&original, &json!({"text":"TOWER"})); assert_eq!(search["paths"], json!(["C:/lib/tower.litematic"]));
        let recent = query_view(&original, &json!({"sort":"recent_analyzed"})); assert_eq!(recent["entryIds"], json!(["a","b"]));
        let matched = query_view(&original, &json!({"matchIds":["b"],"status":"missing"})); assert_eq!(matched["entryIds"], json!(["b"]));
        let grouped = query_view(&original, &json!({"group":"version","versionLabels":{"3953":"1.21","3954":"1.21"}})); assert_eq!(grouped["groups"].as_array().unwrap().len(),1);
        assert_eq!(original["replicas"][1]["status"],"missing"); assert_eq!(original["entries"][0]["categoryIds"],json!(["child"]));
    }
    #[test]
    fn query_excludes_merged_and_trashed_and_terminates_category_cycles() {
        let mut hidden = entry("hidden","H"); hidden["status"] = "merged".into();
        let mut trashed = replica("trash","a","A"); trashed["status"] = "trashed".into();
        let mut original = state(vec![entry("a","A"),hidden],vec![trashed,replica("hidden","hidden","H")]);
        original["categories"] = json!([{"id":"x","parentId":"y"},{"id":"y","parentId":"x"}]);
        let view = query_view(&original, &json!({"category":"x"})); assert_eq!(view["entryIds"],json!([])); assert_eq!(view["paths"],json!([]));
    }
    #[test]
    fn content_identity_merges_metadata_not_files_and_follows_restored_aliases() {
        let mut older = entry("a", "same");
        older["categoryIds"] = json!(["one"]);
        older["tags"] = json!(["redstone"]);
        older["status"] = "retired".into();
        let mut newer = entry("b", "same");
        newer["categoryIds"] = json!(["two"]);
        newer["tags"] = json!(["farm"]);
        newer["preview"] = json!({"customPath":"saved.png"});
        let original = state(
            vec![older, newer, entry("other", "different")],
            vec![
                replica("physical-a", "a", "same"),
                replica("physical-b", "b", "same"),
                replica("other", "other", "different"),
            ],
        );
        let next = command(json!({"action":"normalize","state":original,"at":10})).unwrap();
        assert_eq!(next["replicas"].as_array().unwrap().len(), 3);
        assert_eq!(next["entries"][1]["status"], "merged");
        assert_eq!(next["entries"][0]["status"], "active");
        assert_eq!(next["entries"][0]["categoryIds"], json!(["one", "two"]));
        assert_eq!(next["entries"][0]["tags"], json!(["redstone", "farm"]));
        assert_eq!(next["entries"][0]["preview"]["customPath"], "saved.png");
        assert_eq!(next["replicas"][1]["entryId"], "a");
        assert_eq!(next["entries"][2]["status"], "active");
        assert_eq!(next["custom"], "preserved");
        let again = command(json!({"action":"normalize","state":next,"at":11})).unwrap();
        assert_eq!(again, next);
        let mut restored = next.clone();
        restored["replicas"][1]["entryId"] = "b".into();
        assert_eq!(
            command(json!({"action":"normalize","state":restored})).unwrap()["replicas"][1]
                ["entryId"],
            "a"
        );
    }
    #[test]
    fn dirty_content_and_failed_copies_keep_true_fingerprints() {
        let original = state(
            vec![entry("original", "old")],
            vec![
                replica("written", "original", "old"),
                replica("failed", "original", "old"),
            ],
        );
        let next=command(json!({"action":"sync","state":original,"entryId":"original","checksum":"new","replicaIds":["written"]})).unwrap();
        assert_eq!(next["replicas"][0]["checksum"], "new");
        assert_eq!(next["replicas"][0]["entryId"], "original");
        assert_eq!(next["replicas"][1]["checksum"], "old");
        assert_ne!(next["replicas"][1]["entryId"], "original");
        let unchanged = command(json!({"action":"reconcile","state":next})).unwrap();
        assert!(unchanged["moved"].as_array().unwrap().is_empty());
    }
    #[test]
    fn scan_same_content_moves_missing_revives_and_respects_disabled_import() {
        let file = |path: &str, sum: &str| json!({"path":path,"fileName":"same.litematic","fileSize":10,"mtimeMs":1,"checksum":sum});
        let options = json!({"now":10,"coveredFolders":[{"path":"C:/lib","recursive":true}],"useCache":false});
        let result=command(json!({"action":"scan","state":state(vec![],vec![]),"files":[file("C:/lib/a.litematic","X"),file("C:/lib/b.litematic","X"),file("C:/lib/c.litematic","Y")],"options":options})).unwrap();
        assert_eq!(result["state"]["entries"].as_array().unwrap().len(), 2);
        assert_eq!(result["state"]["replicas"].as_array().unwrap().len(), 3);
        assert_eq!(
            result["stats"]["pendingAnalysis"].as_array().unwrap().len(),
            2
        );
        let next=command(json!({"action":"scan","state":result["state"],"files":[file("C:/lib/moved.litematic","X")],"options":options})).unwrap();
        assert_eq!(next["stats"]["markedMissing"].as_array().unwrap().len(), 3);
        assert_eq!(next["state"]["entries"].as_array().unwrap().len(), 2);
        let disabled=command(json!({"action":"scan","state":next["state"],"files":[file("C:/lib/moved.litematic","Z"),file("C:/lib/new.litematic","Q")],"options":{"coveredFolders":[],"useCache":false,"noAutoImportFolders":[{"path":"C:/lib","recursive":true}]}})).unwrap();
        assert_eq!(disabled["stats"]["newReplicas"], 0);
        assert_eq!(
            disabled["stats"]["pendingAnalysis"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn checksum_index_and_platform_paths_keep_existing_identity_rules() {
        let mut retired = entry("retired", "A");
        retired["status"] = "retired".into();
        let mut merged = entry("merged", "B");
        merged["status"] = "merged".into();
        let map = index(&[
            retired,
            entry("active", "A"),
            merged,
            entry("b", "C"),
            entry("a", "C"),
        ]);
        assert_eq!(map["A"], "active");
        assert_eq!(map["C"], "a");
        assert!(!map.contains_key("B"));
        assert_eq!(key("C:/lib/X/"), key("\\\\?\\C:\\LIB\\x"));
        assert_eq!(
            key("\\\\server\\share\\x"),
            key("\\\\?\\UNC\\server\\share\\x")
        );
        assert_ne!(key("/lib/X"), key("/lib/x"));
    }
}
