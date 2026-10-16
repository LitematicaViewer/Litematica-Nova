//! Item payload bridges follow the local official ItemStackComponentizationFix
//! (3818.5), ItemStackEnchantmentNamesFix (1494), AttributeModifierIdFix (3945)
//! and TooltipDisplayComponentFix (4307). Unknown fields remain an explicit
//! migration error: they are not discarded or hidden inside custom_data.
use super::*;

// Exact DAMAGE_IDS from ItemStackTheFlatteningFix; block/item metadata is not
// durability. In particular, colored wool and potion variants must not enter it.
const DAMAGE_ITEMS: &[&str] = &[
    "bow", "carrot_on_a_stick", "chainmail_boots", "chainmail_chestplate", "chainmail_helmet", "chainmail_leggings",
    "diamond_axe", "diamond_boots", "diamond_chestplate", "diamond_helmet", "diamond_hoe", "diamond_leggings", "diamond_pickaxe", "diamond_shovel", "diamond_sword",
    "elytra", "fishing_rod", "flint_and_steel", "golden_axe", "golden_boots", "golden_chestplate", "golden_helmet", "golden_hoe", "golden_leggings", "golden_pickaxe", "golden_shovel", "golden_sword",
    "iron_axe", "iron_boots", "iron_chestplate", "iron_helmet", "iron_hoe", "iron_leggings", "iron_pickaxe", "iron_shovel", "iron_sword",
    "leather_boots", "leather_chestplate", "leather_helmet", "leather_leggings", "shears", "shield", "stone_axe", "stone_hoe", "stone_pickaxe", "stone_shovel", "stone_sword",
    "wooden_axe", "wooden_hoe", "wooden_pickaxe", "wooden_shovel", "wooden_sword",
];
const ENCHANTMENTS: &[(i16, &str)] = &[
    (0,"protection"),(1,"fire_protection"),(2,"feather_falling"),(3,"blast_protection"),(4,"projectile_protection"),
    (5,"respiration"),(6,"aqua_affinity"),(7,"thorns"),(8,"depth_strider"),(9,"frost_walker"),(10,"binding_curse"),
    (16,"sharpness"),(17,"smite"),(18,"bane_of_arthropods"),(19,"knockback"),(20,"fire_aspect"),(21,"looting"),(22,"sweeping_edge"),
    (32,"efficiency"),(33,"silk_touch"),(34,"unbreaking"),(35,"fortune"),(48,"power"),(49,"punch"),(50,"flame"),(51,"infinity"),
    (61,"luck_of_the_sea"),(62,"lure"),(65,"loyalty"),(66,"impaling"),(67,"riptide"),(68,"channeling"),(70,"mending"),(71,"vanishing_curse"),
];
const MAP_TYPES: &[&str] = &["player","frame","red_marker","blue_marker","target_x","target_point","player_off_map","player_off_limits","mansion","monument",
    "banner_white","banner_orange","banner_magenta","banner_light_blue","banner_yellow","banner_lime","banner_pink","banner_gray","banner_light_gray","banner_cyan","banner_purple","banner_blue","banner_brown","banner_green","banner_red","banner_black","red_x","village_desert","village_plains","village_savanna","village_snowy","village_taiga","jungle_temple","swamp_hut"];

fn only(fields: &Fields, names: &[&str], context: &str) -> Result<()> {
    if let Some(key) = fields.keys().find(|key| !names.contains(&key.as_str())) { bail!("{context}含目标未定义字段 {key}，原数据保留") }
    Ok(())
}
fn number(value: &Value) -> Result<i32> { match value { Value::Byte(v)=>Ok(i32::from(*v)),Value::Short(v)=>Ok(i32::from(*v)),Value::Int(v)=>Ok(*v),_=>bail!("字段应为整数") } }
fn text(value: &Value) -> Result<&str> { match value { Value::String(v)=>Ok(v),_=>bail!("字段应为字符串") } }
fn boolean(value: &Value) -> Result<bool> { match value { Value::Byte(0)=>Ok(false),Value::Byte(1)=>Ok(true),_=>bail!("布尔标签必须为Byte 0/1") } }
fn list(value: Value) -> Result<Vec<Value>> { match value { Value::List(v)=>Ok(v),_=>bail!("字段应为列表") } }
fn insert(fields:&mut Fields,key:&str,value:Value)->Result<()> { if fields.insert(key.into(),value).is_some(){bail!("物品字段 {key} 存在两个来源，拒绝静默覆盖")};Ok(()) }
fn tooltip(fields:&mut Fields,hidden:bool){if hidden {fields.insert("show_in_tooltip".into(),Value::Byte(0));}}
fn item_is(id:&str,name:&str)->bool{id.strip_prefix("minecraft:").unwrap_or(id)==name}

pub(super) fn prepare_legacy_stack(fields:&mut Fields,source:i32,_target:i32,_report:&mut PayloadMigrationReport)->Result<()> {
    if source>=1519{return Ok(())}
    let id=fields.get("id").map(text).transpose()?.unwrap_or("");
    let key=if DAMAGE_ITEMS.contains(&id.strip_prefix("minecraft:").unwrap_or(id)){"Damage"}else if item_is(id,"filled_map"){"map"}else{return Ok(())};
    let damage=match fields.get("Damage"){Some(Value::Short(v))=>i32::from(*v),None=>0,_=>bail!("旧物品Damage必须为Short")};
    if damage<0{bail!("旧物品{key}={damage}不在可等价表示的非负范围")}
    let tag=fields.entry("tag".into()).or_insert_with(||Value::Compound(Fields::new()));
    let tag=object(tag)?;
    if tag.get(key).is_some_and(|value|value!=&Value::Int(damage)){bail!("旧物品顶层Damage与tag/{key}冲突")}
    tag.insert(key.into(),Value::Int(damage));
    fields.insert("Damage".into(),Value::Short(0));
    Ok(())
}
pub(super) fn finish_legacy_stack(fields:&mut Fields,_source:i32,target:i32,_report:&mut PayloadMigrationReport)->Result<()> {
    if target>=1519{return Ok(())}
    let id=fields.get("id").map(text).transpose()?.unwrap_or("");
    let key=if DAMAGE_ITEMS.contains(&id.strip_prefix("minecraft:").unwrap_or(id)){"Damage"}else if item_is(id,"filled_map"){"map"}else{return Ok(())};
    let Some(tag)=fields.get_mut("tag")else{return Ok(())};
    let tag=object(tag)?;
    let damage=match tag.remove(key){Some(value)=>number(&value)?,None=>0};
    let damage=i16::try_from(damage).ok().filter(|v|*v>=0).context("目标旧版Short Damage不能无损保存当前耐久/地图编号")?;
    let empty=tag.is_empty();
    if empty{fields.remove("tag");}
    fields.insert("Damage".into(),Value::Short(damage));
    Ok(())
}

