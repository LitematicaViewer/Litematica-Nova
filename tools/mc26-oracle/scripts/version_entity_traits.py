"""Offline JVM evidence used by generate-version-entity-registry.py."""
import re
import struct
import subprocess
import zipfile


def is_subclass(jar, name, base):
    # Read only this_class / super_class; no game code executes.
    while name != "java/lang/Object":
        if name == base:
            return True
        data = jar.read(name + ".class")
        assert data[:4] == b"\xca\xfe\xba\xbe"
        n = int.from_bytes(data[8:10], "big")
        pool = [None] * n
        offset, i = 10, 1
        while i < n:
            tag = data[offset]
            offset += 1
            if tag == 1:
                size = int.from_bytes(data[offset:offset+2], "big")
                pool[i] = data[offset+2:offset+2+size]
                offset += size + 2
            elif tag == 7:
                pool[i], = struct.unpack_from(">H", data, offset)
                offset += 2
            else:
                offset += {3:4, 4:4, 5:8, 6:8, 8:2, 9:4, 10:4, 11:4, 12:4,
                           15:3, 16:2, 17:4, 18:4, 19:2, 20:2}[tag]
            i += 2 if tag in (5, 6) else 1
        superclass, = struct.unpack_from(">H", data, offset+4)
        name = pool[pool[superclass]].decode()
    return False


def living_ids(src, actual_jar, cache, present):
    package = "net.minecraft.world.entity"
    folder = src / package.replace(".", "/")
    fields = (folder/"EntityTypes.java").read_text(encoding="utf-8")
    symbols = dict(re.findall(r'ResourceKey<EntityType<\?>> (\w+) = create\("([^"]+)"\)',
                              (folder/"EntityTypeIds.java").read_text(encoding="utf-8")))
    imports = {s.rsplit(".", 1)[1]: s for s in re.findall(r"^import ([\w.]+);", fields, re.M)}
    types = {}
    with zipfile.ZipFile(actual_jar) as jar:
        for t, field in re.findall(r'public static final EntityType<([^>]+)> \w+ = register\(\s*EntityTypeIds\.(\w+)', fields):
            first, *nested = t.split(".")
            name = "$".join([imports.get(first, package+"."+first), *nested]).replace(".", "/")
            types["minecraft:" + symbols[field]] = is_subclass(jar, name, "net/minecraft/world/entity/LivingEntity")
    if set(types) != present:
        raise ValueError("26.3 entity types of source and oracle differ")
    # The only canonical rows absent from 26.3. Obtain actual inheritance
    # from the release where each was still registered, not an egg/category.
    for v, pairs in {
        "1.8": {"Mob":"net.minecraft.entity.EntityLiving", "Monster":"net.minecraft.entity.monster.EntityMob"},
        "1.21.1": {"minecraft:boat":"net.minecraft.world.entity.vehicle.Boat",
                   "minecraft:chest_boat":"net.minecraft.world.entity.vehicle.ChestBoat",
                   "minecraft:potion":"net.minecraft.world.entity.projectile.ThrownPotion"},
        "1.21.2": {"minecraft:creaking_transient":"net.minecraft.world.entity.monster.creaking.CreakingTransient"},
    }.items():
        if v == "1.8":
            with zipfile.ZipFile(cache/"mcp/1.8-srg.zip") as z:
                mapped = {name.replace("/", "."): obf for obf, name in re.findall(
                    r"^CL: (\S+) (\S+)", z.read("joined.srg").decode(), re.M)}
            base = mapped["net.minecraft.entity.EntityLivingBase"]
        else:
            mapped = dict(re.findall(r"^(\S+) -> (\S+):", (cache/v/"client-mappings.txt").read_text(encoding="utf-8"), re.M))
            base = mapped["net.minecraft.world.entity.LivingEntity"]
        with zipfile.ZipFile(cache/v/"client.jar") as jar:
            for key, name in pairs.items():
                types[key] = is_subclass(jar, mapped[name], base)
    return types

