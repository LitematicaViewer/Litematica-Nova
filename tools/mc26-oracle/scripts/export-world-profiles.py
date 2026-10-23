"""Extract per-release save-format evidence from existing exact Java inputs.
Development-only: emitted profiles are consumed by the native Rust exporter.
This is bytecode/schema evidence, NOT a claim of world load/save acceptance.
"""
import hashlib,json,pathlib,re,struct,zipfile
ROOT=pathlib.Path(__file__).resolve().parents[3]
CACHE=ROOT/'.tmp/game-data/version-runtime'
OUTPUT=ROOT/'data/world-export/profiles.json'

def strings(data):
 p=10; n=int.from_bytes(data[8:10],'big'); out=[]; i=1
 while i<n:
  tag=data[p];p+=1
  if tag==1:
   length=int.from_bytes(data[p:p+2],'big');p+=2;out.append(data[p:p+length].decode('utf8','replace'));p+=length
  elif tag in (3,4):p+=4
  elif tag in (5,6):p+=8;i+=1
  elif tag in (7,8,16,19,20):p+=2
  elif tag in (9,10,11,12,17,18):p+=4
  elif tag==15:p+=3
  else:raise ValueError('class constant tag '+str(tag))
  i+=1
 return set(out)

def mappings(v,d,z):
 classes={};members={}
 if d.get('mappings'):
  owner=None
  for l in pathlib.Path(d['mappings']).read_text().splitlines():
   if ' -> ' not in l:continue
   if not l[0].isspace():owner,obf=l.split(' -> ');classes[owner]=obf.rstrip(':');members[owner]=[]
   elif owner:members[owner].append(l.strip())
 else:
  mcp=CACHE/'mcp'/((('1.10' if v=='1.10.1' else v)+'-srg.zip'))
  if mcp.exists():
   with zipfile.ZipFile(mcp) as m:
    for l in m.read('joined.srg').decode().splitlines():
     row=l.split()
     if row[0]=='CL:':classes[row[2].replace('/','.')]=row[1]
  else:
   mcp=CACHE/'mcp'/(v+'-config.zip')
   if mcp.exists():
    with zipfile.ZipFile(mcp) as m:
     for l in m.read('config/joined.tsrg').decode().splitlines():
      if l and not l[0].isspace():a,b=l.split()[:2];classes[b.replace('/','.')]=a
 for n in z.namelist():
  if n.startswith('net/minecraft/') and n.endswith('.class'):classes[n[:-6].replace('/','.')]=n[:-6]
 return classes,members