fn enchantment_name(value:&Value,source:i32)->Result<String>{
    let name=match value {
        Value::Short(id) if source<1494=>ENCHANTMENTS.iter().find(|(n,_)|n==id).map(|(_,name)|(*name).to_owned()).context("旧版数字附魔ID不在原版注册表")?,
        Value::Int(id) if source<1494=>ENCHANTMENTS.iter().find(|(n,_)|i32::from(*n)==*id).map(|(_,name)|(*name).to_owned()).context("旧版数字附魔ID不在原版注册表")?,
        Value::String(name)=>name.strip_prefix("minecraft:").unwrap_or(name).to_owned(),
        _=>bail!("附魔ID类型与来源版本不符"),
    };
    let name=if source<3803&&name=="sweeping"{"sweeping_edge"}else{&name};
    if !ENCHANTMENTS.iter().any(|(_,known)|*known==name)&&!["soul_speed","swift_sneak","density","breach","wind_burst"].contains(&name){bail!("附魔 {name} 不在已核实原版注册表")}
    Ok(format!("minecraft:{name}"))
}
fn enchantment_for_target(id:&str,target:i32)->Result<Value>{
    let name=id.strip_prefix("minecraft:").context("附魔必须有minecraft注册表身份")?;
    let introduced=match name{"soul_speed"=>2529,"swift_sneak"=>3105,"density"|"breach"|"wind_burst"=>3953,"loyalty"|"impaling"|"riptide"|"channeling"=>1519,_=>0};
    if target<introduced{bail!("目标DataVersion {target}没有原版附魔 {id}（首次正式版本DataVersion {introduced}），保留原数据")}
    if target<1494{
        let numeric=ENCHANTMENTS.iter().find(|(_,known)|*known==name).map(|(id,_)|*id).context("目标旧版没有此附魔的数字注册ID")?;
        Ok(Value::Short(numeric))
    }else{Ok(Value::String(if target<3803&&name=="sweeping_edge"{"minecraft:sweeping".into()}else{id.into()}))}
}
fn legacy_enchantments(value:Value,source:i32,hidden:bool)->Result<Value>{
    let mut levels=Fields::new();
    for mut entry in list(value)?{
        let entry=object(&mut entry)?;only(entry,&["id","lvl"],"附魔条目")?;
        let id=enchantment_name(entry.get("id").context("附魔缺少id")?,source)?;
        let level=number(entry.get("lvl").context("附魔缺少lvl")?)?;
        if !(1..=255).contains(&level){bail!("目标组件不能无损保存附魔等级 {level}（合法范围1..255）")}
        insert(&mut levels,&id,Value::Int(level))?;
    }
    let mut component=Fields::from([("levels".into(),Value::Compound(levels))]);tooltip(&mut component,hidden);Ok(Value::Compound(component))
}