# Semantics reviewed against the class-specific accessor/storage in 26.3
# (Bat resting flags; Dolphin fish/moistness; Axolotl bucket origin;
# Bee nectar, sting and hive counters; Piglin hunting prohibition).
# DFU reviewed by species/field (e.g. goat horn introduction and ocelot split
# stay separate; bucket-stack Variant is not an entity rewrite). Exclude fields with
# changing tags, ranges, codecs or compound semantics. Presence below is NOT
# inferred from existence of the class: each release must both read AND write
# the key directly using its own typed CompoundTag/Value{Input,Output} methods.
SCALAR_FIELDS = {
    "bat": ("Bat", {"BatFlags":"B"}),
    "dolphin": ("Dolphin", {"GotFish":"Z", "Moistness":"I"}),
    "axolotl": ("Axolotl", {"FromBucket":"Z"}),
    "bee": ("Bee", {"HasNectar":"Z", "HasStung":"Z", "TicksSincePollination":"I",
                    "CannotEnterHiveTicks":"I", "CropsGrownSincePollination":"I"}),
    "piglin": ("Piglin", {"CannotHunt":"Z"}),
    # Fox flags denote the three independent persisted poses, not temporary
    # ticks or a coat index. Sheep/Bogged use distinct case-sensitive wire IDs.
    "fox": ("Fox", {"Sleeping":"Z", "Sitting":"Z", "Crouching":"Z"}),
    "sheep": ("Sheep", {"Sheared":"Z"}),
    "bogged": ("Bogged", {"sheared":"Z"}),
    "hoglin": ("Hoglin", {"IsImmuneToZombification":"Z", "CannotBeHunted":"Z", "TimeInOverworld":"I"}),
    "zoglin": ("Zoglin", {"IsBaby":"Z"}),
    "skeleton_horse": ("SkeletonHorse", {"SkeletonTrap":"Z", "SkeletonTrapTime":"I"}),
    "camel": ("Camel", {"LastPoseTick":"J"}),
    "mannequin": ("Mannequin", {"immovable":"Z", "hide_description":"Z"}),
    "rabbit": ("Rabbit", {"MoreCarrotTicks":"I"}),
    "endermite": ("Endermite", {"Lifetime":"I"}),
    "chicken": ("Chicken", {"IsChickenJockey":"Z", "EggLayTime":"I"}),
    "tadpole": ("Tadpole", {"AgeLocked":"Z"}),
    # Goat horn introduction, old ocelot Trusting/CatType, and wolf Sitting
    # (pose vs order across eras) are left to their dedicated migration.
    "goat": ("Goat", {"IsScreamingGoat":"Z"}),
    "vindicator": ("Vindicator", {"Johnny":"Z"}),
    "ravager": ("Ravager", {"AttackTick":"I", "StunTick":"I", "RoarTick":"I"}),
    "trader_llama": ("TraderLlama", {"DespawnDelay":"I"}),
    "zombie": ("Zombie", {"CanBreakDoors":"Z"}),

}


# Domains from the concrete enum constructors, not from the key spelling.
# Require every release's actual constructor ID/name pairs to match exactly.
ENUMS = {
    "axolotl": ("Axolotl", ("Axolotl$Variant",), ("Variant",), "I", ["lucy","wild","gold","cyan","blue"]),
    "parrot": ("Parrot", ("Parrot$Variant",), ("Variant",), "I", ["red_blue","blue","green","yellow_blue","gray"]),
    "panda": ("Panda", ("Panda$Gene","PandaEntity$Type"), ("MainGene","HiddenGene"), "S",
              ["normal","lazy","worried","playful","brown","weak","aggressive"]),
}