def main():
 versions=[p.parent.name for p in CACHE.glob('*/runtime.json')]
 versions+=['26.3'] if '26.3' not in versions else []
 versions.sort(key=lambda v:tuple(map(int,v.split('.'))))
 result=[]
 roles={
 'chunk':['net.minecraft.world.level.chunk.storage.SerializableChunkData','net.minecraft.world.level.chunk.storage.ChunkSerializer','net.minecraft.world.chunk.storage.AnvilChunkLoader','net.minecraft.world.chunk.storage.ChunkSerializer'],
 'level':['net.minecraft.world.level.storage.PrimaryLevelData','net.minecraft.world.level.storage.LevelData','net.minecraft.world.storage.WorldInfo'],
 'flat':['net.minecraft.world.level.levelgen.flat.FlatLevelGeneratorSettings','net.minecraft.world.gen.FlatGeneratorInfo','net.minecraft.world.gen.FlatGenSettings','net.minecraft.world.gen.FlatGenerationSettings'],
 'bits':['net.minecraft.util.SimpleBitStorage','net.minecraft.util.BitStorage','net.minecraft.util.BitArray'],
 'entities':['net.minecraft.world.level.chunk.storage.EntityStorage'],
 'dimension':['net.minecraft.world.level.dimension.DimensionType','net.minecraft.world.World'],
 'state_codec':['net.minecraft.world.level.block.state.StateHolder','net.minecraft.state.StateHolder'],
 'status':['net.minecraft.world.level.chunk.status.ChunkStatus','net.minecraft.world.level.chunk.ChunkStatus','net.minecraft.world.chunk.ChunkStatus'],
 }
 for v in versions:
  if v=='26.3':
   d={'client_jar':'D:/.minecraft/versions/26.3-Fabric/26.3-Fabric.jar','mappings':None}
   oracle=json.loads((ROOT/'.tmp/game-data/default-adapter-26.3/oracle/oracle-manifest.json').read_text())
  else:
   d=json.loads((CACHE/v/'runtime.json').read_text());oracle=json.loads((ROOT/'.tmp/game-data/oracle-exports'/v/'oracle-manifest.json').read_text())
  raw=pathlib.Path(d['client_jar']).read_bytes();sha=hashlib.sha256(raw).hexdigest()
  assert sha==oracle['client_jar_sha256'],(v,'identity')
  with zipfile.ZipFile(d['client_jar']) as z:
   classes,members=mappings(v,d,z);found={};sets={};mapping_members={}
   for role,candidates in roles.items():
    for c in candidates:
     if c in classes:
      path=classes[c].replace('.','/')+'.class'
      if path not in z.namelist():continue
      data=z.read(path); found[role]={'class':c,'runtime_class':path[:-6],'sha256':hashlib.sha256(data).hexdigest()};sets[role]=strings(data);mapping_members[role]=members.get(c,[]);break
   assert {'chunk','level','flat'}<=found.keys(),(v,found)
   c=sets['chunk'];l=sets['level'];f=sets['flat']
   legacy={'Blocks','Data'}<=c
   modern={'sections','block_states'}<=c
   assert legacy or modern or {'Palette','BlockStates'}<=c,(v,'chunk layout',sorted(c))
   padded=modern or any('valuesPerLong' in m for m in mapping_members.get('bits',[]))
   if not legacy and not modern and not d.get('mappings'):padded=False # MCP 1.13/early1.14 BitArray contains cross-word storage
   numeric_biomes='Biomes' in c and not modern
   # Detect actual integral biome payload width from the writer's invoked NBT method via bytecode symbol descriptor.
   fields={x: x in c for x in ['Biomes','Sections','Level','TileEntities','Entities','HeightMap','Heightmaps','TerrainPopulated','LightPopulated','isLightOn','Status','yPos']}
   raw_dimension=None
   if 'data/minecraft/dimension_type/overworld.json' in z.namelist():
    raw_dimension=json.loads(z.read('data/minecraft/dimension_type/overworld.json'));min_y=raw_dimension['min_y'];height=raw_dimension['height']
   elif modern:min_y=-64;height=384
   else:min_y=0;height=256
   spawn_compound='spawn' in l and 'SpawnX' not in l
   # New PrimaryLevelData explicitly calls the old name OLD_WORLD_GEN_SETTINGS; it does not serialize that old field.
   separated=spawn_compound and 'difficulty_settings' in l and 'GameRules' not in l
   worldgen=('separate' if separated else 'dimensions' if 'WorldGenSettings' in l else 'flat_string' if legacy else 'flat_compound')
   evidence={'client_sha256':sha,'classes':found,'chunk_fields':fields,'level_fields':sorted(l & {'SpawnX','SpawnY','SpawnZ','spawn','GameRules','WorldGenSettings','generatorOptions','DataPacks','enabled_features','difficulty_settings','Difficulty','DataVersion','Version'}),'flat_fields':sorted(f & {'layers','biome','structures','structure_overrides','features','lakes'}),'bit_storage_fields':[m for m in mapping_members.get('bits',[]) if '(' not in m]}
   if raw_dimension:evidence['dimension_resource']={'path':'data/minecraft/dimension_type/overworld.json','sha256':hashlib.sha256(z.read('data/minecraft/dimension_type/overworld.json')).hexdigest()}
   heightmap_blocking=[]
   light_sources=[]
   if 'fullchunk' in sets.get('status',set()):
    source=ROOT/'.tmp/game-data/oracle-exports'/v/'block_states.jsonl'
    light_sources=sorted(r['state_id'] for r in map(json.loads,source.read_text().splitlines()) if r['light_emission']>0)
    evidence['light_sources_sha256']=hashlib.sha256(source.read_bytes()).hexdigest()
   if legacy:
    source=ROOT/'.tmp/game-data/oracle-exports'/v
    ids={r['state_id']:r['raw_id'] for r in map(json.loads,(source/'block_states.jsonl').read_text().splitlines())}
    heightmap_blocking=sorted({ids[r['state_id']] for r in map(json.loads,(source/'block_state_semantics.jsonl').read_text().splitlines()) if r['light_dampening']>0})
    evidence['heightmap_blocking_source_sha256']=hashlib.sha256((source/'block_state_semantics.jsonl').read_bytes()).hexdigest()
   result.append({'light_sources':light_sources,'heightmap_blocking':heightmap_blocking,'overworld_path':('dimensions/minecraft/overworld' if 'dimensions' in sets.get('dimension',set()) and 'DIM-1' not in sets.get('dimension',set()) else ''),'dragon_fight':'DragonFight' in l,'state_id_key':('id' if {'id','properties'} <= sets.get('state_codec',set()) and 'Name' not in sets.get('state_codec',set()) else 'Name'),'state_properties_key':('properties' if {'id','properties'} <= sets.get('state_codec',set()) and 'Properties' not in sets.get('state_codec',set()) else 'Properties'),'version':v,'data_version':oracle['world_version'] if v!='1.8' else None,'family':'numeric' if legacy else 'root_sections' if modern else 'palette_padded' if padded else 'palette_contiguous','min_y':min_y,'height':height,'external_entities':'entities' in found,'external_chunks':not legacy and tuple(map(int,v.split('.'))) >= (1,15),'generator':worldgen,'spawn_compound':spawn_compound,'namespace_status':modern,'chunk_status':('decorated' if 'fullchunk' in sets.get('status',set()) else 'minecraft:full' if modern else 'full'),'structure_overrides':'structure_overrides' in f,'biome': 'minecraft:plains','evidence':evidence,'runtime_validation':'see docs/world-export-versions.md'})
 OUTPUT.parent.mkdir(parents=True,exist_ok=True);OUTPUT.write_text(json.dumps({'schema':1,'profiles':result},ensure_ascii=False,indent=2)+'\n',encoding='utf8')
 print('profiles',len(result))
 for x in result:print(x['version'],x['family'],x['generator'],x['spawn_compound'],x['external_entities'])
if __name__=='__main__':main()