pub(super) fn legacy_to_components(tag:&mut Fields,components:&mut Fields,source:i32,item_id:&str,report:&mut PayloadMigrationReport)->Result<()> {
    let flags=tag.get("HideFlags").map(number).transpose()?.unwrap_or(0);
    if flags&!255!=0{bail!("HideFlags包含未定义位，原数据保留")}
    for (old,new,bit) in [("ench","minecraft:enchantments",1),("Enchantments","minecraft:enchantments",1),("StoredEnchantments","minecraft:stored_enchantments",32)]{
        if let Some(value)=tag.remove(old){insert(components,new,legacy_enchantments(value,source,flags&bit!=0)?)?;}
    }
    for (old,new,bit) in [("CanDestroy","minecraft:can_break",8),("CanPlaceOn","minecraft:can_place_on",16)]{
        if let Some(value)=tag.remove(old){
            let predicates=list(value)?.iter().map(|value|legacy_predicate(text(value)?,source)).collect::<Result<Vec<_>>>()?;
            let mut component=Fields::from([("predicates".into(),Value::List(predicates))]);tooltip(&mut component,flags&bit!=0);
            insert(components,new,Value::Compound(component))?;
        }
    }
    if let Some(value)=tag.remove("AttributeModifiers"){
        let mut modifiers=Vec::new();
        for mut entry in list(value)?{
            let entry=object(&mut entry)?;only(entry,&["AttributeName","Name","UUID","UUIDMost","UUIDLeast","Amount","Operation","Slot"],"旧属性修饰器")?;
            let ty=entry.remove("AttributeName").context("属性修饰器缺少AttributeName")?;
            let slot=entry.remove("Slot");
            entry.entry("Name".into()).or_insert_with(||Value::String(String::new()));
            entry.entry("Amount".into()).or_insert(Value::Double(0.0));
            entry.entry("Operation".into()).or_insert(Value::Int(0));
            let mut entity=Fields::from([("Attributes".into(),Value::List(vec![Value::Compound(Fields::from([
                ("Name".into(),ty),("Modifiers".into(),Value::List(vec![Value::Compound(entry.clone())]))
            ]))]))]);
            migrate_entity_attributes(&mut entity,source,3818)?;
            let Value::List(mut attributes)=entity.remove("Attributes").unwrap()else{unreachable!()};
            let attribute=object(&mut attributes[0])?;
            let ty=attribute.remove("Name").context("属性类型丢失")?;
            let Value::List(mut values)=attribute.remove("Modifiers").unwrap()else{unreachable!()};
            let mut output=object(&mut values[0])?.clone();
            for(old,new)in[("UUID","uuid"),("Name","name"),("Amount","amount"),("Operation","operation")]{if let Some(value)=output.remove(old){output.insert(new.into(),value);}}
            let operation=number(output.get("operation").context("属性Operation缺失")?)?;
            output.insert("operation".into(),Value::String(operation_name(operation)?.into()));
            output.insert("type".into(),ty);if let Some(slot)=slot{output.insert("slot".into(),slot);}
            validate_modifier(&output,3818)?;modifiers.push(Value::Compound(output));
        }
        let mut component=Fields::from([("modifiers".into(),Value::List(modifiers))]);tooltip(&mut component,flags&2!=0);
        insert(components,"minecraft:attribute_modifiers",Value::Compound(component))?;
    }
    if let Some(mut trim)=tag.remove("Trim"){validate_trim(&trim)?;tooltip(object(&mut trim)?,flags&128!=0);insert(components,"minecraft:trim",trim)?;}
    for(bit,key)in[(2,"minecraft:attribute_modifiers"),(8,"minecraft:can_break"),(16,"minecraft:can_place_on"),(128,"minecraft:trim")]{
        if flags&bit!=0&&!components.contains_key(key){bail!("HideFlags位{bit}没有显式{key}载荷，目标默认属性的隐藏效果不能从空组件等价恢复")}
    }
    if flags&32!=0{insert(components,"minecraft:hide_additional_tooltip",Value::Compound(Fields::new()))?;}
    if let Some(mut owner)=tag.remove("SkullOwner"){migrate_profile(&mut owner,source,3818,3818)?;insert(components,"minecraft:profile",owner)?;}
    if let Some(value)=tag.remove("map"){let id=number(&value)?;if id<0{bail!("地图编号不能为负数")};insert(components,"minecraft:map_id",Value::Int(id))?;}
    if let Some(value)=tag.remove("Decorations"){insert(components,"minecraft:map_decorations",legacy_map_decorations(value)?)?;}
    if let Some(display)=tag.get_mut("display"){
        if let Some(value)=object(display)?.remove("MapColor"){if !matches!(value,Value::Int(_)){bail!("MapColor应为Int")};insert(components,"minecraft:map_color",value)?;}
    }
    if let Some(mut explosion)=tag.remove("Explosion"){firework_explosion(&mut explosion,true)?;insert(components,"minecraft:firework_explosion",explosion)?;}
    if let Some(mut fireworks)=tag.remove("Fireworks"){
        let fields=object(&mut fireworks)?;only(fields,&["Flight","Explosions"],"烟花火箭")?;
        let mut output=Fields::new();
        if let Some(flight)=fields.remove("Flight"){let n=number(&flight)?;if !(0..=255).contains(&n){bail!("烟花飞行时长不在0..255范围")};output.insert("flight_duration".into(),Value::Int(n));}
        if let Some(explosions)=fields.remove("Explosions"){let mut explosions=list(explosions)?;for explosion in &mut explosions{firework_explosion(explosion,true)?;}output.insert("explosions".into(),Value::List(explosions));}
        insert(components,"minecraft:fireworks",Value::Compound(output))?;
    }
    for(old,new)in[("ChargedProjectiles","minecraft:charged_projectiles"),("Items","minecraft:bundle_contents")]{
        if old=="Items"&&!item_is(item_id,"bundle"){continue}
        if let Some(value)=tag.remove(old){let mut items=list(value)?;for item in &mut items{item_payload(item,source,3818,report)?;}insert(components,new,Value::List(items))?;}
    }
    if let Some(charged)=tag.remove("Charged"){
        let charged=boolean(&charged)?;
        let has_projectiles=matches!(components.get("minecraft:charged_projectiles"),Some(Value::List(items))if !items.is_empty());
        if charged!=has_projectiles{bail!("Charged标记与实际弹药列表冲突，目标仅按弹药判定上弦，不能无损迁移")}
    }
    if let Some(value)=tag.remove("BlockStateTag"){insert(components,"minecraft:block_state",legacy_block_state(value)?)?;}
    if let Some(mut value)=tag.remove("EntityTag"){
        migrate_entity(&mut value,source,3818,report)?;insert(components,"minecraft:entity_data",value)?;
    }
    for(old,new)in[("instrument","minecraft:instrument"),("Recipes","minecraft:recipes"),("DebugProperty","minecraft:debug_stick_state")]{
        if let Some(mut value)=tag.remove(old){migrate_component(new,&mut value,3818,3818,report,&mut Vec::new())?;insert(components,new,value)?;}
    }
    let pos=tag.remove("LodestonePos");let dimension=tag.remove("LodestoneDimension");let tracked=tag.remove("LodestoneTracked");
    if pos.is_some()||dimension.is_some()||tracked.is_some(){
        let mut tracker=Fields::new();
        match(pos,dimension){(Some(pos),Some(dimension))=>{tracker.insert("target".into(),Value::Compound(Fields::from([("pos".into(),pos),("dimension".into(),dimension)])));},(None,None)=>{},_=>bail!("磁石坐标/维度只有一项，无法等价表示")}
        if let Some(tracked)=tracked{boolean(&tracked)?;tracker.insert("tracked".into(),tracked);}
        insert(components,"minecraft:lodestone_tracker",Value::Compound(tracker))?;
    }
    Ok(())
}

fn operation_name(operation:i32)->Result<&'static str>{match operation{0=>Ok("add_value"),1=>Ok("add_multiplied_base"),2=>Ok("add_multiplied_total"),_=>bail!("属性Operation={operation}不是原版0/1/2")}}
fn operation_number(operation:&str)->Result<i32>{match operation{"add_value"=>Ok(0),"add_multiplied_base"=>Ok(1),"add_multiplied_total"=>Ok(2),_=>bail!("属性operation={operation}无旧版等价值")}}
fn validate_modifier(fields:&Fields,version:i32)->Result<()> {
    only(fields,if version<3945{&["type","name","uuid","amount","operation","slot"]}else{&["type","id","amount","operation","slot","display"]},"属性修饰器")?;
    text(fields.get("type").context("属性修饰器缺少type")?)?;
    match fields.get("amount"){Some(Value::Double(v))if v.is_finite()=>{},_=>bail!("属性amount应为有限Double")}
    operation_number(text(fields.get("operation").context("属性修饰器缺少operation")?)?)?;
    if version<3945{uuid_text(fields.get("uuid").context("属性修饰器缺少uuid")?)?;text(fields.get("name").context("属性修饰器缺少name")?)?;}else{text(fields.get("id").context("属性修饰器缺少id")?)?;}
    if let Some(slot)=fields.get("slot"){if !["any","mainhand","offhand","hand","feet","legs","chest","head","armor","body","saddle"].contains(&text(slot)?){bail!("属性slot不是原版装备槽组")}}
    Ok(())
}
fn migrate_attribute_entries(entries:&mut Vec<Value>,source:i32,target:i32)->Result<()> {
    for entry in entries{
        let entry=object(entry)?;validate_modifier(entry,source)?;
        let slot=entry.remove("slot");let display=entry.remove("display");
        if display.is_some()&&target<4307{bail!("目标版本没有逐属性display规则，不能丢弃自定义属性提示")}
        let ty=entry.remove("type").unwrap();
        let mut modifier=entry.clone();
        if source<3945{for(old,new)in[("uuid","UUID"),("name","Name"),("amount","Amount")]{if let Some(value)=modifier.remove(old){modifier.insert(new.into(),value);}}
            let operation=operation_number(text(modifier.get("operation").unwrap())?)?;modifier.remove("operation");modifier.insert("Operation".into(),Value::Int(operation));}
        let mut carrier=Fields::from([(if source<3945{"Attributes"}else{"attributes"}.into(),Value::List(vec![Value::Compound(Fields::from([
            (if source<3945{"Name"}else{"id"}.into(),ty),(if source<3945{"Modifiers"}else{"modifiers"}.into(),Value::List(vec![Value::Compound(modifier)]))
        ]))]))]);
        migrate_entity_attributes(&mut carrier,source,target)?;
        let Value::List(mut attributes)=carrier.remove(if target<3945{"Attributes"}else{"attributes"}).unwrap()else{unreachable!()};
        let attribute=object(&mut attributes[0])?;
        let ty=attribute.remove(if target<3945{"Name"}else{"id"}).unwrap();
        let Value::List(mut modifiers)=attribute.remove(if target<3945{"Modifiers"}else{"modifiers"}).unwrap()else{unreachable!()};
        *entry=object(&mut modifiers[0])?.clone();
        if target<3945{for(old,new)in[("UUID","uuid"),("Name","name"),("Amount","amount")]{if let Some(value)=entry.remove(old){entry.insert(new.into(),value);}}
            let operation=number(&entry.remove("Operation").context("属性操作缺失")?)?;entry.insert("operation".into(),Value::String(operation_name(operation)?.into()));}
        entry.insert("type".into(),ty);if let Some(slot)=slot{entry.insert("slot".into(),slot);}if let Some(display)=display{entry.insert("display".into(),display);}
        validate_modifier(entry,target)?;
    }
    Ok(())
}
fn validate_trim(value:&Value)->Result<()> {
    let Value::Compound(fields)=value else{bail!("盔甲纹饰不是Compound")};only(fields,&["material","pattern","show_in_tooltip"],"盔甲纹饰")?;
    for key in ["material","pattern"]{text(fields.get(key).with_context(||format!("盔甲纹饰缺少{key}"))?)?;}
    if let Some(show)=fields.get("show_in_tooltip"){boolean(show)?;}Ok(())
}

