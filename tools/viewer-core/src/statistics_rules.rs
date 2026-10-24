//! Structure-rule evaluation runs beside the analysis, never in the WebView.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};
#[derive(Clone)]
struct Collection {
    id: String,
    name: String,
    symbol: String,
    category: String,
    version: String,
    values: BTreeSet<String>,
}
fn normalized(s: &str) -> String {
    let s = s.trim().to_lowercase();
    if s.starts_with("e/") {
        if s[2..].contains(':') {
            s
        } else {
            format!("e/minecraft:{}", &s[2..])
        }
    } else if s.contains(':') {
        s
    } else {
        format!("minecraft:{s}")
    }
}
fn collections() -> Result<Arc<Vec<Collection>>> {
    type Cache = Option<(String, Arc<Vec<Collection>>)>;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let root = crate::runtime_paths::data_root()?;
    let builtin = crate::runtime_paths::app_root()?.join("pack-in");
    let context = crate::game_data::active_context()?;
    let mut files = Vec::<(String, PathBuf)>::new();
    for category in ["version", "system_enum", "creative", "map", "custom"] {
        let mut names = BTreeMap::new();
        for base in [&builtin, &root] {
            let dir = base.join("enumerator").join(category);
            if dir.is_dir() {
                for e in std::fs::read_dir(dir)? {
                    let path = e?.path();
                    if path.extension().is_some_and(|s| s == "json") {
                        names.insert(path.file_name().unwrap().to_owned(), path);
                    }
                }
            }
        }
        files.extend(names.into_values().map(|p| (category.into(), p)));
    }
    let key = format!(
        "{}:{:?}",
        context.content_id,
        files
            .iter()
            .map(|(_, p)| std::fs::metadata(p).map(|m| (p, m.len(), m.modified().ok())))
            .collect::<std::io::Result<Vec<_>>>()?
    );
    let slot = CACHE.get_or_init(Default::default);
    if let Some((old, values)) = &*slot.lock().unwrap() {
        if old == &key {
            return Ok(values.clone());
        }
    }
    let mut out = Vec::new();
    for (file, symbol, name) in [
        ("DV_Blocks.json", "B", "blocks"),
        ("DV_Items.json", "I", "items"),
        ("DV_Entities.json", "M", "entities"),
        ("DV_Enchantments.json", "E", "enchantments"),
    ] {
        let path = context.root.join("derived/enumerator").join(file);
        if path.is_file() {
            let values: Vec<String> = serde_json::from_reader(std::fs::File::open(path)?)?;
            out.push(Collection {
                id: format!("base:{name}"),
                name: name.into(),
                symbol: symbol.into(),
                category: "base".into(),
                version: String::new(),
                values: values.iter().map(|v| normalized(v)).collect(),
            });
        }
    }
    for (index, (category, path)) in files.into_iter().enumerate() {
        let value: Value = serde_json::from_reader(std::fs::File::open(&path)?)?;
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let values = value
            .as_array()
            .or_else(|| value["values"].as_array())
            .context("Enumerator collection has no values")?
            .iter()
            .filter_map(Value::as_str)
            .map(normalized)
            .collect();
        out.push(Collection {
            id: value["id"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| format!("{category}:{stem}")),
            name: value["name"].as_str().unwrap_or(&stem).into(),
            symbol: value["symbol"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| {
                    format!("{}{index}", category.chars().next().unwrap().to_uppercase())
                }),
            version: value["version"].as_str().unwrap_or(&stem).into(),
            category,
            values,
        });
    }
    let out = Arc::new(out);
    *slot.lock().unwrap() = Some((key, out.clone()));
    Ok(out)
}
fn precedence(op: &str) -> u8 {
    match op {
        "&" | "∩" => 2,
        "|" | "∪" | "-" => 1,
        _ => 0,
    }
}
fn evaluate(text: &str, collections: &[Collection]) -> Result<BTreeSet<String>> {
    let mut tokens = Vec::new();
    let mut buf = String::new();
    for c in text.chars() {
        if "()∪|∩&-".contains(c) {
            if !buf.trim().is_empty() {
                tokens.push(buf.trim().to_owned());
            }
            buf.clear();
            tokens.push(c.to_string());
        } else {
            buf.push(c);
        }
    }
    if !buf.trim().is_empty() {
        tokens.push(buf.trim().into());
    }
    let mut rpn = Vec::new();
    let mut ops = Vec::<String>::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.contains('*') {
            ensure!(
                index > 0
                    && tokens[index - 1] == "("
                    && tokens.get(index + 1).is_some_and(|x| x == ")"),
                "Wildcard collections must be parenthesized"
            );
        }
        match token.as_str() {
            "(" => ops.push(token.clone()),
            ")" => {
                while ops.last().is_some_and(|x| x != "(") {
                    rpn.push(ops.pop().unwrap());
                }
                ensure!(ops.pop().as_deref() == Some("("), "Unmatched parentheses");
            }
            _ if precedence(token) > 0 => {
                while ops
                    .last()
                    .is_some_and(|x| precedence(x) >= precedence(token))
                {
                    rpn.push(ops.pop().unwrap());
                }
                ops.push(token.clone());
            }
            _ => rpn.push(token.clone()),
        }
    }
    while let Some(op) = ops.pop() {
        ensure!(op != "(", "Unmatched parentheses");
        rpn.push(op);
    }
    let mut stack = Vec::<BTreeSet<String>>::new();
    for token in rpn {
        if precedence(&token) > 0 {
            let right = stack.pop().context("Incomplete collection expression")?;
            let left = stack.pop().context("Incomplete collection expression")?;
            stack.push(match token.as_str() {
                "&" | "∩" => left.intersection(&right).cloned().collect(),
                "-" => left.difference(&right).cloned().collect(),
                _ => left.union(&right).cloned().collect(),
            });
        } else {
            let key = token.to_lowercase();
            let found = if key.contains('*') {
                ensure!(
                    !key.trim_matches('*').contains('*'),
                    "Only edge wildcards are supported"
                );
                let prefix = key.split('*').next().unwrap();
                let suffix = key.rsplit('*').next().unwrap();
                collections
                    .iter()
                    .filter(|c| {
                        let n = c.name.to_lowercase();
                        n.starts_with(prefix) && n.ends_with(suffix)
                    })
                    .collect::<Vec<_>>()
            } else {
                collections
                    .iter()
                    .filter(|c| {
                        [&c.id, &c.name, &c.symbol]
                            .iter()
                            .any(|s| s.to_lowercase() == key)
                    })
                    .take(1)
                    .collect()
            };
            ensure!(!found.is_empty(), "Unknown collection: {token}");
            stack.push(
                found
                    .into_iter()
                    .flat_map(|c| c.values.iter().cloned())
                    .collect(),
            );
        }
    }
    ensure!(stack.len() == 1, "Incomplete collection expression");
    Ok(stack.pop().unwrap())
}
fn version_key(value: &str) -> Option<Vec<u32>> {
    if value == "v_older" {
        return Some(vec![1, 12, 2]);
    }
    value
        .strip_prefix("v_")
        .map(|v| {
            v.split('_')
                .map(str::parse)
                .collect::<std::result::Result<Vec<_>, _>>()
        })
        .transpose()
        .ok()
        .flatten()
}
pub fn query(stats: &Value, input: &Value) -> Result<Value> {
    let base = collections()?;
    let mut sets = base.as_ref().clone();
    let counts = stats["structural_block_counts"]
        .as_array()
        .context("Statistics have no structural counts")?
        .iter()
        .filter_map(|v| Some((v["block_id"].as_str()?.to_owned(), v["count"].as_u64()?)))
        .collect::<BTreeMap<_, _>>();
    sets.push(Collection {
        id: "runtime:projection".into(),
        name: "projection".into(),
        symbol: "L".into(),
        category: "runtime".into(),
        version: String::new(),
        values: counts.keys().cloned().collect(),
    });
    let zh = input["uiLocale"]
        .as_str()
        .unwrap_or("zh-CN")
        .starts_with("zh");
    let rules = input["rules"]
        .as_array()
        .context("Statistics rules are required")?;
    ensure!(rules.len() <= 128, "Too many statistics rules");
    let mut has_errors = false;
    let results = rules
        .iter()
        .map(|raw| {
            let raw = raw.as_str().unwrap_or("");
            let (title, expression) = raw.split_once('=').unwrap_or((raw, ""));
            let mut title = title.trim().to_owned();
            if raw == "理论版本 = version(L)" {
                title = if zh {
                    "理论版本"
                } else {
                    "Theoretical version"
                }
                .into();
            }
            if raw == "生存不可达 = any(L & survival_impossible)" {
                title = if zh {
                    "生存不可达"
                } else {
                    "Survival inaccessible"
                }
                .into();
            }
            let result = (|| -> Result<String> {
                let start = expression
                    .find('(')
                    .context("Use function(collection expression)")?;
                ensure!(expression.trim_end().ends_with(')'), "Incomplete function");
                let name = expression[..start].trim().to_lowercase();
                let argument = &expression[start + 1..expression.trim_end().len() - 1];
                let values = evaluate(argument, &sets)?;
                match name.as_str() {
                    "any" => Ok(if values.is_empty() {
                        if zh { "否" } else { "No" }
                    } else if zh {
                        "是"
                    } else {
                        "Yes"
                    }
                    .into()),
                    "ratio" => {
                        let total = stats["structure_stats"]["total_non_air_blocks"]
                            .as_u64()
                            .unwrap_or(0);
                        Ok(if total == 0 {
                            "--".into()
                        } else {
                            format!(
                                "{:.1}%",
                                values
                                    .iter()
                                    .map(|id| counts.get(id).copied().unwrap_or(0))
                                    .sum::<u64>() as f64
                                    / total as f64
                                    * 100.
                            )
                        })
                    }
                    "version" => {
                        if values.is_empty() {
                            return Ok("--".into());
                        }
                        let mut found = sets
                            .iter()
                            .filter(|c| c.category == "version" && !c.values.is_disjoint(&values))
                            .filter_map(|c| version_key(&c.version).map(|v| (v, c)))
                            .collect::<Vec<_>>();
                        found.sort_by(|a, b| a.0.cmp(&b.0));
                        Ok(found
                            .last()
                            .map(|(_, c)| {
                                if c.version == "v_older" {
                                    if zh {
                                        "1.12.2 及以前".into()
                                    } else {
                                        "1.12.2 or earlier".into()
                                    }
                                } else {
                                    c.version.trim_start_matches("v_").replace('_', ".")
                                }
                            })
                            .unwrap_or_else(|| {
                                if zh {
                                    "高于现有版本集合".into()
                                } else {
                                    "Newer than available version sets".into()
                                }
                            }))
                    }
                    _ => bail!("Unsupported statistics function: {name}"),
                }
            })();
            let (value, error) = match result {
                Ok(value) => (value, String::new()),
                Err(error) => {
                    has_errors = true;
                    (String::new(), format!("{error:#}"))
                }
            };
            json!({"title":title,"value":value,"error":error,"raw":raw})
        })
        .collect::<Vec<_>>();
    Ok(json!({"results":results,"hasErrors":has_errors,"rules":rules}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn set_rules_preserve_precedence_and_fail_missing() {
        let sets = vec![
            Collection {
                id: "a".into(),
                name: "a".into(),
                symbol: "A".into(),
                category: String::new(),
                version: String::new(),
                values: ["x", "y"].into_iter().map(String::from).collect(),
            },
            Collection {
                id: "b".into(),
                name: "b".into(),
                symbol: "B".into(),
                category: String::new(),
                version: String::new(),
                values: ["y"].into_iter().map(String::from).collect(),
            },
        ];
        assert_eq!(
            evaluate("A - B", &sets).unwrap(),
            ["x".into()].into_iter().collect()
        );
        assert!(evaluate("missing", &sets).is_err());
    }
}
