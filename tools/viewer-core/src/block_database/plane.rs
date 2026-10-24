//! Shared orthographic model raster. No item images or flattened side-face fallback.
use super::*;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct LayerVisualError {
    pub kind: &'static str,
    pub resource_key: Option<String>,
    pub message: String,
    pub retryable: bool,
}
impl std::fmt::Display for LayerVisualError {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {write!(f,"{}: {}",self.kind,self.message)}
}
impl std::error::Error for LayerVisualError {}
impl LayerVisualError {
    pub(crate) fn from_error(error: anyhow::Error)->Self {
        let message=format!("{error:#}");let lower=message.to_lowercase();
        let kind=if lower.contains("changed") {"resource_changed"}
        else if lower.contains("blockstate missing") {"missing_blockstate"}
        else if lower.contains("model missing")||lower.contains("model reference missing") {"missing_model"}
        else if lower.contains("texture missing")||lower.contains("texture variable missing") {"missing_texture"}
        else if lower.contains("unsupported builtin") {"unsupported_model"}
        else if lower.contains("invalid state")||lower.contains("invalid block") {"invalid_state"}
        else if lower.contains("decode")||lower.contains("json")||lower.contains("model ")||lower.contains("uv ")||lower.contains("animation") {"decode_failed"}
        else if error.chain().any(|e|e.downcast_ref::<std::io::Error>().is_some()) {"temporary_io"}
        else {"missing_version_resources"};
        let resource_key=message.split_whitespace().find(|part|part.starts_with("assets/")).map(|part|part.trim_end_matches([':',',']).to_owned());
        Self{kind,resource_key,message,retryable:matches!(kind,"temporary_io"|"resource_changed")}
    }
}
pub(super) struct PlaneOutput {pub image:RgbaImage,pub tints:BTreeSet<i16>,pub faces:Vec<serde_json::Value>}
struct Surface {quad:[[f32;3];4],uv:[[f32;2];4],texture:Arc<RgbaImage>,tint:u32}
#[derive(Clone,Copy)]
struct Fragment {depth:f32,rgba:[u8;4]}