fn legacy_predicate(input:&str,source:i32)->Result<Value>{
    let end=input.find(['[','{']).unwrap_or(input.len());let block=input[..end].trim();
    if block.is_empty(){bail!("冒险方块条件缺少方块/标签")}
    if source<1519&&input.contains(['[','{']){bail!("来源旧版冒险条件不支持状态/NBT，原数据保留")}
    let mut fields=Fields::from([("blocks".into(),Value::String(block.into()))]);
    let mut suffix=input[end..].trim();
    if suffix.starts_with('['){let end=suffix.find(']').context("冒险方块条件缺少]")?;let mut state=Fields::new();
        for property in suffix[1..end].split(',').filter(|part|!part.trim().is_empty()){
            let(key,value)=property.split_once('=').context("方块状态条件缺少=")?;insert(&mut state,key.trim(),Value::String(value.trim().into()))?;
        }fields.insert("state".into(),Value::Compound(state));suffix=suffix[end+1..].trim();}
    if !suffix.is_empty(){if !suffix.starts_with('{')||!suffix.ends_with('}'){bail!("冒险方块条件NBT不完整")};fields.insert("nbt".into(),Value::String(suffix.into()));}
    Ok(Value::Compound(fields))
}
fn predicate_strings(value:&Value,target:i32)->Result<Vec<Value>>{
    let Value::Compound(fields)=value else{bail!("冒险条件条目不是Compound")};only(fields,&["blocks","state","nbt"],"冒险方块条件")?;
    let blocks=match fields.get("blocks"){Some(Value::String(block))=>vec![block.clone()],Some(Value::List(blocks))=>blocks.iter().map(|v|text(v).map(str::to_owned)).collect::<Result<Vec<_>>>()?,_=>bail!("旧版CanDestroy/CanPlaceOn不能表示无方块限定条件")};
    let mut suffix=String::new();
    if let Some(Value::Compound(state))=fields.get("state"){
        if target<1519&&!state.is_empty(){bail!("目标1.12及以前CanDestroy/CanPlaceOn不能表达方块状态条件")}
        let mut properties=state.iter().map(|(key,value)|Ok(format!("{key}={}",text(value)?))).collect::<Result<Vec<_>>>()?;properties.sort();
        if !properties.is_empty(){suffix=format!("[{}]",properties.join(","));}
    }else if fields.contains_key("state"){bail!("旧版不能表达范围型方块状态条件")}
    if let Some(nbt)=fields.get("nbt"){if target<1519{bail!("目标1.12及以前冒险条件不支持NBT")};match nbt{Value::String(nbt)=>suffix.push_str(nbt),Value::Compound(_)=>suffix.push_str(&write_snbt(nbt)?),_=>bail!("冒险NBT条件应为SNBT字符串或Compound")}}
    Ok(blocks.into_iter().map(|block|Value::String(format!("{block}{suffix}"))).collect())
}
fn legacy_block_state(mut value:Value)->Result<Value>{
    const BOOLEAN:&[&str]=&["attached","bottom","conditional","disarmed","drag","enabled","extended","eye","falling","hanging","has_bottle_0","has_bottle_1","has_bottle_2","has_record","has_book","inverted","in_wall","lit","locked","occupied","open","persistent","powered","short","signal_fire","snowy","triggered","unstable","waterlogged","berries","bloom","shrieking","can_summon","up","down","north","east","south","west","slot_0_occupied","slot_1_occupied","slot_2_occupied","slot_3_occupied","slot_4_occupied","slot_5_occupied","cracked","crafting"];
    for(key,value)in object(&mut value)?{*value=Value::String(match &*value{Value::String(v)=>v.clone(),Value::Byte(v)if BOOLEAN.contains(&key.as_str())&&[0,1].contains(v)=>if *v==0{"false".into()}else{"true".into()},Value::Byte(v)=>v.to_string(),Value::Short(v)=>v.to_string(),Value::Int(v)=>v.to_string(),Value::Long(v)=>v.to_string(),_=>bail!("BlockStateTag属性{key}不是原版字符串/整数")});}Ok(value)
}

