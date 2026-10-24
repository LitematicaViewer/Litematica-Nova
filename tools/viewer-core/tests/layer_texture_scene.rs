//! Opt-in local production-path check; never starts a Viewer or changes a source.
use anyhow::{Context, Result, ensure};
use base64::Engine;
use litematica_core::{block_database, layer_resources::{self, LayerResourceContext, ResourceAccess}, layer_view::LayerService, nbt, visual};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::PathBuf, sync::atomic::AtomicBool};

#[test]
#[ignore = "set NOVA_LAYER_TEST_SOURCE and optional NOVA_LAYER_TEST_PACKS_JSON / NOVA_LAYER_TEST_OUTPUT"]
fn actual_palette_and_layer_pixels() -> Result<()> {
    let path=PathBuf::from(std::env::var_os("NOVA_LAYER_TEST_SOURCE").context("NOVA_LAYER_TEST_SOURCE missing")?);
    let paths:Vec<PathBuf>=serde_json::from_str(&std::env::var("NOVA_LAYER_TEST_PACKS_JSON").unwrap_or_else(|_|"[]".into()))?;
    let (root,_)=visual::load_visual_catalog(&path)?;
    let db=block_database::database()?;
    let version=db.version_for_data_version(root.minecraft_data_version.context("source data version missing")?).context("exact source version missing")?;
    let resources=LayerResourceContext::resolve(version,&paths)?;
    let _scope=layer_resources::enter(resources.clone());
    let states:BTreeSet<String>=root.regions.values().flat_map(|region|region.block_state_palette.iter()).filter(|block|!visual::is_air_state(&block.name)).map(visual::format_block_state).collect();
    let mut failed=Vec::new();let mut transparent=0;let mut success=0;let mut pixel_errors=Vec::new();
    for state in &states {
        match db.layer_top_sprite(version,state) {
            Ok(image)=>{ensure!(image.dimensions()==(32,32),"unexpected sprite extent for {state}");success+=1;if image.pixels().all(|pixel|pixel[3]==0){transparent+=1;}},
            Err(error)=>failed.push(json!({"state":state,"kind":error.kind,"resourceKey":error.resource_key,"message":error.message,"retryable":error.retryable})),
        }
    }
    let service=LayerService::default();let cancel=AtomicBool::new(false);
    let opened=service.execute(&json!({"action":"openScene","sourcePath":path,"resourcePacks":paths}),&cancel)?;
    let handle=opened["sceneHandle"].as_str().context("scene handle missing")?;
    let generation=opened["generation"].as_u64().context("scene generation missing")?;
    let mut layers=Vec::new();let mut trace=Value::Null;
    let height=root.regions.values().map(|region|nbt::region_bounds(region).max_y).max().unwrap_or(1);
    for (index,y) in [1,17,34,68].into_iter().filter(|y|*y<=height).enumerate() {
        let id=index as u64+1;
        let view=service.execute(&json!({"action":"requestLayerView","sceneHandle":handle,"generation":generation,"requestId":id,
            "resourcePacks":paths,"y":y,"onionDepth":0,"viewRect":{"x":0,"z":0,"width":80,"height":76},"scale":4.0,"stateHints":true}),&cancel)?;
        let png=base64::engine::general_purpose::STANDARD.decode(view["imageUrl"].as_str().context("layer PNG missing")?.split_once(',').context("layer PNG URL invalid")?.1)?;
        let image=image::load_from_memory_with_format(&png,image::ImageFormat::Png)?.to_rgba8();
        if view["blockCount"].as_u64()==Some(0) { ensure!(image.pixels().all(|pixel|pixel[3]==0),"empty layer must stay transparent"); }
        layers.push(json!({"y":y,"width":image.width(),"height":image.height(),"blockCount":view["blockCount"],"missingVisuals":view["missingVisuals"],"visualDiagnostics":view["visualDiagnostics"]}));
        if y==1 {
            let hit=service.execute(&json!({"action":"hitTest","sceneHandle":handle,"generation":generation,"requestId":id,"resourcePacks":paths,"y":y,"x":71,"z":32}),&cancel)?;
            let state=hit["hit"]["states"].as_str().unwrap_or("minecraft:white_stained_glass");
            ensure!(hit["hit"]["id"]=="minecraft:white_stained_glass","reported source coordinate does not point to white stained glass: {hit}");
            let top=db.layer_top_sprite(version,state).map_err(|error|anyhow::anyhow!("{}",error.message))?;
            let key="assets/minecraft/textures/block/white_stained_glass.png";
            let bytes=resources.read(key)?.context("white glass original texture missing")?;
            let texture=image::load_from_memory_with_format(&bytes,image::ImageFormat::Png)?.to_rgba8();
            let min_alpha=top.pixels().map(|pixel|pixel[3]).min().unwrap();let max_alpha=top.pixels().map(|pixel|pixel[3]).max().unwrap();
            ensure!(max_alpha>0 && min_alpha<255,"white glass must preserve real translucent alpha");
            let rendered=image.get_pixel(71*4+2,32*4+2).0;
            if rendered!=top.get_pixel(16,16).0 {pixel_errors.push(format!("glass pixel differs from true sprite: {rendered:?}"));}
            trace=json!({"coordinate":[71,1,32],"hit":hit["hit"],"version":version,"resourceIdentity":resources.identity(),
                "textureKey":key,"origin":resources.origin(key)?,"originalTextureSize":[texture.width(),texture.height()],
                "spriteAlpha":[min_alpha,max_alpha],"spriteCenter":top.get_pixel(16,16).0,"layerCenter":rendered,
                "sourceCenter":texture.get_pixel(texture.width()/2,texture.height()/2).0,"stateHints":true});
        }
    }
    let report=json!({"source":path.file_name().map(|name|name.to_string_lossy()),"version":version,"paletteTotal":states.len(),"success":success,"transparent":transparent,"failures":failed,"pixelErrors":pixel_errors,"layers":layers,"trace":trace});
    if let Some(output)=std::env::var_os("NOVA_LAYER_TEST_OUTPUT"){std::fs::write(output,serde_json::to_vec_pretty(&report)?)?;}
    println!("LAYER_PALETTE_RESULT {}",serde_json::to_string(&report)?);
    ensure!(failed.is_empty(),"{} actual palette states unresolved; see complete collected result",failed.len());
    ensure!(pixel_errors.is_empty(),"layer pixel composition needs correction; see source/sprite/layer trace");
    Ok(())
}

