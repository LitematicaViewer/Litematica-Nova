// Interactive enumerator snapshots preserve the editor's value order and
// localized syntax diagnostics. Large collection payloads cross IPC once.
#[path = "../../../../tools/viewer-core/src/enumerator_syntax.rs"]
mod syntax;
use std::{collections::{HashMap, HashSet}, sync::{Arc, Mutex, OnceLock, atomic::{AtomicU64, Ordering}}};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub(crate) struct Collection { id: String, name: String, symbol: String, values: Vec<String> }
#[derive(Debug, Default, Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct View { values: Vec<String>, dependencies: Vec<String>, error_key: Option<&'static str>, token: Option<String> }

type Snapshots = HashMap<String, (u64, Arc<Vec<Collection>>)>;
fn snapshots() -> &'static Mutex<Snapshots> {
    static DATA: OnceLock<Mutex<Snapshots>> = OnceLock::new();
    DATA.get_or_init(Default::default)
}
pub(crate) fn bind(window: &str, collections: Vec<Collection>) -> Result<u64, String> {
    if !matches!(window, "main" | "enumerator") { return Err("Unexpected enumerator window".into()); }
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let id = SERIAL.fetch_add(1, Ordering::Relaxed);
    snapshots().lock().map_err(|_| "Enumerator snapshot lock failed")?
        .insert(window.to_owned(), (id, Arc::new(collections)));
    Ok(id)
}
pub(crate) fn forget(window: &str) { if let Ok(mut entries) = snapshots().lock() { entries.remove(window); } }
pub(crate) fn query(window: &str, id: u64, text: &str) -> Result<View, String> {
    let collections = snapshots().lock().map_err(|_| "Enumerator snapshot lock failed")?
        .get(window).filter(|(current, _)| *current == id).map(|(_, v)| v.clone()).ok_or("Enumerator snapshot has changed")?;
    Ok(evaluate(text, &collections))
}

fn distinct<'a>(values: impl Iterator<Item=&'a str>) -> Vec<&'a str> {
    let mut seen=HashSet::new();
    values.filter(|v| seen.insert(*v)).collect()
}
fn evaluate(text: &str, collections: &[Collection]) -> View {
    let error = |key, token: Option<&str>| View { error_key: Some(key), token: token.map(str::to_owned), ..View::default() };
    let tokens=syntax::tokens(text);
    if tokens.is_empty() { return View::default(); }
    // The interactive editor reports malformed wildcard groups before parentheses.
    for i in 0..tokens.len() {
        if let Err(syntax::SyntaxError::WildcardGroup(token))=syntax::wildcard_group(&tokens,i) {
            return error("native.enumerator.wildcardGroup",Some(token));
        }
    }
    let Ok(rpn)=syntax::postfix(&tokens) else { return error("native.enumerator.parentheses",None) };
    let mut stack=Vec::<Vec<&str>>::new();
    let mut deps=Vec::new();
    for token in rpn {
        if syntax::precedence(token)>0 {
            let (Some(right),Some(left))=(stack.pop(),stack.pop()) else {return error("native.enumerator.incomplete",None)};
            if matches!(token,"∪"|"|") { stack.push(distinct(left.into_iter().chain(right))); }
            else {
                let right:HashSet<_>=right.into_iter().collect();
                stack.push(left.into_iter().filter(|v| right.contains(v) == (token!="-")).collect());
            }
            continue;
        }
        let key=token.trim().to_lowercase();
        let mut found=Vec::new();
        if let Some(first)=key.find('*') {
            let last=key.rfind('*').unwrap();
            if (first!=0 && first+1!=key.len()) || (last!=first && last+1!=key.len()) {
                return error("native.enumerator.wildcardEdge",Some(&key));
            }
            let prefix=&key[..first]; let suffix=&key[last+1..];
            found.extend(collections.iter().filter(|c| {
                let name=c.name.trim().to_lowercase();
                name.starts_with(prefix) && name.ends_with(suffix) && name.len()>=prefix.len()+suffix.len()
            }));
            if found.is_empty(){return error("native.enumerator.noMatch",Some(token));}
        } else if let Some(c)=collections.iter().find(|c| [&c.id,&c.name,&c.symbol].iter().any(|s|s.trim().to_lowercase()==key)) {
            found.push(c);
        } else {return error("native.enumerator.notFound",Some(token));}
        deps.extend(found.iter().map(|c|c.id.trim()).filter(|id|!id.is_empty()));
        stack.push(distinct(found.into_iter().flat_map(|c|c.values.iter().map(String::as_str))));
    }
    if stack.len()!=1 {return error("native.enumerator.incomplete",None);}
    View{values:stack.pop().unwrap().into_iter().map(str::to_owned).collect(),
        dependencies:distinct(deps.into_iter()).into_iter().map(str::to_owned).collect(),..View::default()}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn collection(id:&str,name:&str,symbol:&str,values:&[&str])->Collection {
        Collection{id:id.into(),name:name.into(),symbol:symbol.into(),values:values.iter().map(|s|s.to_string()).collect()}
    }
    #[test]
    fn enumerator_editor_preserves_order_wildcards_diagnostics_and_snapshot_isolation() {
        let data=vec![collection("a","颜色组一","A",&["z","a","z","c"]),collection("b","颜色组二","B",&["c","b"]),collection("c","Other","C",&["c"])];
        let out=evaluate("A | B & C",&data);assert_eq!(out.values,["z","a","c"]);assert_eq!(out.dependencies,["a","b","c"]);
        assert_eq!(evaluate("(颜色组*) - C",&data).values,["z","a","b"]);
        for (text,key,token) in [("颜色组*","wildcardGroup",Some("颜色组*")),("(颜色*组)","wildcardEdge",Some("颜色*组")),
            ("(absent*)","noMatch",Some("absent*")),("x","notFound",Some("x")),("(A","parentheses",None),("A|","incomplete",None)] {
            let result=evaluate(text,&data);assert_eq!(result.error_key.unwrap(),format!("native.enumerator.{key}"));assert_eq!(result.token.as_deref(),token);
        }
        let id=bind("main",data).unwrap();
        assert_eq!(query("main",id,"A").unwrap().values,["z","a","c"]);
        assert!(query("enumerator",id,"A").is_err());
        let next=bind("main",vec![]).unwrap();assert_ne!(id,next);assert!(query("main",id,"A").is_err());
    }
}