// The old predicate parser accepts SNBT, not JSON. Quote only the two escapes
// supported by StringReader; preserve literal newlines and all numeric widths.
pub(super) fn write_snbt(value:&Value)->Result<String>{
    let quote=|text:&str|format!("\"{}\"",text.replace('\\',"\\\\").replace('"',"\\\""));
    Ok(match value{
        Value::Byte(v)=>format!("{v}b"),Value::Short(v)=>format!("{v}s"),Value::Int(v)=>v.to_string(),Value::Long(v)=>format!("{v}L"),
        Value::Float(v)if v.is_finite()=>format!("{v}f"),Value::Double(v)if v.is_finite()=>format!("{v}d"),
        Value::String(v)=>quote(v),
        Value::List(values)=>format!("[{}]",values.iter().map(write_snbt).collect::<Result<Vec<_>>>()?.join(",")),
        Value::Compound(fields)=>{let mut fields=fields.iter().collect::<Vec<_>>();fields.sort_by(|a,b|a.0.cmp(b.0));format!("{{{}}}",fields.into_iter().map(|(key,value)|Ok(format!("{}:{}",quote(key),write_snbt(value)?))).collect::<Result<Vec<_>>>()?.join(","))},
        Value::ByteArray(values)=>format!("[B;{}]",values.iter().map(|v|format!("{v}b")).collect::<Vec<_>>().join(",")),
        Value::IntArray(values)=>format!("[I;{}]",values.iter().map(ToString::to_string).collect::<Vec<_>>().join(",")),
        Value::LongArray(values)=>format!("[L;{}]",values.iter().map(|v|format!("{v}L")).collect::<Vec<_>>().join(",")),
        _=>bail!("非有限浮点NBT不能等价写为旧SNBT"),
    })
}

fn firework_explosion(value:&mut Value,to_component:bool)->Result<()> {
    let fields=object(value)?;
    only(fields,if to_component{&["Type","Colors","FadeColors","Trail","Flicker"]}else{&["shape","colors","fade_colors","has_trail","has_twinkle"]},"烟花爆炸")?;
    const SHAPES:&[&str]=&["small_ball","large_ball","star","creeper","burst"];
    if to_component{
        let shape=fields.remove("Type").map(|v|number(&v)).transpose()?.unwrap_or(0);
        let shape=SHAPES.get(usize::try_from(shape)?).context("烟花Type没有原版形状等价值")?;fields.insert("shape".into(),Value::String((*shape).into()));
    }else{
        let shape=fields.remove("shape").unwrap_or(Value::String("small_ball".into()));
        let shape=SHAPES.iter().position(|known|*known==text(&shape).unwrap_or("")).context("烟花shape没有旧版等价形状")?;fields.insert("Type".into(),Value::Byte(shape as i8));
    }
    for(old,new)in[("Colors","colors"),("FadeColors","fade_colors"),("Trail","has_trail"),("Flicker","has_twinkle")]{
        let(from,to)=if to_component{(old,new)}else{(new,old)};
        if let Some(value)=fields.remove(from){if old=="Colors"||old=="FadeColors"{if !matches!(value,Value::IntArray(_)){bail!("烟花颜色必须为IntArray")}}else{boolean(&value)?;}fields.insert(to.into(),value);}
    }Ok(())
}
fn double(value:&Value)->Result<f64>{match value{Value::Double(v)if v.is_finite()=>Ok(*v),Value::Float(v)if v.is_finite()=>Ok(f64::from(*v)),_=>bail!("地图装饰坐标/角度必须为有限浮点数")}}
fn legacy_map_decorations(value:Value)->Result<Value>{
    let mut output=Fields::new();for mut decoration in list(value)?{
        let fields=object(&mut decoration)?;only(fields,&["id","type","x","z","rot"],"地图装饰")?;
        let id=text(fields.get("id").context("地图装饰缺少id")?)?.to_owned();
        let ty=number(fields.get("type").context("地图装饰缺少type")?)?;let ty=MAP_TYPES.get(usize::try_from(ty)?).context("地图装饰type没有已核实映射")?;
        let rotation=double(fields.get("rot").context("地图装饰缺少rot")?)?;let rotation32=rotation as f32;
        if f64::from(rotation32)!=rotation{bail!("地图装饰旋转角不能无损表示为目标Float")}
        insert(&mut output,&id,Value::Compound(Fields::from([("type".into(),Value::String((*ty).into())),("x".into(),Value::Double(double(fields.get("x").context("地图装饰缺少x")?)?)),("z".into(),Value::Double(double(fields.get("z").context("地图装饰缺少z")?)?)),("rotation".into(),Value::Float(rotation32))])))?;
    }Ok(Value::Compound(output))
}