def enum_constants(code):
    result = {}
    for item in re.split(r"(?m)^ *\d+: new\s+",code.split('static {};')[-1])[1:]:
        args = item.split('invokespecial',1)[0]
        names = re.findall(r'// String (\w+)',args)
        if len(names)!=2:
            continue
        # Constructor: synthetic enum name+ordinal, explicit saved id, variant
        # name. Keep the explicit id immediately before the serialized name.
        before = args.split('// String '+names[-1])[0]
        ids = [int(x or y) for x,y in re.findall(r'\biconst_(\d+)\b|\b(?:bipush|sipush)\s+(\d+)',before)]
        if len(ids)==2:
            result[ids[-1]]=names[-1]
    return result

def enum_codecs(code, wire):
    actual = set()
    prior=[]
    for line in code.split('static {};')[-1].splitlines():
        prior.append(line)
        if 'putstatic' in line:
            field = re.search(r'// Field (\w+):Lcom/mojang/serialization/Codec;',line)
            if field:
                ins=' '.join(prior)
                if (wire=='I' and 'Field com/mojang/serialization/Codec.INT:' in ins and '.xmap:' in ins) or (
                    wire=='S' and re.search(r'\(Ljava/util/function/Supplier;\)L[^;]+(?:EnumCodec|\$[a-z]+);',ins)):
                    actual.add(field[1])
            prior=[]
    return actual


def classmap(cache, version, jar):
    # Return {named JVM class: obfuscated JVM class} from the actual mapping
    # for this release. Unobfuscated 26.x releases supply JVM names directly.
    if version.startswith("26."):
        with zipfile.ZipFile(jar) as z:
            return {s[:-6].replace("/", "."): s[:-6].replace("/", ".")
                    for s in z.namelist() if s.startswith("net/minecraft/") and s.endswith(".class")}
    official = cache/version/"client-mappings.txt"
    if official.exists():
        with official.open(encoding="utf-8") as f:
            rows = (re.match(r"(net\.minecraft\.\S+) -> (\S+):", line) for line in f)
            return {m[1]: m[2] for m in rows if m}
    ver = "1.10" if version == "1.10.1" else version
    flat = ver.startswith(("1.13", "1.14"))
    with zipfile.ZipFile(cache/"mcp"/(ver+("-config.zip" if flat else "-srg.zip"))) as z:
        text = z.read("config/joined.tsrg" if flat else "joined.srg").decode()
    return {name.replace("/", "."): obf.replace("/", ".") for obf, name in
            re.findall(r"^(?:CL: )?(\S+) (net/minecraft/\S+)", text, re.M)}


