#!/usr/bin/env python3
"""Extract per-release serializable brain memory codecs and proven owner slots.

For static providers, only take fields that actually feed the typed memory list
or an inline Brain.provider call. Activities-driven brains additionally have a
few verified requirement paths (e.g. Villager -> rest package -> SleepInBed).
Do not infer memory ownership from the global registry or a spawn egg.
"""
import argparse, hashlib, json, pathlib, re, subprocess
from concurrent.futures import ThreadPoolExecutor
from version_entity_traits import classmap

ROOT = pathlib.Path(__file__).resolve().parents[3]
OWNERS = {n: re.sub(r'(?<!^)(?=[A-Z])','_',n).lower() for n in
          'Villager Piglin PiglinBrute Hoglin Zoglin Axolotl Goat Allay Frog Tadpole Warden Sniffer Camel Armadillo Breeze Creaking CopperGolem Nautilus ZombieNautilus CamelHusk'.split()}
# Verified activity chains on named, unobfuscated releases. Check each exact
# JAR for the package's behavior instantiation AND that behavior's required
# slots, not a mere ID reference in an unrelated entity.
CHAINS = [
    ('Villager','VillagerGoalPackages','SleepInBed',('home','last_woken','last_slept')),
    ('Villager','VillagerGoalPackages','AssignProfessionFromJobSite',('potential_job_site','job_site')),
    ('Villager','VillagerGoalPackages','SetWalkTargetFromBlockMemory',()), # parameterized; do not infer slots
    ('Axolotl','AxolotlAi','ValidatePlayDead',('play_dead_ticks',)),
    ('Piglin','PiglinAi','StopBeingAngryIfTargetDead',('angry_at',)),
    ('Piglin','PiglinAi','StartHuntingHoglin',('angry_at','hunted_recently')),
    ('Piglin','PiglinAi','StartAdmiringItemIfSeen',('admiring_item','admiring_disabled')),
    ('Breeze','BreezeAi','LongJump',('breeze_jump_cooldown','breeze_jump_inhaling','breeze_jump_target','breeze_shoot','breeze_leaving_water')),
    ('Breeze','BreezeAi','Shoot',('breeze_shoot_cooldown','breeze_shoot_charging','breeze_shoot_recover','breeze_shoot','breeze_jump_target')),
    ('Warden','WardenAi','Roar',('roar_sound_cooldown','roar_sound_delay')),
    ('Warden','WardenAi','TryToSniff',('sniff_cooldown','is_sniffing')),
    ('Warden','WardenAi','Sniffing',('is_sniffing',)),
    ('Warden','WardenAi','Digging',('dig_cooldown',)),
    ('PiglinBrute','PiglinBruteAi','StopBeingAngryIfTargetDead',('angry_at',)),
]

def javap(jar, classes, verbose=False):
    return subprocess.check_output(['javap','-p','-v' if verbose else '-c','-classpath',str(jar),*classes],text=True,errors='replace')

def sections(output):
    parts = {}
    key = None
    for line in output.splitlines():
        m = re.match(r'^(?:[\w ]* )?class ([.$\w]+)', line)
        if m:
            key=m[1]; parts[key]=[]
        elif key:
            parts[key].append(line)
    return {k:'\n'.join(v) for k,v in parts.items()}