pub(super) fn component_to_legacy(key:&str,value:&mut Value,tag:&mut Fields,_block:&mut Fields,display:&mut Fields,target:i32,hide_flags:&mut i32,report:&mut PayloadMigrationReport)->Result<bool>{
    match key{
        "minecraft:enchantments"|"minecraft:stored_enchantments"=>{
            let fields=object(value)?;only(fields,&["levels","show_in_tooltip"],"附魔组件")?;
            if fields.get("show_in_tooltip")==Some(&Value::Byte(0)){*hide_flags|=if key=="minecraft:enchantments"{1}else{32};}
            let Value::Compound(levels)=fields.get("levels").context("附魔组件缺少levels")?else{bail!("附魔levels不是Compound")};
            let mut entries=Vec::new();for(id,level)in levels{let level=number(level)?;if !(1..=255).contains(&level){bail!("附魔等级不能无损保存")};entries.push(Value::Compound(Fields::from([("id".into(),enchantment_for_target(id,target)?),("lvl".into(),Value::Short(level as i16))])));}
            entries.sort_by_key(|entry|format!("{entry:?}"));let name=if key=="minecraft:stored_enchantments"{"StoredEnchantments"}else if target<1494{"ench"}else{"Enchantments"};insert(tag,name,Value::List(entries))?;
        }
        "minecraft:can_break"|"minecraft:can_place_on"=>{
            let fields=object(value)?;only(fields,&["predicates","show_in_tooltip"],"冒险模式组件")?;
            if fields.get("show_in_tooltip")==Some(&Value::Byte(0)){*hide_flags|=if key=="minecraft:can_break"{8}else{16};}
            let Value::List(predicates)=fields.get("predicates").context("冒险组件缺少predicates")?else{bail!("predicates不是List")};
            let mut output=Vec::new();for predicate in predicates{output.extend(predicate_strings(predicate,target)?);}
            insert(tag,if key=="minecraft:can_break"{"CanDestroy"}else{"CanPlaceOn"},Value::List(output))?;
        }
        "minecraft:attribute_modifiers"=>{
            let fields=object(value)?;only(fields,&["modifiers","show_in_tooltip"],"属性组件")?;
            if fields.get("show_in_tooltip")==Some(&Value::Byte(0)){*hide_flags|=2;}
            let mut entries=list(fields.get("modifiers").context("属性组件缺少modifiers")?.clone())?;migrate_attribute_entries(&mut entries,3818,3818)?;
            let mut output=Vec::new();for mut entry in entries{let fields=object(&mut entry)?;
                let slot=fields.remove("slot");if let Some(slot)=slot{let slot=text(&slot)?;if slot!="any"{if !["mainhand","offhand","feet","legs","chest","head"].contains(&slot){bail!("旧版单装备槽不能等价表示属性slot组 {slot}")};fields.insert("Slot".into(),Value::String(slot.into()));}}
                let ty=fields.remove("type").unwrap();let operation=operation_number(text(fields.get("operation").unwrap())?)?;fields.remove("operation");fields.insert("Operation".into(),Value::Int(operation));
                for(old,new)in[("uuid","UUID"),("name","Name"),("amount","Amount")]{if let Some(value)=fields.remove(old){fields.insert(new.into(),value);}}
                let mut carrier=Fields::from([("Attributes".into(),Value::List(vec![Value::Compound(Fields::from([("Name".into(),ty),("Modifiers".into(),Value::List(vec![Value::Compound(fields.clone())]))]))]))]);
                migrate_entity_attributes(&mut carrier,3818,target)?;
                let Value::List(mut attributes)=carrier.remove("Attributes").unwrap()else{unreachable!()};let attr=object(&mut attributes[0])?;let ty=attr.remove("Name").unwrap();let Value::List(mut modifiers)=attr.remove("Modifiers").unwrap()else{unreachable!()};let modifier=object(&mut modifiers[0])?;modifier.insert("AttributeName".into(),ty);output.push(Value::Compound(modifier.clone()));
            }insert(tag,"AttributeModifiers",Value::List(output))?;
        }
        "minecraft:trim"=>{if target<3463{bail!("目标版本没有盔甲纹饰，不能删除material/pattern或将其伪装为普通标签")};validate_trim(value)?;let fields=object(value)?;if fields.remove("show_in_tooltip")==Some(Value::Byte(0)){*hide_flags|=128;}insert(tag,"Trim",value.clone())?;}
        "minecraft:profile"=>{migrate_profile(value,3818,target,3818)?;insert(tag,"SkullOwner",value.clone())?;}
        "minecraft:map_id"=>{let id=number(value)?;if id<0{bail!("地图id不能为负")};insert(tag,"map",Value::Int(id))?;}
        "minecraft:map_color"=>{if !matches!(value,Value::Int(_)){bail!("地图颜色不是Int")};insert(display,"MapColor",value.clone())?;}
        "minecraft:map_decorations"=>{
            let mut output=Vec::new();for(id,mut decoration)in object(value)?.clone(){let fields=object(&mut decoration)?;only(fields,&["type","x","z","rotation"],"地图装饰")?;
                let ty=text(fields.get("type").context("地图装饰缺少type")?)?;let ty=ty.strip_prefix("minecraft:").unwrap_or(ty);let ty=MAP_TYPES.iter().position(|known|*known==ty).context("目标旧版没有地图装饰类型")?;
                if target<1519&&ty>=10{bail!("目标1.12及以前没有此地图装饰类型")}
                output.push(Value::Compound(Fields::from([("id".into(),Value::String(id)),("type".into(),Value::Byte(ty as i8)),("x".into(),Value::Double(double(fields.get("x").context("地图装饰缺少x")?)?)),("z".into(),Value::Double(double(fields.get("z").context("地图装饰缺少z")?)?)),("rot".into(),Value::Double(double(fields.get("rotation").context("地图装饰缺少rotation")?)?))])));
            }insert(tag,"Decorations",Value::List(output))?;
        }
        "minecraft:firework_explosion"=>{firework_explosion(value,false)?;insert(tag,"Explosion",value.clone())?;}
        "minecraft:fireworks"=>{
            let fields=object(value)?;only(fields,&["flight_duration","explosions"],"烟花火箭组件")?;let mut fireworks=Fields::new();
            if let Some(flight)=fields.get("flight_duration"){let flight=number(flight)?;if !(0..=127).contains(&flight){bail!("旧版Byte飞行时长不能无损保存{flight}")};fireworks.insert("Flight".into(),Value::Byte(flight as i8));}
            if let Some(explosions)=fields.get("explosions"){let mut explosions=list(explosions.clone())?;for explosion in &mut explosions{firework_explosion(explosion,false)?;}fireworks.insert("Explosions".into(),Value::List(explosions));}
            insert(tag,"Fireworks",Value::Compound(fireworks))?;
        }
        "minecraft:charged_projectiles"|"minecraft:bundle_contents"=>{
            if key=="minecraft:charged_projectiles"&&target<1901{bail!("目标正式版本没有弩及上弦弹药")}
            if key=="minecraft:bundle_contents"&&target<4080{bail!("目标正式版本没有收纳袋，不能保留不可用物品内容")}
            let mut items=list(value.clone())?;for item in &mut items{item_payload(item,3818,target,report)?;}
            if key=="minecraft:charged_projectiles"{insert(tag,"Charged",Value::Byte(i8::from(!items.is_empty())))?;}
            insert(tag,if key=="minecraft:charged_projectiles"{"ChargedProjectiles"}else{"Items"},Value::List(items))?;
        }
        "minecraft:block_state"=>{for value in object(value)?.values(){text(value)?;}insert(tag,"BlockStateTag",value.clone())?;}
        "minecraft:entity_data"=>{migrate_entity(value,3818,target,report)?;insert(tag,"EntityTag",value.clone())?;}
        "minecraft:instrument"=>{if target<3105{bail!("目标没有山羊角乐器")};text(value)?;insert(tag,"instrument",value.clone())?;}
        "minecraft:recipes"=>{for recipe in list(value.clone())?{text(&recipe)?;}insert(tag,"Recipes",value.clone())?;}
        "minecraft:debug_stick_state"=>{for value in object(value)?.values(){text(value)?;}if target<1519{bail!("目标旧版没有调试棒")};insert(tag,"DebugProperty",value.clone())?;}
        "minecraft:lodestone_tracker"=>{
            if target<2566{bail!("目标没有磁石指南针")};let fields=object(value)?;only(fields,&["target","tracked"],"磁石组件")?;
            if let Some(mut destination)=fields.get("target").cloned(){let destination=object(&mut destination)?;only(destination,&["pos","dimension"],"磁石目标")?;insert(tag,"LodestonePos",destination.remove("pos").context("磁石缺坐标")?)?;insert(tag,"LodestoneDimension",destination.remove("dimension").context("磁石缺维度")?)?;}
            if let Some(tracked)=fields.get("tracked"){boolean(tracked)?;insert(tag,"LodestoneTracked",tracked.clone())?;}
        }
        _=>return Ok(false),
    }Ok(true)
}