#[test]
#[ignore = "local replay of the reported coordinate; same NOVA_LAYER_TEST_SOURCE/PACKS_JSON"]
fn actual_reported_glass_pixels() -> Result<()> {
    let source=PathBuf::from(std::env::var_os("NOVA_LAYER_TEST_SOURCE").context("source missing")?);
    let paths:Vec<PathBuf>=serde_json::from_str(&std::env::var("NOVA_LAYER_TEST_PACKS_JSON").unwrap_or_else(|_|"[]".into()))?;
    let service=LayerService::default();let cancel=AtomicBool::new(false);
    let scene=service.execute(&json!({"action":"openScene","sourcePath":source,"resourcePacks":paths}),&cancel)?;
    let identity=json!({"sceneHandle":scene["sceneHandle"],"generation":scene["generation"],"resourcePacks":paths,"requestId":1});
    let mut view_input=identity.clone();view_input["action"]="requestLayerView".into();view_input["y"]=1.into();view_input["scale"]=4.0.into();view_input["stateHints"]=true.into();view_input["viewRect"]=json!({"x":70,"z":31,"width":3,"height":3});
    let view=service.execute(&view_input,&cancel)?;
    let mut hit_input=identity.clone();hit_input["action"]="hitTest".into();hit_input["x"]=71.into();hit_input["y"]=1.into();hit_input["z"]=32.into();
    let hit=service.execute(&hit_input,&cancel)?;ensure!(hit["hit"]["id"]=="minecraft:white_stained_glass","reported coordinate identity changed");
    let version=hit["hit"]["minecraftVersion"].as_str().context("hit version missing")?;
    let resources=LayerResourceContext::resolve(version,&paths)?;let _scope=layer_resources::enter(resources.clone());
    let key="assets/minecraft/textures/block/white_stained_glass.png";
    let original=image::load_from_memory_with_format(&resources.read(key)?.context("original PNG missing")?,image::ImageFormat::Png)?.to_rgba8();
    let sprite=block_database::database()?.layer_top_sprite(version,hit["hit"]["states"].as_str().unwrap())?;
    let png=base64::engine::general_purpose::STANDARD.decode(view["imageUrl"].as_str().unwrap().split_once(',').unwrap().1)?;
    let image=image::load_from_memory_with_format(&png,image::ImageFormat::Png)?.to_rgba8();
    ensure!(image.get_pixel(6,6)==sprite.get_pixel(16,16),"layer pixel {:?} != real sprite {:?}",image.get_pixel(6,6),sprite.get_pixel(16,16));
    ensure!(image.get_pixel(6,6)[3]>0&&image.get_pixel(6,6)[3]<255,"actual glass alpha was not preserved");
    println!("ACTUAL_GLASS_REPLAY {}",json!({"version":version,"coordinate":[71,1,32],"origin":resources.origin(key)?,"sourceCenter":original.get_pixel(original.width()/2,original.height()/2).0,"spriteCenter":sprite.get_pixel(16,16).0,"layerCenter":image.get_pixel(6,6).0,"missingVisuals":view["missingVisuals"]}));Ok(())
}

#[test]
#[ignore = "additional existing projection palette; no Viewer or GPU"]
fn actual_additional_palette_assets() -> Result<()> {
    let source=PathBuf::from(std::env::var_os("NOVA_LAYER_TEST_SOURCE").context("source missing")?);
    let paths:Vec<PathBuf>=serde_json::from_str(&std::env::var("NOVA_LAYER_TEST_PACKS_JSON").unwrap_or_else(|_|"[]".into()))?;
    let(root,_)=visual::load_visual_catalog(&source)?;let db=block_database::database()?;
    let version=db.version_for_data_version(root.minecraft_data_version.context("source version missing")?).context("exact source version missing")?;
    let context=LayerResourceContext::resolve(version,&paths)?;let _scope=layer_resources::enter(context);
    let states:BTreeSet<String>=root.regions.values().flat_map(|region|region.block_state_palette.iter()).filter(|block|!visual::is_air_state(&block.name)).map(visual::format_block_state).collect();
    let mut failures=Vec::new();let mut transparent=0;
    for state in &states{match db.layer_top_sprite(version,state){Ok(image)=>{if image.pixels().all(|pixel|pixel[3]==0){transparent+=1}},Err(error)=>failures.push(json!({"state":state,"kind":error.kind,"key":error.resource_key,"message":error.message}))}}
    println!("ADDITIONAL_LAYER_PALETTE {}",json!({"source":source.file_name().unwrap().to_string_lossy(),"version":version,"states":states.len(),"success":states.len()-failures.len(),"transparent":transparent,"failures":failures}));
    ensure!(failures.is_empty(),"additional palette contains {} unresolved states",failures.len());Ok(())
}
