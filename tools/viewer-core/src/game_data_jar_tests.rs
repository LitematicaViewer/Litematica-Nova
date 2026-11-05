use super::*;

#[test]
fn jar_wins_over_loose_and_legacy_only_entries_remain_readable() -> Result<()> {
    let dir = staging(&std::env::temp_dir())?;
    let root = dir.0.canonicalize()?;
    let path = "assets/mod/Lang.json";
    let legacy = "assets/mod/langs/legacy.json";
    let missing = "assets/mod/missing.json";
    let mut index = BTreeMap::new();
    let mut jar = new_resource_jar(&root)?;
    add_resource(&mut jar, path, &b"canonical"[..], &mut index)?;
    jar.finish()?.flush()?;
    fs::create_dir_all(root.join("assets/mod"))?;
    fs::write(root.join(path), b"stale")?;
    record_file(&root, legacy, &b"legacy"[..], &mut index)?;
    index.insert(missing.into(), index[legacy].clone());
    atomic_json(&root.join("index.json"), &index)?;
    let context = GameDataContext { schema_version: SCHEMA, id: "test".into(), content_id: "1".repeat(64),
        game_version: "test-version".into(), resource_jar: root.join(RESOURCE_JAR), root, source: "test".into() };
    assert_eq!(read_resource_from(&context, path)?.unwrap(), b"canonical");
    assert_eq!(read_resource_from(&context, legacy)?.unwrap(), b"legacy");
    assert!(read_resource_from(&context, "assets/mod/lang.json")?.is_none());
    assert_eq!(list_resources_from(&context, "assets/mod/langs/")?, [legacy]);
    let error = read_resource_from(&context, missing).unwrap_err().to_string();
    assert!(error.contains("test-version") && error.contains(missing));
    Ok(())
}

pub(crate) fn check_candidate(consumer: impl FnOnce(&GameDataContext) -> Result<()>) -> Result<()> {
    let root = PathBuf::from(std::env::var_os("NOVA_TEST_SLIM_DEFAULT").context("NOVA_TEST_SLIM_DEFAULT")?);
    let old = PathBuf::from(std::env::var_os("NOVA_TEST_LOOSE_DEFAULT").context("NOVA_TEST_LOOSE_DEFAULT")?);
    let cache = PathBuf::from(std::env::var_os("LBA_DATA_ROOT").context("fresh isolated LBA_DATA_ROOT")?);
    ensure!(!cache.join("cache/item-names").exists() && !cache.join("cache/recipes").exists(), "test requires a cold, isolated cache");
    let context = package_context(&root)?;
    let _bound = OperationContext(OPERATION_CONTEXT.with(|slot| slot.replace(Some(context.clone()))));
    let source = open_build_jar(&old)?;
    let expected: BTreeSet<_> = source.file_names().map(str::to_owned).collect();
    let actual: BTreeSet<_> = list_resources_from(&context, "")?.into_iter().filter(|p| resource_path(p)).collect();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 21_494);
    assert!(!context.root.join("assets").exists() && !context.root.join("data").exists());
    let mut bytes = 0u64;
    for path in &actual {
        if !path.contains('/') { assert!(!context.root.join(path).exists(), "redundant loose {path}"); }
        let value = read_resource_from(&context, path)?.with_context(|| format!("missing: {path}"))?;
        ensure!(value == fs::read(old.join(path))?, "different bytes: {path}");
        bytes += value.len() as u64;
    }
    consumer(&context)?;
    println!("{} JAR-only resources: {} bytes equal; language, recipe, tag, inherited model, glass texture/alpha OK", actual.len(), bytes);
    Ok(())
}