def generate_entry(v, sha, jar, cache):
    if hashlib.sha256(jar.read_bytes()).hexdigest()!=sha:
        raise ValueError(f'{v} JAR identity mismatch')
    m=classmap(cache,v,jar)
    def named(*short):
        values=[(name,cl) for name,cl in m.items() if name.rsplit('.',1)[-1] in short]
        return values[0][1] if len(values)==1 else None
    mem=named('MemoryModuleType')
    aliases={o.replace('.','/'):name.rsplit('.',1)[-1] for name,o in m.items()}
    props=[(k,named(k,'Entity'+k,k+'Entity')) for k in OWNERS]
    props=[(k,cl) for k,cl in props if cl]
    helpers={k:named(k+'Ai') for k,cl in props}

    chain_classes = {k:named(k) for _,ai,beh,_ in CHAINS for k in (ai,beh)} if v.startswith('26.') else {}
    classes=[mem,*[cl for _,cl in props],*[cl for cl in helpers.values() if cl],*[cl for cl in chain_classes.values() if cl]]
    by_class=sections(javap(jar, list(dict.fromkeys(classes))))
    boot=by_class[mem].split('static {};')[-1]
    vintage=v.startswith(('1.14','1.15'))
    legacy_bsm={}
    if vintage:
        extra=javap(jar,[mem],True).split('BootstrapMethods:')[-1]
        for idx,part in enumerate(re.split(r'(?m)^  \d+:',extra)[1:]):
            impl=re.search(r'REF_invokeStatic ([.$/\w]+)\.\w+:\(Lcom/mojang/datafixers/Dynamic;\)',part)
            if impl:
                legacy_bsm[idx]=aliases.get(impl[1])
    ids, fields={},{}
    for segment in re.split(r'(?=^ *\d+: ldc(?:_w)? .*// String )',boot,flags=re.M):
        ident=re.match(r'^ *\d+: ldc(?:_w)? .*// String ([a-z0-9_]+)\n',segment)
        if not ident:continue
        key=ident[1]
        field=re.search(r'putstatic[^\n]+// Field (\w+):',segment)
        if not field:raise ValueError((v,key,'unbound registry field'))
        fields[field[1]]=key
        codec='none'
        if vintage and 'Optional.of:' in segment:
            index=int(re.search(r'// InvokeDynamic #(\d+):',segment)[1])
            codec={'GlobalPos':'global','SerializableLong':'long'}.get(legacy_bsm.get(index),'other')
        elif 'Ljava/lang/String;Lcom/mojang/serialization/Codec;' in segment:
            codec='other'
            primitive=re.search(r'// Field com/mojang/serialization/Codec\.(BOOL|INT|LONG):',segment)
            source=re.search(r'getstatic[^\n]+// Field ([\w/$]+)\.(\w+):',segment)
            if primitive:
                codec=primitive[1].lower()
            elif source:
                owner=aliases.get(source[1])
                codec={'GlobalPos':'global','UUIDUtil':'uuid','SerializableUUID':'uuid','BlockPos':'pos','Unit':'unit'}.get(owner,'other')
                if 'Codec.list:' in segment or 'Codec.listOf:' in segment:
                    codec+='[]'
                # xmap to a set/deque has a different contract from a plain list.
                if 'Codec.xmap:' in segment:codec='other'
        ids[key]=codec
    if not ids or ids.get('home')!='global':raise ValueError((v,'missing verified home codec'))
    slots={}
    mem_path=mem.replace('.','/')
    field_ref=re.compile(r'getstatic[^\n]+// Field '+re.escape(mem_path)+r'\.(\w+):')
    inputs=[]
    for k,cl in props:
        inputs.append((k,cl))
        helper=helpers.get(k)
        if helper and helper.replace('.','/') in by_class[cl]:
            inputs.append((k,helper))
    for k,cl in inputs:
        body=by_class[cl]; owner=OWNERS[k]
        slots.setdefault(owner,set())
        # Legacy/modern static ImmutableList<MemoryModuleType<?>> explicitly
        # passed to create the species brain (ignore sensor/activity/other lists).
        names=re.findall(r'(?:ImmutableList|java\.util\.List)<(?:\? extends )?'+re.escape(mem)+r'<\?>> (\w+);',body)
        stat=body.split('static {};')[-1] if 'static {};' in body else ''
        available=[]
        for line in stat.splitlines():
            match=field_ref.search(line)
            if match: available.append(fields[match[1]])
            if 'putstatic' in line:
                field=re.search(r'// Field (\w+):',line)
                if field and field[1] in names:
                    slots[owner].update(available)
                # Modern Sniffer/etc.: immutable list passed directly as the
                # first parameter of Brain.provider(memory, sensors, ...).
                if field and re.search(r'L[\w/$]+\$Provider;',line):
                    before=stat[:stat.index(line)]
                    seq=before[before.rfind('putstatic')+1:]
                    if re.search(r'\.provider:\(Ljava/util/Collection;Ljava/util/Collection;',seq):
                        slots[owner].update(available)
                available=[]
        if v.startswith('26.'):
            # These behaviors declare exactly their constructor-supplied memory
            # as REGISTERED. Only extract an immediate typed constructor operand.
            helper=helpers.get(k)
            if helper and cl!=helper and helper.replace('.','/') in body:
                ai_body=by_class[helper]
                for n in re.finditer(r'getstatic[^\n]+// Field '+re.escape(mem_path)+r'\.(\w+):[^\n]+\n[^\n]+invokespecial[^\n]+CountDownCooldownTicks\."<init>"',ai_body):
                    slots[owner].add(fields[n[1]])
            for species,ai,beh,keys in CHAINS:
                if species!=k or not keys or not chain_classes.get(ai) or not chain_classes.get(beh):continue
                a,b=chain_classes[ai],chain_classes[beh]
                ai_body=by_class[a]
                if a.replace('.','/') not in body or b.replace('.','/') not in ai_body:continue
                if species=='Villager' and ai=='VillagerGoalPackages':
                    for match in re.finditer(r'getstatic[^\n]+// Field '+re.escape(mem_path)+r'\.(\w+):[^\n]+((?:\n[^\n]*){1,8})',ai_body):
                        if fields[match[1]]=='meeting_point' and 'AcquirePoi.create:' in match[2]:
                            slots[owner].add('meeting_point')
                beh_body=by_class[b]
                for member in field_ref.findall(beh_body):
                    if fields[member] in keys: slots[owner].add(fields[member])
    return ids, slots

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--cache',type=pathlib.Path,default=ROOT/'.tmp/game-data/version-runtime')
    p.add_argument('--current-jar',type=pathlib.Path,default=pathlib.Path('D:/.minecraft/versions/26.3-Fabric/26.3-Fabric.jar'));a=p.parse_args()
    catalog=json.loads((ROOT/'tools/viewer-core/assets/version_entity_registry.json').read_bytes())['versions']
    def extract(row):
        dv,v,_,sha=row
        if dv<1952:return {},{}
        jar=a.current_jar if v=='26.3' else pathlib.Path(json.loads((a.cache/v/'runtime.json').read_bytes())['client_jar'])
        ids,slots=generate_entry(v,sha,jar,a.cache)
        print(v,len(ids),sum(len(s) for s in slots.values()),flush=True)
        return ids,slots
    def record(table,key,i):
        spans=table.setdefault(key,[])
        if spans and spans[-1][1]==i-1:spans[-1][1]=i
        else:spans.append([i,i])
    types,owners={},{}
    with ThreadPoolExecutor(max_workers=4) as pool:
        for i,(ids,slots) in enumerate(pool.map(extract,catalog)):
            for key,value in ids.items():record(types,(key,value),i)
            for owner,mem in slots.items():
                for key in mem:
                    if ids.get(key,'none') not in ('none','other'):record(owners,(owner,key),i)
    dest=ROOT/'tools/viewer-core/assets/version_brain_memory.json'
    def rows(table):
        return ',\n'.join(json.dumps([*key,[[catalog[i][0],catalog[j][0]] for i,j in intervals]],separators=(',',':'))
                          for key,intervals in sorted(table.items()))
    dest.write_bytes(('{"types":[\n'+rows(types)+'\n],"owners":[\n'+rows(owners)+'\n]}\n').encode())

if __name__=='__main__':main()
