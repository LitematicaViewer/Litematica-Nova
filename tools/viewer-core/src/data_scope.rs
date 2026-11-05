use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaterialScopeKind {
    All,
    Region,
    Layer,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialScope {
    pub kind: MaterialScopeKind,
    pub value: Option<String>,
}

/// Per-scope joins are internal working data, not part of the Host's analysis response.
pub(crate) fn material_snapshot_dto(mut snapshot: Value) -> Value {
    for key in ["material_items", "non_item_blocks", "structural_block_counts"] {
        if let Some(rows) = snapshot.get_mut(key).and_then(Value::as_array_mut) {
            for row in rows.iter_mut().filter_map(Value::as_object_mut) {
                row.remove("scope_counts");
            }
        }
    }
    if let Some(scan) = snapshot.get_mut("container_scan").and_then(Value::as_object_mut) {
        scan.remove("scope_entries");
    }
    snapshot
}

pub(crate) fn material_snapshot_for_scope(
    mut snapshot: Value,
    scope: &MaterialScope,
) -> Result<Value, String> {
    if scope.kind == MaterialScopeKind::All {
        return Ok(snapshot);
    }
    // Whole-structure derivation is not a material-scope result. Never retain
    // global ratios on a selected-region/layer DTO. Scoped Core omits it too.
    snapshot
        .as_object_mut()
        .ok_or("统计快照格式异常")?
        .remove("derived");
    let value = scope
        .value
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "请选择有效统计范围".to_owned())?;
    let selected_layer = if scope.kind == MaterialScopeKind::Layer {
        Some(
            value
                .parse::<i32>()
                .map_err(|_| "层数必须是整数".to_owned())?,
        )
    } else {
        None
    };
    let summaries = snapshot[if selected_layer.is_some() {
        "layers_summary"
    } else {
        "regions"
    }]
    .as_array()
    .ok_or_else(|| "统计快照缺少范围信息".to_owned())?;
    let summary = summaries
        .iter()
        .find(|row| {
            if let Some(layer) = selected_layer {
                row["layer"].as_i64() == Some(i64::from(layer))
            } else {
                row["name"].as_str() == Some(value)
            }
        })
        .cloned()
        .ok_or_else(|| "统计范围已不存在，请重新选择".to_owned())?;
    let layer_y = snapshot["layers_summary"]
        .as_array()
        .ok_or_else(|| "统计快照缺少层信息".to_owned())?
        .iter()
        .filter_map(|row| Some((row["layer"].as_i64()?, row["world_y"].as_i64()?)))
        .collect::<BTreeMap<_, _>>();
    let min_world_y = layer_y.get(&0).copied().unwrap_or(0);
    let matches = |row: &Value| {
        if let Some(layer) = selected_layer {
            row["layer"].as_i64() == Some(i64::from(layer))
        } else {
            row["region"].as_str() == Some(value)
        }
    };
    let source_rows = snapshot["material_items"]
        .as_array_mut()
        .ok_or_else(|| "统计快照缺少材料".to_owned())?;
    let mut materials = Vec::new();
    for mut row in std::mem::take(source_rows) {
        let joints = row.get_mut("scope_counts")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| "统计核心未返回分范围数据，请更新统计核心".to_owned())?;
        let (mut blocks, mut containers, mut custom) = (0_u64, 0_u64, false);
        let mut regions = BTreeMap::<String, u64>::new();
        let mut layers = BTreeMap::<i64, u64>::new();
        joints.retain(&matches);
        for joint in joints.iter() {
            let block = joint["block_count"].as_u64().unwrap_or(0);
            let container = joint["container_item_count"].as_u64().unwrap_or(0);
            blocks += block;
            containers += container;
            custom |= joint["has_custom_stack_size"].as_bool().unwrap_or(false);
            *regions
                .entry(joint["region"].as_str().unwrap_or("").to_owned())
                .or_default() += block + container;
            if let Some(layer) = joint["layer"].as_i64() {
                *layers.entry(layer).or_default() += block + container;
            }
        }
        if blocks + containers == 0 {
            continue;
        }
        row["block_count"] = blocks.into();
        row["container_item_count"] = containers.into();
        row["count"] = (blocks + containers).into();
        row["total_count"] = (blocks + containers).into();
        row["has_custom_stack_size"] = custom.into();
        row["region_counts"] = Value::Array(
            regions
                .into_iter()
                .map(|(region, count)| json!({"region":region,"count":count}))
                .collect(),
        );
        row["layer_counts"] = Value::Array(layers.into_iter().map(|(layer, count)| json!({"layer":layer,"world_y":min_world_y+layer,"count":count})).collect());
        materials.push(row);
    }
    materials.sort_by(|left, right| {
        right["total_count"]
            .as_u64()
            .cmp(&left["total_count"].as_u64())
            .then_with(|| left["block_id"].as_str().cmp(&right["block_id"].as_str()))
    });
    snapshot["material_items"] = Value::Array(materials);
    for (key, missing_rows, missing_counts) in [
        ("non_item_blocks", "统计快照缺少结构说明", "统计核心未返回分范围结构说明"),
        ("structural_block_counts", "统计快照缺少原结构分布，请刷新统计核心", "统计核心未返回原结构范围数据"),
    ] {
        let rows = snapshot[key].as_array_mut().ok_or(missing_rows)?;
        let mut selected = Vec::new();
        for mut row in std::mem::take(rows) {
            let counts = row.get_mut("scope_counts").and_then(Value::as_array_mut).ok_or(missing_counts)?;
            counts.retain(&matches);
            let count: u64 = counts.iter().map(|joint| joint["count"].as_u64().unwrap_or(0)).sum();
            if count != 0 {
                row["count"] = count.into();
                selected.push(row);
            }
        }
        *rows = selected;
    }
    if snapshot["container_scan"]["enabled"].as_bool() == Some(true) {
        let entries = snapshot["container_scan"]["scope_entries"]
            .as_array()
            .ok_or_else(|| "统计核心未返回分范围容器信息".to_owned())?;
        let (mut containers, mut stacks) = (0_u64, 0_u64);
        let mut warnings = Vec::new();
        for entry in entries {
            if matches(entry) {
                containers += entry["containers_scanned"].as_u64().unwrap_or(0);
                stacks += entry["item_stacks_scanned"].as_u64().unwrap_or(0);
                warnings.extend(entry["warnings"].as_array().into_iter().flatten().cloned());
            } else if selected_layer.is_some() && entry["layer"].is_null() {
                if let Some(warning) = entry["missing_layer_warning"].as_str() {
                    warnings.push(warning.into());
                }
            }
        }
        snapshot["container_scan"]["containers_scanned"] = containers.into();
        snapshot["container_scan"]["item_stacks_scanned"] = stacks.into();
        snapshot["container_scan"]["warnings"] = Value::Array(warnings);
    }
    snapshot["structure_stats"]["scope"] = if let Some(layer) = selected_layer {
        json!({"kind":"layer","layer":layer,"world_y":summary["world_y"]})
    } else {
        json!({"kind":"region","region":value})
    };
    snapshot["structure_stats"]["total_non_air_blocks"] = summary["total_non_air_blocks"].clone();
    snapshot["structure_stats"]["unique_block_types"] = summary["unique_block_types"].clone();
    Ok(snapshot)
}