pub(super) fn migrate_component(key:&str,value:&mut Value,source:i32,target:i32,report:&mut PayloadMigrationReport,hidden:&mut Vec<String>)->Result<bool>{
    match key{
        "minecraft:attribute_modifiers"|"minecraft:can_break"|"minecraft:can_place_on"=>{
            let inner=if key=="minecraft:attribute_modifiers"{"modifiers"}else{"predicates"};
            if source>=4307&&key!="minecraft:attribute_modifiers"&&matches!(value,Value::Compound(_)){*value=Value::List(vec![value.clone()]);}
            if source<4307&&target>=4307{let fields=object(value)?;only(fields,&[inner,"show_in_tooltip"],"提示组件")?;if fields.get("show_in_tooltip")==Some(&Value::Byte(0)){hidden.push(key.into());}*value=fields.remove(inner).context("物品提示组件缺少实际值")?;}
            else if source>=4307&&target<4307{*value=Value::Compound(Fields::from([(inner.into(),value.clone())]));}
            let entries=if target<4307{object(value)?.get_mut(inner).context("物品组件缺少实际值")?}else{&mut *value};
            let Value::List(entries)=entries else{bail!("{key}实际值不是列表")};
            if key=="minecraft:attribute_modifiers"{migrate_attribute_entries(entries,source,target)?;}else{for predicate in entries{let fields=object(predicate)?;only(fields,&["blocks","state","nbt"],"冒险条件")?;}}
        }
        "minecraft:trim"=>{validate_trim(value)?;if source<4307&&target>=4307&&object(value)?.remove("show_in_tooltip")==Some(Value::Byte(0)){hidden.push(key.into());}}
        "minecraft:profile"=>{
            if let Value::String(name)=value{*value=Value::Compound(Fields::from([("name".into(),Value::String(name.clone()))]));}
            let fields=object(value)?;only(fields,&["name","id","properties"],"玩家资料（皮肤覆盖字段需目标codec核实）")?;
            let mut validated=value.clone();migrate_profile(&mut validated,3818,3817,3818)?;migrate_profile(&mut validated,3817,3818,3818)?;
        }
        "minecraft:map_id"|"minecraft:map_color"=>{if !matches!(value,Value::Int(_)){bail!("{key}必须为Int")};if key=="minecraft:map_color"&&target>=5008{bail!("目标v5008移除了map_color；自定义地图颜色无等价字段，原数据保留")}}
        "minecraft:map_decorations"=>{
            for decoration in object(value)?.values_mut(){let fields=object(decoration)?;only(fields,&["type","x","z","rotation"],"地图装饰")?;
                let ty=text(fields.get("type").context("地图装饰缺少type")?)?;let ty=ty.strip_prefix("minecraft:").unwrap_or(ty);
                if !MAP_TYPES.contains(&ty)&&ty!="trial_chambers"&&!["abandoned_camp","ancient_city","desert_pyramid","mineshaft","ocean_ruin_warm"].contains(&ty){bail!("地图装饰类型 {ty} 不在已核实原版注册表")}
                if ty=="trial_chambers"&&target<3953{bail!("目标正式版本没有试炼密室地图装饰")}
                if ["abandoned_camp","ancient_city","desert_pyramid","mineshaft","ocean_ruin_warm"].contains(&ty)&&target<5008{bail!("目标v5008之前没有此新地图装饰类型")}
                for key in ["x","z","rotation"]{double(fields.get(key).with_context(||format!("地图装饰缺少{key}"))?)?;}
            }
        }
        "minecraft:firework_explosion"=>{let mut check=value.clone();firework_explosion(&mut check,false)?;}
        "minecraft:fireworks"=>{let mut check=value.clone();component_to_legacy(key,&mut check,&mut Fields::new(),&mut Fields::new(),&mut Fields::new(),3818,&mut 0,report)?;}
        "minecraft:charged_projectiles"|"minecraft:bundle_contents"=>{let Value::List(items)=value else{bail!("{key}应为ItemStack列表")};for item in items{item_payload(item,source,target,report)?;}}
        "minecraft:block_state"=>{for value in object(value)?.values(){text(value)?;}}
        "minecraft:entity_data"=>migrate_entity(value,source,target,report)?,
        "minecraft:recipes"=>{for recipe in list(value.clone())?{text(&recipe)?;}}
        "minecraft:instrument"=>{text(value)?;}
        "minecraft:debug_stick_state"=>{for value in object(value)?.values(){text(value)?;}}
        "minecraft:lodestone_tracker"=>{
            let tracker=object(value)?;only(tracker,&["target","tracked"],"磁石组件")?;
            if let Some(target_field)=tracker.get_mut("target"){let fields=object(target_field)?;only(fields,&["pos","dimension"],"磁石目标")?;text(fields.get("dimension").context("磁石缺少维度")?)?;
                if let Some(position)=fields.get_mut("pos"){
                    if source<3820&&target>=3820{let coordinates=object(position)?;only(coordinates,&["X","Y","Z"],"磁石坐标")?;let xyz=["X","Y","Z"].map(|axis|coordinates.get(axis).context("磁石缺少坐标轴").and_then(number));let[x,y,z]=xyz;*position=Value::IntArray(fastnbt::IntArray::new(vec![x?,y?,z?]));}
                    else if source>=3820&&target<3820{let Value::IntArray(xyz)=position else{bail!("磁石坐标应为IntArray")};if xyz.len()!=3{bail!("磁石坐标长度错误")};*position=Value::Compound([("X".into(),Value::Int(xyz[0])),("Y".into(),Value::Int(xyz[1])),("Z".into(),Value::Int(xyz[2]))].into());}
                }
            }if let Some(tracked)=tracker.get("tracked"){boolean(tracked)?;}
        }
        _=>return Ok(false),
    }Ok(true)
}