def scalar_evidence(cache, version, jar, javap):
    names = classmap(cache, version, jar)
    find = lambda *short: [c for n, c in names.items() if n.rsplit(".",1)[-1] in short]
    nbt = set(k.replace(".", "/") for k in find("NBTTagCompound", "CompoundNBT", "CompoundTag", "ValueInput", "ValueOutput"))
    owners = {}
    for identity, (klass, fields) in SCALAR_FIELDS.items():
        candidates = find(klass, klass+"Entity", "Entity"+klass)
        if candidates:
            cls, = candidates
            owners.setdefault(cls, []).append(("minecraft:"+identity, fields))
    # Piglin shared save/load moved to AbstractPiglin. Read both places only
    # after the exact JAR verifies the species inherits that implementation.
    shared={"IsImmuneToZombification":"Z", "TimeInOverworld":"I"}
    with zipfile.ZipFile(jar) as z:
        for kind, klass in [("piglin","Piglin"),("piglin_brute","PiglinBrute")]:
            for child in find(klass,klass+"Entity","Entity"+klass):
                for base in [child, *find("AbstractPiglin","AbstractPiglinEntity")]:
                    if is_subclass(z,child.replace(".","/"),base.replace(".","/")):
                        owners.setdefault(base, []).append(("minecraft:"+kind, shared))
    variants={}
    classes=set(owners)
    for identity,(species,enum_names,keys,wire,domain) in ENUMS.items():
        candidates=find(species,species+"Entity","Entity"+species)
        if not candidates:continue
        cls,=candidates
        names_=find(*enum_names)
        if names_:
            enum,=names_
            classes.add(enum)
        elif identity=="parrot":
            # Before Parrot.Variant was introduced the saved integer indexes
            # the renderer's texture array directly. Verify that array in EACH
            # exact old release, not the order in a modern enum.
            renderer,=find("RenderParrot","ParrotRenderer")
            enum=None;classes.add(renderer)
        else:
            continue
        variants[identity]=(cls,enum,renderer if enum is None else None,keys,wire,domain)
        classes.add(cls)
    full = subprocess.check_output([javap, "-p", "-c", "-classpath", str(jar), *sorted(classes)], text=True)
    by_class = {}
    current = None
    for line in full.splitlines():
        dec = re.match(r'^(?:[\w ]* )?class ([.$\w]+)', line)
        if dec:
            current = dec[1]
            by_class[current] = []
        elif current is not None:
            by_class[current].append(line)
    evidence = set()
    for cls, species in owners.items():
        for identity, fields in species:
            code = by_class[cls]
            for key, scalar in fields.items():
                read = write = False
                for i, operation in enumerate(code):
                    if not operation.endswith("// String " + key):
                        continue
                    for following in code[i+1:i+15]:
                        if not following.strip() or "// String " in following:
                            break
                        call = re.search(r"// (?:InterfaceMethod|Method) ([\w/$]+)\.\w+:(\(Ljava/lang/String;[^ ]+)", following)
                        if not call or call[1] not in nbt:
                            continue
                        desc = call[2]
                        read |= desc in {"(Ljava/lang/String;)"+scalar, "(Ljava/lang/String;"+scalar+")"+scalar}
                        write |= desc == "(Ljava/lang/String;"+scalar+")V"
                if read and write:
                    evidence.add((identity, key, scalar))
    for identity,(cls,enum,renderer,keys,wire,domain) in variants.items():
        if enum:
            enum_code='\n'.join(by_class[enum])
            if enum_constants(enum_code)!=dict(enumerate(domain)):continue
            enum_path=enum.replace(".","/")
            codec_fields=enum_codecs(enum_code,wire)
        else:
            textures=re.findall(r'// String textures/entity/parrot/parrot_([a-z_]+)\.png','\n'.join(by_class[renderer]))
            if [("gray" if name=="grey" else name) for name in textures]!=domain:continue
            enum_path=None;codec_fields=set()
        code=by_class[cls]
        for key in keys:
            read=write=False
            for i,op in enumerate(code):
                if not op.endswith("// String "+key):continue
                lines=[]
                for line in code[i+1:i+16]:
                    if not line.strip() or '// String ' in line:break
                    lines.append(line)
                ins=' '.join(lines)
                typed=wire if wire!='S' else 'Ljava/lang/String;'
                functions=re.findall(r'// (?:InterfaceMethod|Method) ([\w/$]+)\.\w+:(\(Ljava/lang/String;[^ ]+)',ins)
                for owner,desc in functions:
                    if owner not in nbt:continue
                    if not enum_path or enum_path in ins:
                        read |= desc in {'(Ljava/lang/String;)'+typed,'(Ljava/lang/String;'+typed+')'+typed}
                        write |= desc=='(Ljava/lang/String;'+typed+')V'
                    if enum_path and any('Field '+enum_path+'.'+f+':' in ins for f in codec_fields):
                        read |= desc.startswith('(Ljava/lang/String;Lcom/mojang/serialization/Codec;)Ljava/util/Optional;')
                        write |= desc.startswith('(Ljava/lang/String;Lcom/mojang/serialization/Codec;Ljava/lang/Object;)V')
            if read and write:
                evidence.add(('minecraft:'+identity,key,('index:' if wire=='I' else 'name:')+','.join(domain)))
    return evidence