fn normal(side:u8)->[f32;3] {match side{0=>[0.,-1.,0.],1=>[0.,1.,0.],2=>[0.,0.,-1.],3=>[0.,0.,1.],4=>[-1.,0.,0.],_=>[1.,0.,0.]}}
fn corners(e:&Element,side:u8)->[[f32;3];4] {
    let [x,y,z]=e.from;let [a,b,c]=e.to;
    match side {
        0=>[[x,y,c],[a,y,c],[a,y,z],[x,y,z]],
        1=>[[x,b,z],[a,b,z],[a,b,c],[x,b,c]],
        2=>[[a,b,z],[x,b,z],[x,y,z],[a,y,z]],
        3=>[[x,b,c],[a,b,c],[a,y,c],[x,y,c]],
        4=>[[x,b,z],[x,b,c],[x,y,c],[x,y,z]],
        _=>[[a,b,c],[a,b,z],[a,y,z],[a,y,c]],
    }
}
fn variant_rotate(p:&mut [f32;3],v:&Variant) {rotate(p,0,-f32::from(v.x)*90.);rotate(p,1,-f32::from(v.y)*90.);}
fn facing(e:&Element,v:&Variant,side:u8,plane:Plane)->bool {
    let mut n=normal(side);
    if let Some(r)=&e.rotation {if r.kind==0{rotate(&mut n,r.axis,r.angle)}else{for axis in 0..3{rotate(&mut n,axis,r.vector[usize::from(axis)])}}}
    variant_rotate(&mut n,v);
    match plane {Plane::Top=>n[1]>0.0001,Plane::Side=>n[2]< -0.0001}
}
fn uvlock(mut uv:[f32;2],side:u8,v:&Variant)->[f32;2] {
    if !v.uvlock{return uv;}
    let [u,w]=uv;
    let mut p=match side {0=>[u,0.,16.-w],1=>[u,16.,w],2=>[16.-u,16.-w,0.],3=>[u,16.-w,16.],4=>[0.,16.-w,u],_=>[16.,16.-w,16.-u]};
    for a in &mut p{*a-=8.;}variant_rotate(&mut p,v);for a in &mut p{*a+=8.;}
    let mut n=normal(side);variant_rotate(&mut n,v);
    uv=if n[1]< -0.5{[p[0],16.-p[2]]}else if n[1]>0.5{[p[0],p[2]]}else if n[2]< -0.5{[16.-p[0],16.-p[1]]}else if n[2]>0.5{[p[0],16.-p[1]]}else if n[0]< -0.5{[p[2],16.-p[1]]}else{[16.-p[2],16.-p[1]]};uv
}
fn face_uv(f:&Face,v:&Variant)->[[f32;2];4] {
    [[0.,0.],[1.,0.],[1.,1.],[0.,1.]].map(|[mut u,mut w]|{
        for _ in 0..(f.rotation/90)%4 {(u,w)=(w,1.-u);}
        uvlock([f.uv[0]+u*(f.uv[2]-f.uv[0]),f.uv[1]+w*(f.uv[3]-f.uv[1])],f.side,v)
    })
}
fn raster(surfaces:&[Surface],plane:Plane)->Result<RgbaImage> {
    let mut pixels:Vec<Vec<Fragment>>=vec![Vec::new();32*32];
    for surface in surfaces {
        if surface.quad.iter().flatten().chain(surface.uv.iter().flatten()).any(|n|!n.is_finite()){bail!("model coordinates/UV are not finite");}
        let q=surface.quad.map(|p|match plane{Plane::Top=>[p[0]*2.,p[2]*2.],Plane::Side=>[p[0]*2.,32.-p[1]*2.]});
        let depth=surface.quad.map(|p|match plane{Plane::Top=>p[1],Plane::Side=>-p[2]});
        let a=[q[1][0]-q[0][0],q[1][1]-q[0][1]];let b=[q[3][0]-q[0][0],q[3][1]-q[0][1]];
        let det=a[0]*b[1]-a[1]*b[0];if det.abs()<0.0001{continue;}
        let bounds=|axis:usize|{let lo=q.iter().map(|p|p[axis]).fold(f32::INFINITY,f32::min).floor().clamp(0.,32.) as usize;let hi=q.iter().map(|p|p[axis]).fold(f32::NEG_INFINITY,f32::max).ceil().clamp(0.,32.) as usize;(lo,hi)};
        let (x0,x1)=bounds(0);let(y0,y1)=bounds(1);
        for y in y0..y1 {for x in x0..x1 {
            let p=[x as f32+0.5-q[0][0],y as f32+0.5-q[0][1]];
            let u=(p[0]*b[1]-p[1]*b[0])/det;let w=(a[0]*p[1]-a[1]*p[0])/det;
            if !(0.0..1.0).contains(&u)||!(0.0..1.0).contains(&w){continue;}
            let uv:[f32;2]=std::array::from_fn(|i|surface.uv[0][i]+u*(surface.uv[1][i]-surface.uv[0][i])+w*(surface.uv[3][i]-surface.uv[0][i]));
            let tx=(uv[0]*surface.texture.width() as f32/16.).floor().clamp(0.,surface.texture.width() as f32-1.) as u32;
            let ty=(uv[1]*surface.texture.height() as f32/16.).floor().clamp(0.,surface.texture.height() as f32-1.) as u32;
            let mut rgba=surface.texture.get_pixel(tx,ty).0;if rgba[3]==0{continue;}
            for c in 0..3 {rgba[c]=((u32::from(rgba[c])*((surface.tint>>((2-c)*8))&255)+127)/255) as u8;}
            let fragments=&mut pixels[y*32+x];fragments.try_reserve(1).context("model fragment allocation failed")?;
            fragments.push(Fragment{depth:depth[0]+u*(depth[1]-depth[0])+w*(depth[3]-depth[0]),rgba});
        }}
    }
    let mut out=RgbaImage::new(32,32);
    for (index,fragments) in pixels.iter_mut().enumerate(){
        // Stable ties preserve multipart order; real depth determines occlusion, not JSON order.
        fragments.sort_by(|a,b|a.depth.total_cmp(&b.depth));let mut color=[0f32;3];let mut alpha=0f32;
        for f in fragments {let a=f32::from(f.rgba[3])/255.;for c in 0..3{color[c]=f32::from(f.rgba[c])*a+color[c]*(1.-a);}alpha=a+alpha*(1.-a);}
        if alpha>0.{out.put_pixel(index as u32%32,index as u32/32,image::Rgba([(color[0]/alpha).round() as u8,(color[1]/alpha).round() as u8,(color[2]/alpha).round() as u8,(alpha*255.).round() as u8]));}
    }
    Ok(out)
}
/// Vanilla ModelPart.Cube layout, including its downward model Y and mirrored boxes.
/// Geometry/UV below are the 1.21 LayerDefinitions, not item icon projections.
fn entity_box(out:&mut Vec<Surface>,plane:Plane,from:[f32;3],size:[f32;3],uv:[f32;2],atlas:[f32;2],deform:f32,mirror:bool,texture:&Arc<RgbaImage>,tint:u32,map:&impl Fn([f32;3])->[f32;3]) {
    let e=Element{from:from.map(|v|v-deform),to:std::array::from_fn(|i|from[i]+size[i]+deform),faces:vec![],rotation:None};
    let [x,y,z]=size;let [u,v]=uv;
    let rects=[[u+z,v,u+z+x,v+z],[u+z+x,v+z,u+z+x+x,v],
        [u+z,v+z,u+z+x,v+z+y],[u+z+x+z,v+z,u+z+x+z+x,v+z+y],
        [u,v+z,u+z,v+z+y],[u+z+x,v+z,u+z+x+z,v+z+y]];
    let origin=map([0.;3]);
    for side in 0..6u8 {
        let mut n=normal(side);if mirror{n[0]= -n[0];}let transformed=map(n);
        let visible=match plane{Plane::Top=>transformed[1]-origin[1]>0.0001,Plane::Side=>transformed[2]-origin[2]< -0.0001};if !visible{continue;}
        let quad=corners(&e,side).map(|mut point|{if mirror{point[0]=e.from[0]+e.to[0]-point[0];}map(point)});
        let r=rects[usize::from(side)];let uv=[[r[0],r[1]],[r[2],r[1]],[r[2],r[3]],[r[0],r[3]]].map(|p|[p[0]*16./atlas[0],p[1]*16./atlas[1]]);
        out.push(Surface{quad,uv,texture:texture.clone(),tint});
    }
}
fn banner_dye(color:&str)->Result<u32>{Ok(match color{"white"=>0xf9fffe,"orange"=>0xf9801d,"magenta"=>0xc74ebd,"light_blue"=>0x3ab3da,"yellow"=>0xfed83d,"lime"=>0x80c71f,"pink"=>0xf38baa,"gray"=>0x474f52,"light_gray"=>0x9d9d97,"cyan"=>0x169c9c,"purple"=>0x8932b8,"blue"=>0x3c44aa,"brown"=>0x835432,"green"=>0x5e7c16,"red"=>0xb02e26,"black"=>0x1d1d21,_=>bail!("invalid banner dye: {color}")})}
fn baked_condition(condition:&str,props:&BTreeMap<String,String>)->bool {
    condition.is_empty()||condition.split('|').any(|group|group.split(',').all(|clause|clause.split_once('=').is_some_and(|(key,expected)|props.get(key).is_some_and(|actual|expected==actual||matches!(expected,"True"|"False")&&expected.eq_ignore_ascii_case(actual)))))
}
impl BlockDatabase {
    /// Exact effective resource scope is pinned by LayerScene; this API never uses an item icon.
    pub fn layer_top_sprite(&self,version:&str,state:&str)->std::result::Result<RgbaImage,LayerVisualError>{
        self.render_plane(version,state,Plane::Top).map(|out|out.image).map_err(LayerVisualError::from_error)
    }
    fn baked_models(&self,version:&str,id:&str,props:&BTreeMap<String,String>)->Result<SelectedPlaneModels>{
        let resource=self.resources.get(&self.version(version)?.resource_version).context("missing version resources: recorded baked source absent")?;
        let block=resource.blocks.get(id).with_context(||{let (ns,local)=id.split_once(':').unwrap_or(("minecraft",id));format!("selected blockstate missing: assets/{ns}/blockstates/{local}.json")})?;
        let mut variants=Vec::new();if let Some(variant)=block.variants.iter().find(|v|baked_condition(&v.condition,props)){variants.push(variant);}
        variants.extend(block.multipart.iter().filter(|v|baked_condition(&v.condition,props)));
        if variants.is_empty(){bail!("invalid state: baked model has no matching variant: {id} {props:?}");}
        variants.into_iter().map(|v|{
            let mut elements=self.models.get(v.model).context("selected model missing: baked model index")?.clone();
            let mut names=Vec::new();let mut indices=BTreeMap::new();
            for e in &mut elements{for f in &mut e.faces{let old=f.texture;f.texture=*indices.entry(old).or_insert_with(||{let index=names.len() as u32;names.push(self.strings.get(old as usize).cloned().unwrap_or_default());index});}}
            Ok((v.clone(),elements,names))
        }).collect()
    }
    fn plane_texture(&self,context:&crate::layer_resources::LayerResourceContext,version:&str,name:&str)->Result<Arc<RgbaImage>>{
        let name=namespaced(name);let(ns,local)=name.split_once(':').context("texture namespace missing")?;
        let key=format!("assets/{ns}/textures/{local}.png");
        match selected_texture(context,&name) {
            Ok(texture)=>return Ok(texture),
            Err(error) if error.to_string().starts_with("selected texture missing:")=>{},
            Err(error)=>return Err(error),
        }
        if !context.allows_baked_fallback(){bail!("selected texture missing: {key}");}
        // Only the recorded vanilla source may use baked assets; explicit imported JAR omissions stay authoritative.
        let resource=self.resources.get(&self.version(version)?.resource_version).context("missing version resources")?;
        let index=resource.textures.get(&name).or_else(||(ns=="minecraft").then(||resource.textures.get(local)).flatten()).with_context(||format!("selected texture missing: {key}"))?;
        let t=self.textures.get(*index).context("selected texture missing: baked descriptor")?;
        let end=t.offset.checked_add(t.len).context("texture byte range overflow")?;
        let pixels=self.pixels.get(t.offset..end).context("texture decode failed: baked byte range")?;
        Ok(Arc::new(RgbaImage::from_raw(u32::from(t.width),u32::from(t.height),pixels.to_vec()).context("texture decode failed: baked dimensions")?))
    }
    pub(super) fn render_plane(&self,version:&str,state:&str,plane:Plane)->Result<PlaneOutput>{
        let definition=self.version(version)?;let(id,props)=crate::version_conversion::parse_key(state)?;
        let canonical=if props.is_empty(){id.clone()}else{format!("{id}[{}]",props.iter().map(|(k,v)|format!("{k}={v}")).collect::<Vec<_>>().join(","))};
        let registry=self.registry(version)?;
        // Legacy canonical IDs use their recorded raw aliases, never latest-version texture names.
        if let Some(block)=registry.blocks.get(&id){if !block.accepts(&canonical,&props){bail!("invalid state: {state}");}}
        let context=crate::layer_resources::current(version)?;
        let (mut model_id,mut model_props)=(id.clone(),props.clone());
        if definition.legacy {
            let alias=registry.legacy_state_aliases.iter().find(|(_,mapped)|mapped.contains(&canonical)).map(|(alias,_)|crate::version_conversion::parse_key(alias)).transpose()?;
            let path=|id:&str|{let(ns,local)=id.split_once(':').unwrap_or(("minecraft",id));format!("assets/{ns}/blockstates/{local}.json")};
            // Old state mappers selected a distinct resource file for each variant/color.
            // Prefer the precise canonical file (granite is NOT stone[variant=granite]).
            // The recorded tallgrass alias disambiguates its modern `grass` ID from old grass-block.
            let mut selector=None;
            if let Some((old,values))=&alias {if old=="minecraft:tallgrass"{selector=values.get("type").map(|value|format!("minecraft:{value}"));}}
            if props.get("type").is_some_and(|value|value=="double") {if let Some(base)=id.strip_suffix("_slab"){selector=Some(format!("{base}_double_slab"));}}
            let selector=if let Some(name)=selector{context.read(&path(&name))?.is_some().then_some(name)}else{None};
            if let Some(selector)=selector {
                model_id=selector;model_props=BTreeMap::new();
            }else if context.read(&path(&id))?.is_some(){
                if id.ends_with("_slab"){if let Some(kind)=model_props.remove("type"){if kind=="top"||kind=="bottom"{model_props.insert("half".into(),kind);}}}
            }else if let Some((old,values))=alias {if context.read(&path(&old))?.is_some(){model_id=old;model_props=values;}}
        }
        let(ns,local)=model_id.split_once(':').context("invalid block namespace")?;
        let key=format!("assets/{ns}/blockstates/{local}.json");
        let native=id=="minecraft:water"||id=="minecraft:lava"||self.special_plane(version,&id,&props).is_some()||id.ends_with("shulker_box")||id.ends_with("_head")||id.ends_with("_skull")||id.ends_with("_banner")||id=="minecraft:decorated_pot";
        let selected=if context.read(&key)?.is_some(){selected_plane_models(context.as_ref(),&model_id,&model_props,definition.legacy)?}
            else if native{Arc::new(vec![(Variant{condition:String::new(),model:0,x:0,y:0,uvlock:false,builtin:id!="minecraft:water"&&id!="minecraft:lava"},vec![],vec![])])}
            else if context.allows_baked_fallback(){Arc::new(self.baked_models(version,&id,&props)?)}else{bail!("selected blockstate missing: {key}")};
        let mut output=PlaneOutput{image:RgbaImage::new(32,32),tints:BTreeSet::new(),faces:Vec::new()};let mut surfaces=Vec::new();
        for (variant,elements,strings) in selected.iter(){
            if elements.is_empty()&&(variant.builtin||self.special_plane(version,&id,&props).is_some()||id.ends_with("shulker_box")||id.ends_with("_head")||id.ends_with("_skull")||id.ends_with("_banner")||id=="minecraft:conduit"||id=="minecraft:decorated_pot"||id=="minecraft:dragon_egg"){
                self.builtin_surfaces(context.as_ref(),version,&id,&props,plane,&mut surfaces,&mut output)?;continue;
            }
            for e in elements {for f in &e.faces {
                if !facing(e,variant,f.side,plane){continue;}
                let name=strings.get(f.texture as usize).context("selected texture missing: model texture index")?;
                let texture=self.plane_texture(context.as_ref(),version,name)?;
                let tint=self.face_tint(version,state,f.tint)?;
                surfaces.push(Surface{quad:corners(e,f.side).map(|p|transform(p,e,variant)),uv:face_uv(f,variant),texture,tint});
                if f.tint>=0{output.tints.insert(f.tint);}
                output.faces.push(serde_json::json!({"texture":name,"tintIndex":f.tint,"side":f.side,"rotationX":u16::from(variant.x)*90,"rotationY":u16::from(variant.y)*90,"uvlock":variant.uvlock,"forceTranslucent":f.force_translucent}));
            }}
        }
        if id=="minecraft:water"||id=="minecraft:lava"||props.get("waterlogged").is_some_and(|v|v=="true"){
            let water=id!="minecraft:lava";let stem=if water{"water_still"}else{"lava_still"};
            let modern=format!("assets/minecraft/textures/block/{stem}.png");
            let texture=if context.read(&modern)?.is_some(){format!("minecraft:block/{stem}")}else{format!("minecraft:blocks/{stem}")};
            let level=props.get("level").and_then(|v|v.parse::<u8>().ok()).unwrap_or(0);
            let height=if level>=8{16.*8./9.}else{16.*f32::from(8-level)/9.};
            let e=Element{from:[0.,0.,0.],to:[16.,height,16.],faces:vec![],rotation:None};
            let v=Variant{condition:String::new(),model:0,x:0,y:0,uvlock:false,builtin:false};
            let f=Face{side:if plane==Plane::Top{1}else{2},texture:0,uv:[0.,0.,16.,16.],tint:-1,rotation:0,force_translucent:false};
            let tint=if water {self.water_tint(version,context.as_ref())?}else{0xffffffff};
            surfaces.push(Surface{quad:corners(&e,f.side),uv:face_uv(&f,&v),texture:self.plane_texture(context.as_ref(),version,&texture)?,tint});
            output.faces.push(serde_json::json!({"texture":texture,"rule":"FluidRenderer still surface","side":f.side}));
        }
        output.image=raster(&surfaces,plane)?;Ok(output)
    }
    fn water_tint(&self,version:&str,context:&impl ResourceAccess)->Result<u32>{
        if self.version(version)?.data_version<1519 {return Ok(0xffffffff);}
        if let Some(bytes)=context.read("data/minecraft/worldgen/biome/plains.json")? {
            let biome:serde_json::Value=serde_json::from_slice(&bytes).context("biome decode failed: plains water tint")?;
            if let Some(color)=biome.pointer("/effects/water_color").and_then(serde_json::Value::as_u64){return Ok(0xff000000|u32::try_from(color).context("water tint overflow")?);}
        }
        let data=plains_tint_metadata()?;
        let table=data.versions.get(version).and_then(|key|data.tables.get(key)).context("missing version water tint metadata")?;
        table.get("minecraft:water_cauldron[level=1]|0").copied().context("selected texture missing: actual plains water tint metadata")
    }
    fn special_plane(&self,version:&str,id:&str,props:&BTreeMap<String,String>)->Option<&serde_json::Value>{
        let key=if matches!(id,"minecraft:chest"|"minecraft:trapped_chest"|"minecraft:ender_chest"){format!("{id}[{}]",props.get("type").map(String::as_str).unwrap_or("single"))}else{id.to_owned()};
        self.supplement.special_planes.get(version)?.get(&key)
    }
    fn builtin_surfaces(&self,context:&crate::layer_resources::LayerResourceContext,version:&str,id:&str,props:&BTreeMap<String,String>,plane:Plane,out:&mut Vec<Surface>,output:&mut PlaneOutput)->Result<()> {
        let special=self.special_plane(version,id,props);
        // Historical special-plane PNGs are front elevations only, never top projections.
        if plane==Plane::Side {if let Some(special)=special {
            let bytes=base64::engine::general_purpose::STANDARD.decode(special["png"].as_str().context("special model PNG missing")?)?;
            let texture=Arc::new(image::load_from_memory(&bytes)?.to_rgba8());
            let e=Element{from:[0.;3],to:[16.;3],faces:vec![],rotation:None};
            out.push(Surface{quad:corners(&e,2),uv:[[0.,0.],[16.,0.],[16.,16.],[0.,16.]],texture,tint:0xffffffff});
            output.faces.push(serde_json::json!({"texture":special["texture"],"rule":special["rule"]}));return Ok(());
        }}
        let local=id.strip_prefix("minecraft:").context("unsupported builtin model namespace")?;
        // Dedicated vanilla entity layers feed the SAME projection/depth/alpha raster.
        if local=="decorated_pot" {
            let name="minecraft:entity/decorated_pot/decorated_pot_base";
            let texture=self.plane_texture(context,version,name)?;
            let yaw=match props.get("facing").map(String::as_str){Some("south")=>180.,Some("west")=>90.,Some("east")=>270.,_=>0.};
            let orient=|mut p:[f32;3]|{p[0]-=8.;p[2]-=8.;rotate(&mut p,1,yaw);p[0]+=8.;p[2]+=8.;p};
            // DecoratedPotRenderer.createBaseLayer: neck pose offset(0,37,16), X=PI.
            let neck=|p:[f32;3]|orient([p[0],37.-p[1],16.-p[2]]);
            entity_box(out,plane,[4.,17.,4.],[8.,3.,8.],[0.,0.],[32.,32.],-0.1,false,&texture,0xffffffff,&neck);
            entity_box(out,plane,[5.,20.,5.],[6.,1.,6.],[0.,5.],[32.,32.],0.2,false,&texture,0xffffffff,&neck);
            // The zero-height top plane carries the real interior/rim texture under the neck.
            entity_box(out,plane,[0.;3],[14.,0.,14.],[-14.,13.],[32.,32.],0.,false,&texture,0xffffffff,&|p|orient([p[0]+1.,p[1]+16.,p[2]+1.]));
            output.faces.push(serde_json::json!({"texture":name,"rule":"DecoratedPotRenderer neck/top LayerDefinition","side":1}));return Ok(());
        }
        if local.ends_with("_banner") {
            let wood_name="minecraft:entity/banner_base";let cloth_name="minecraft:entity/banner/base";
            let wood=self.plane_texture(context,version,wood_name)?;let cloth=self.plane_texture(context,version,cloth_name)?;
            let wall=local.ends_with("_wall_banner");
            let yaw=if wall{match props.get("facing").map(String::as_str){Some("south")=>0.,Some("west")=>90.,Some("east")=>270.,_=>180.}}else{props.get("rotation").and_then(|v|v.parse::<f32>().ok()).unwrap_or(0.)*22.5};
            let map=|p:[f32;3]|{let mut p=[p[0]*2./3.,-p[1]*2./3.,-p[2]*2./3.];if wall{p[1]-=5.;p[2]-=7.;}rotate(&mut p,1,-yaw);[p[0]+8.,p[1]+if wall{-16./6.}else{8.},p[2]+8.]};
            // BannerRenderer: standing pole; bar is shared by standing and wall banners.
            if !wall{entity_box(out,plane,[-1.,-30.,-1.],[2.,42.,2.],[44.,0.],[64.,64.],0.,false,&wood,0xffffffff,&map);}
            entity_box(out,plane,[-10.,-32.,-1.],[20.,2.,2.],[0.,42.],[64.,64.],0.,false,&wood,0xffffffff,&map);
            // No world-time input exists for a state sprite: exact animation phase zero reference pose.
            let flag=|mut p:[f32;3]|{rotate(&mut p,0,(-0.0125+0.01)*180.);p[1]-=32.;map(p)};
            entity_box(out,plane,[-10.,0.,-2.],[20.,40.,1.],[0.,0.],[64.,64.],0.,false,&wood,0xffffffff,&flag);
            let color=local.strip_suffix("_wall_banner").or_else(||local.strip_suffix("_banner")).context("invalid banner color")?;
            let tint=0xff000000|banner_dye(color)?;
            entity_box(out,plane,[-10.,0.,-2.],[20.,40.,1.],[0.,0.],[64.,64.],0.,false,&cloth,tint,&flag);
            output.faces.push(serde_json::json!({"texture":wood_name,"overlay":cloth_name,"rule":"BannerRenderer flag/pole/bar LayerDefinition, phase 0","side":1}));return Ok(());
        }
        if local=="dragon_head"||local=="dragon_wall_head" {
            let name="minecraft:entity/enderdragon/dragon";let texture=self.plane_texture(context,version,name)?;
            let wall=local=="dragon_wall_head";
            let yaw=if wall{match props.get("facing").map(String::as_str){Some("east")=>90.,Some("south")=>180.,Some("west")=>270.,_=>0.}}else{props.get("rotation").and_then(|v|v.parse::<f32>().ok()).unwrap_or(0.)*22.5};
            let offset=if wall{match props.get("facing").map(String::as_str){Some("east")=>[-4.,4.,0.],Some("south")=>[0.,4.,-4.],Some("west")=>[4.,4.,0.],_=>[0.,4.,4.]}}else{[0.;3]};
            // SkullBlockRenderer mirror + DragonHeadModel translation(-0.374375Y), scale(.75).
            let head=|mut p:[f32;3]|{rotate(&mut p,1,yaw);[8.+offset[0]-p[0]*0.75,offset[1]+16.*0.374375-p[1]*0.75,8.+offset[2]+p[2]*0.75]};
            for (from,size,uv,mirror) in [
                ([-6.,-1.,-24.],[12.,5.,16.],[176.,44.],false),
                ([-8.,-8.,-10.],[16.,16.,16.],[112.,30.],false),
                ([-5.,-12.,-4.],[2.,4.,6.],[0.,0.],true),
                ([-5.,-3.,-22.],[2.,2.,4.],[112.,0.],true),
                ([3.,-12.,-4.],[2.,4.,6.],[0.,0.],false),
                ([3.,-3.,-22.],[2.,2.,4.],[112.,0.],false),
            ] {entity_box(out,plane,from,size,uv,[256.,256.],0.,mirror,&texture,0xffffffff,&head);}
            // DragonHeadModel.setupAnim(0): jaw X=(sin(0)+1)*.2 radians.
            entity_box(out,plane,[-6.,0.,-16.],[12.,4.,16.],[176.,65.],[256.,256.],0.,false,&texture,0xffffffff,&|mut p|{rotate(&mut p,0,0.2f32.to_degrees());p[1]+=4.;p[2]-=8.;head(p)});
            output.faces.push(serde_json::json!({"texture":name,"rule":"DragonHeadModel head/lip/scales/nostrils/jaw, phase 0","side":1}));return Ok(());
        }
        let (texture,from,to,atlas,rect)=if matches!(local,"chest"|"trapped_chest"|"ender_chest"){
            let texture=special.and_then(|s|s["texture"].as_str()).context("selected texture missing: chest entity binding")?.to_owned();
            let double=props.get("type").is_some_and(|v|v!="single");let width=if double{15.}else{14.};
            let left=props.get("type").is_some_and(|v|v=="left");
            (texture,[if double&&left{0.}else{1.},10.,1.],[if double&&!left{16.}else{15.},15.,15.],64.,[14.,0.,14.+width,14.])
        }else if local.ends_with("_head")||local.ends_with("_skull")||local.ends_with("_wall_head")||local.ends_with("_wall_skull"){
            let texture=if let Some(texture)=special.and_then(|s|s["texture"].as_str()){texture.to_owned()}else if local.contains("player"){
                let modern="assets/minecraft/textures/entity/player/wide/steve.png";
                if context.read(modern)?.is_some(){modern.to_owned()}else{"assets/minecraft/textures/entity/steve.png".to_owned()}
            }else if local.contains("piglin"){"assets/minecraft/textures/entity/piglin/piglin.png".to_owned()}else{bail!("unsupported builtin head model: {id}");};
            if local.contains("dragon"){bail!("unsupported builtin dragon head geometry: {id}");}
            let wall=local.contains("_wall_");
            (texture,if wall{[4.,4.,8.]}else{[4.,0.,4.]},if wall{[12.,12.,16.]}else{[12.,8.,12.]},64.,[8.,0.,16.,8.])
        }else if local.ends_with("shulker_box"){
            let color=local.strip_suffix("_shulker_box");let texture=match color{Some(c)=>format!("assets/minecraft/textures/entity/shulker/shulker_{c}.png"),None=>"assets/minecraft/textures/entity/shulker/shulker.png".to_owned()};
            (texture,[0.,8.,0.],[16.,16.,16.],64.,[16.,0.,32.,16.])
        }else{bail!("unsupported builtin model: {id}");};
        let name=texture.strip_prefix("assets/").and_then(|s|s.split_once("/textures/")).map(|(ns,local)|format!("{ns}:{}",local.strip_suffix(".png").unwrap_or(local))).unwrap_or(texture.clone());
        let image=self.plane_texture(context,version,&name)?;
        let y=match props.get("facing").map(String::as_str){Some("east")=>1,Some("south")=>2,Some("west")=>3,_=>0};
        let mut e=Element{from,to,faces:vec![],rotation:None};
        if let Some(rotation)=props.get("rotation").and_then(|v|v.parse::<f32>().ok()){e.rotation=Some(ElementRotation{kind:0,origin:[8.,8.,8.],axis:1,angle:-rotation*22.5,rescale:false,vector:[0.;3]});}
        let v=Variant{condition:String::new(),model:0,x:0,y,uvlock:false,builtin:false};
        let atlas_height=if (local.contains("head")||local.contains("skull"))&&image.width()==image.height()*2 {atlas/2.}else{atlas};
        let f=Face{side:1,texture:0,uv:[rect[0]*16./atlas,rect[1]*16./atlas_height,rect[2]*16./atlas,rect[3]*16./atlas_height],tint:-1,rotation:0,force_translucent:false};
        out.push(Surface{quad:corners(&e,1).map(|p|transform(p,&e,&v)),uv:face_uv(&f,&v),texture:image,tint:0xffffffff});
        output.faces.push(serde_json::json!({"texture":name,"rule":"entity model lid/head top UV","side":1}));Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn variant(y:u8,locked:bool)->Variant{Variant{condition:String::new(),model:0,x:0,y,uvlock:locked,builtin:false}}
    fn top(y:f32,rgba:[u8;4])->Surface {
        let e=Element{from:[0.,0.,0.],to:[16.,y,16.],faces:vec![],rotation:None};
        Surface{quad:corners(&e,1),uv:[[0.,0.],[16.,0.],[16.,16.],[0.,16.]],texture:Arc::new(RgbaImage::from_pixel(2,2,image::Rgba(rgba))),tint:0xffffffff}
    }
    #[test]
    fn real_depth_and_straight_alpha_ignore_element_order(){
        let far=top(2.,[255,0,0,255]);let near=top(14.,[0,0,255,128]);
        let a=raster(&[near,far],Plane::Top).unwrap();let b=raster(&[top(2.,[255,0,0,255]),top(14.,[0,0,255,128])],Plane::Top).unwrap();
        assert_eq!(a,b);assert_eq!(a.get_pixel(10,10).0,[127,0,128,255]);
        assert!(raster(&[top(12.,[255,255,255,0])],Plane::Top).unwrap().pixels().all(|p|p[3]==0));
        assert!(raster(&[],Plane::Top).unwrap().pixels().all(|p|p[3]==0));
    }
    #[test]
    fn uvlock_keeps_world_orientation_and_rotation_changes_unlocked_pixels(){
        let e=Element{from:[0.;3],to:[16.;3],faces:vec![],rotation:None};
        let f=Face{side:1,texture:0,uv:[0.,0.,16.,16.],tint:-1,rotation:0,force_translucent:false};
        let texture=Arc::new(RgbaImage::from_fn(2,2,|x,y|image::Rgba([x as u8*255,y as u8*255,0,255])));
        let render=|y,locked|{let v=variant(y,locked);raster(&[Surface{quad:corners(&e,1).map(|p|transform(p,&e,&v)),uv:face_uv(&f,&v),texture:texture.clone(),tint:0xffffffff}],Plane::Top).unwrap()};
        assert_ne!(render(0,false),render(1,false));assert_eq!(render(0,false),render(1,true));
        assert_eq!(render(0,false).get_pixel(1,1).0,[0,0,0,255]);
        assert_eq!(render(0,false).get_pixel(30,30).0,[255,255,0,255]);
    }
    #[test]
    fn tilted_element_uses_all_visible_faces_and_never_flattens_cross_plants(){
        let mut e=Element{from:[3.,0.,3.],to:[13.,16.,13.],faces:vec![],rotation:Some(ElementRotation{kind:0,origin:[8.;3],axis:2,angle:22.5,rescale:false,vector:[0.;3]})};
        let v=variant(0,false);
        let visible:Vec<_>=(0..6).filter(|side|facing(&e,&v,*side,Plane::Top)).collect();assert_eq!(visible,vec![1,5]);
        e.rotation=None;
        assert!(!facing(&e,&v,2,Plane::Top));assert!(!facing(&e,&v,4,Plane::Top));
        assert!(baked_condition("north=True|east=False,north=False",&BTreeMap::from([("north".into(),"true".into())])));
    }
    struct Resources {id:&'static str,files:BTreeMap<String,Vec<u8>>}
    impl ResourceAccess for Resources {fn identity(&self)->&str{self.id}fn read(&self,path:&str)->Result<Option<Vec<u8>>>{Ok(self.files.get(path).cloned())}}
    #[test]
    fn actual_json_parent_indirection_multipart_and_animation_frame_are_resolved(){
        let mut files=BTreeMap::new();
        files.insert("assets/test/models/block/base.json".into(),serde_json::to_vec(&serde_json::json!({"textures":{"base":"test:block/tile","redirect":"#base"},"elements":[{"from":[0,0,0],"to":[16,8,16],"faces":{"up":{"texture":"#redirect","rotation":90}}}]})).unwrap());
        files.insert("assets/test/models/block/child.json".into(),br##"{"parent":"test:block/base","textures":{"base":{"sprite":"#actual","force_translucent":true},"actual":"test:block/override"}}"##.to_vec());
        files.insert("assets/test/blockstates/multipart.json".into(),br##"{"multipart":[{"apply":{"model":"test:block/child","uvlock":true,"y":90}},{"when":{"OR":[{"north":"true"},{"east":"true"}]},"apply":{"model":"test:block/child"}}]}"##.to_vec());
        let strip=RgbaImage::from_fn(2,4,|_,y|image::Rgba(if y<2{[255,0,0,255]}else{[0,255,0,255]}));let mut png=Vec::new();
        image::codecs::png::PngEncoder::new(&mut png).write_image(strip.as_raw(),2,4,image::ExtendedColorType::Rgba8).unwrap();
        files.insert("assets/test/textures/block/override.png".into(),png);
        files.insert("assets/test/textures/block/override.png.mcmeta".into(),br##"{"animation":{"frames":[1,0]}}"##.to_vec());
        let resources=Resources{id:"plane-fixture-parent-multipart-animation-v1",files};
        let models=selected_plane_models(&resources,"test:multipart",&BTreeMap::from([("north".into(),"true".into())]),false).unwrap();
        assert_eq!(models.len(),2);assert!(models[0].0.uvlock);assert_eq!(models[0].0.y,1);
        assert!(models[0].1[0].faces[0].force_translucent);assert_eq!(models[0].1[0].faces[0].rotation,90);assert_eq!(models[0].2[0],"test:block/override");
        let image=selected_texture(&resources,"test:block/override").unwrap();assert_eq!(image.dimensions(),(2,2));assert!(image.pixels().all(|p|p.0==[0,255,0,255]));
    }
    #[test]
    fn exact_version_top_glass_and_real_entity_tops_have_pixels(){
        let db=database().unwrap();
        for version in ["1.21.1","26.3"] {
            for state in ["minecraft:white_stained_glass","minecraft:chest[facing=north,type=single,waterlogged=false]","minecraft:hopper[enabled=true,facing=down]","minecraft:oak_stairs[facing=north,half=bottom,shape=straight,waterlogged=false]","minecraft:water[level=0]"] {
                let image=db.layer_top_sprite(version,state).unwrap_or_else(|e|panic!("{version} {state}: {e}"));
                assert!(image.pixels().any(|p|p[3]>0),"{version} {state}");
            }
        }
    }
    #[test]
    fn representative_real_registry_states_across_legacy_and_current_assets(){
        let db=database().unwrap();let mut all_errors=Vec::new();
        for version in ["1.12.2","1.21","26.3"] {
            let registry=db.registry(version).unwrap();let mut states=BTreeSet::new();
            for candidates in [
                &["minecraft:stone"][..], &["minecraft:white_stained_glass"], &["minecraft:glass_pane"],
                &["minecraft:water"], &["minecraft:oak_stairs"], &["minecraft:stone_slab"], &["minecraft:oak_log"],
                &["minecraft:piston"], &["minecraft:hopper"], &["minecraft:chest"], &["minecraft:oak_fence"],
                &["minecraft:cobblestone_wall"], &["minecraft:iron_bars"], &["minecraft:redstone_wire"],
                &["minecraft:rail"], &["minecraft:poppy"], &["minecraft:short_grass","minecraft:grass"]
            ] {
                let block=candidates.iter().find_map(|id|registry.blocks.get(*id)).expect("recorded family exists in actual registry");states.insert(block.default.clone());
            }
            for (id,key,value) in [
                ("minecraft:oak_log","axis","x"),("minecraft:oak_log","axis","z"),
                ("minecraft:oak_stairs","waterlogged","true"),("minecraft:oak_stairs","facing","west"),
                ("minecraft:stone_slab","type","top"),("minecraft:piston","facing","up"),
                ("minecraft:hopper","facing","west"),("minecraft:redstone_wire","power","15"),
                ("minecraft:rail","shape","ascending_east"),("minecraft:oak_fence","north","true"),
                ("minecraft:cobblestone_wall","north","low"),("minecraft:glass_pane","north","true"),
                ("minecraft:iron_bars","east","true")
            ] {
                let Some(block)=registry.blocks.get(id)else{continue};let(_,mut props)=crate::version_conversion::parse_key(&block.default).unwrap();
                if !block.properties.get(key).is_some_and(|values|values.iter().any(|v|v==value)){continue;}
                props.insert(key.to_owned(),value.to_owned());
                let state=format!("{id}[{}]",props.iter().map(|(k,v)|format!("{k}={v}")).collect::<Vec<_>>().join(","));
                if block.accepts(&state,&props){states.insert(state);}
            }
            assert!((20..=30).contains(&states.len()),"{version}: {} representative states",states.len());
            let mut errors=Vec::new();let mut transparent=0;
            for state in &states {match db.layer_top_sprite(version,state){
                Ok(image)=>{assert_eq!(image.dimensions(),(32,32));if image.pixels().all(|p|p[3]==0){transparent+=1;}}
                Err(error)=>errors.push(format!("{state}: {error}")),
            }}
            eprintln!("{version}: {} real registry states, {transparent} transparent, {} errors",states.len(),errors.len());
            all_errors.extend(errors.into_iter().map(|error|format!("{version}: {error}")));
        }
        assert!(all_errors.is_empty(),"{}",all_errors.join("\n"));
    }

    #[test]
    fn legacy_stone_and_granite_keep_distinct_actual_model_files(){
        let db=database().unwrap();let registry=db.registry("1.12.2").unwrap();
        let stone=&registry.blocks["minecraft:stone"].default;
        let granite=&registry.blocks["minecraft:granite"].default;
        let stone=db.layer_top_sprite("1.12.2",stone).unwrap();
        let granite=db.layer_top_sprite("1.12.2",granite).unwrap();
        assert_ne!(stone,granite,"legacy variant selection must not collapse granite into stone");
    }

    #[test]
    fn actual_native_entity_layers_project_pot_dragon_and_banners(){
        let db=database().unwrap();let mut images=Vec::new();
        for state in [
            "minecraft:decorated_pot[cracked=false,facing=south,waterlogged=false]",
            "minecraft:dragon_head[powered=false,rotation=12]","minecraft:dragon_head[powered=false,rotation=4]",
            "minecraft:white_banner[rotation=6]","minecraft:white_banner[rotation=8]",
            "minecraft:white_wall_banner[facing=east]","minecraft:white_wall_banner[facing=north]",
            "minecraft:white_wall_banner[facing=south]","minecraft:white_wall_banner[facing=west]",
        ] {
            let image=db.layer_top_sprite("1.21",state).unwrap_or_else(|error|panic!("{state}: {error}"));
            assert!(image.pixels().any(|p|p[3]>0),"actual entity geometry projected empty: {state}");
            assert!(image.pixels().any(|p|p[3]==0),"entity footprint became fake full cube: {state}");images.push(image);
        }
        assert_ne!(images[1],images[2],"dragon rotation must transform head/lip footprint");
        assert_ne!(images[3],images[4],"banner rotation must transform support/cloth footprint");
        assert_ne!(images[5],images[7],"wall banners must stay on opposite physical wall edges");
    }

}