#[cfg(test)]
mod tests{
    use super::*;
    fn compound(fields: &[(&str,Value)])->Value{Value::Compound(fields.iter().map(|(k,v)|((*k).into(),v.clone())).collect())}
    #[test]
    fn numeric_enchantments_and_all_requested_hide_flags_bridge(){
        let mut tag=Fields::from([("HideFlags".into(),Value::Int(8|16|128)),("ench".into(),Value::List(vec![compound(&[("id",Value::Short(22)),("lvl",Value::Short(3))])])),("CanDestroy".into(),Value::List(vec![Value::String("minecraft:stone".into())])),("CanPlaceOn".into(),Value::List(vec![Value::String("minecraft:dirt".into())])),("Trim".into(),compound(&[("material",Value::String("minecraft:gold".into())),("pattern",Value::String("minecraft:sentry".into()))]))]);
        let mut components=Fields::new();legacy_to_components(&mut tag,&mut components,1343,"minecraft:diamond_chestplate",&mut PayloadMigrationReport::default()).unwrap();
        let mut enchant=components.remove("minecraft:enchantments").unwrap();let mut restored=Fields::new();let mut flags=0;component_to_legacy("minecraft:enchantments",&mut enchant,&mut restored,&mut Fields::new(),&mut Fields::new(),1343,&mut flags,&mut PayloadMigrationReport::default()).unwrap();
        assert!(matches!(restored.get("ench"),Some(Value::List(entries))if matches!(&entries[0],Value::Compound(fields)if fields.get("id")==Some(&Value::Short(22)))));
        assert_eq!(object(components.get_mut("minecraft:trim").unwrap()).unwrap().get("show_in_tooltip"),Some(&Value::Byte(0)));
    }
    #[test]
    fn durability_is_not_block_metadata_and_returns_to_top_level(){
        let mut tool=Fields::from([("id".into(),Value::String("minecraft:diamond_sword".into())),("Damage".into(),Value::Short(387))]);
        prepare_legacy_stack(&mut tool,1343,3465,&mut PayloadMigrationReport::default()).unwrap();assert_eq!(tool.get("Damage"),Some(&Value::Short(0)));assert_eq!(object(tool.get_mut("tag").unwrap()).unwrap().get("Damage"),Some(&Value::Int(387)));
        finish_legacy_stack(&mut tool,3465,1343,&mut PayloadMigrationReport::default()).unwrap();assert_eq!(tool.get("Damage"),Some(&Value::Short(387)));
        let mut wool=Fields::from([("id".into(),Value::String("minecraft:wool".into())),("Damage".into(),Value::Short(14))]);let before=wool.clone();prepare_legacy_stack(&mut wool,1343,3465,&mut PayloadMigrationReport::default()).unwrap();assert_eq!(wool,before);
    }
    #[test]
    fn adventure_predicate_keeps_state_nbt_and_multi_block_semantics(){
        let source="minecraft:chest[facing=north]{Lock:\"中文\"}";let value=legacy_predicate(source,3465).unwrap();assert_eq!(predicate_strings(&value,3465).unwrap(),vec![Value::String(source.into())]);assert!(predicate_strings(&value,1343).is_err());
    }
    #[test]
    fn firework_fields_roundtrip_and_unknown_payload_is_rejected(){
        let mut value=compound(&[("Type",Value::Byte(3)),("Colors",Value::IntArray(fastnbt::IntArray::new(vec![0xff0000]))),("Trail",Value::Byte(1))]);let original=value.clone();firework_explosion(&mut value,true).unwrap();firework_explosion(&mut value,false).unwrap();assert_eq!(value,original);
        object(&mut value).unwrap().insert("unknown".into(),Value::Int(2));assert!(firework_explosion(&mut value,true).is_err());
    }
    #[test]
    fn item_attribute_modifier_reuses_uuid_names_and_hideflags_bridge(){
        let modifier=compound(&[("AttributeName",Value::String("minecraft:generic.attack_damage".into())),("Name",Value::String("custom tool".into())),("UUID",uuid_value("12345678-1234-5678-9abc-123456789abc").unwrap()),("Amount",Value::Double(1.5)),("Operation",Value::Int(2)),("Slot",Value::String("mainhand".into()))]);
        let mut tag=Fields::from([("HideFlags".into(),Value::Int(2)),("AttributeModifiers".into(),Value::List(vec![modifier.clone()]))]);
        let mut components=Fields::new();let mut report=PayloadMigrationReport::default();legacy_to_components(&mut tag,&mut components,3465,"minecraft:diamond_sword",&mut report).unwrap();
        let mut component=components.remove("minecraft:attribute_modifiers").unwrap();let mut restored=Fields::new();let mut flags=0;
        component_to_legacy("minecraft:attribute_modifiers",&mut component,&mut restored,&mut Fields::new(),&mut Fields::new(),3465,&mut flags,&mut report).unwrap();
        assert_eq!(flags,2);assert_eq!(restored.get("AttributeModifiers"),Some(&Value::List(vec![modifier])));
        let mut hidden=Vec::new();migrate_component("minecraft:attribute_modifiers",&mut component,3818,4671,&mut report,&mut hidden).unwrap();assert_eq!(hidden,vec!["minecraft:attribute_modifiers"]);assert!(matches!(component,Value::List(_)));
        migrate_component("minecraft:attribute_modifiers",&mut component,4671,3818,&mut report,&mut Vec::new()).unwrap();
        let fields=object(&mut component).unwrap();let Value::List(entries)=fields.get("modifiers").unwrap()else{unreachable!()};
        assert!(matches!(&entries[0],Value::Compound(fields)if fields.get("uuid")==Some(&uuid_value("12345678-1234-5678-9abc-123456789abc").unwrap())&&fields.get("amount")==Some(&Value::Double(1.5))));
    }
    #[test]
    fn modern_compact_predicate_and_typed_nbt_have_exact_legacy_representation(){
        let nbt=compound(&[("text",Value::String("中文\\\"".into())),("bytes",Value::ByteArray(fastnbt::ByteArray::new(vec![1,-1]))),("longs",Value::LongArray(fastnbt::LongArray::new(vec![i64::MAX])))]);
        let mut component=compound(&[("blocks",Value::String("minecraft:chest".into())),("nbt",nbt)]);
        let mut report=PayloadMigrationReport::default();migrate_component("minecraft:can_break",&mut component,4671,3818,&mut report,&mut Vec::new()).unwrap();
        let mut tag=Fields::new();component_to_legacy("minecraft:can_break",&mut component,&mut tag,&mut Fields::new(),&mut Fields::new(),3465,&mut 0,&mut report).unwrap();
        let Value::List(entries)=tag.get("CanDestroy").unwrap()else{unreachable!()};let encoded=text(&entries[0]).unwrap();assert!(encoded.contains("[B;1b,-1b]"));assert!(encoded.contains("[L;9223372036854775807L]"));assert!(encoded.contains("中文"));
    }
    #[test]
    fn map_decorations_preserve_coordinates_rotation_and_identity(){
        let original=Value::List(vec![compound(&[("id",Value::String("标记一".into())),("type",Value::Byte(8)),("x",Value::Double(14.25)),("z",Value::Double(-10.0)),("rot",Value::Double(90.0))])]);
        let mut components=legacy_map_decorations(original.clone()).unwrap();let mut tag=Fields::new();component_to_legacy("minecraft:map_decorations",&mut components,&mut tag,&mut Fields::new(),&mut Fields::new(),3465,&mut 0,&mut PayloadMigrationReport::default()).unwrap();assert_eq!(tag.get("Decorations"),Some(&original));
    }
}
