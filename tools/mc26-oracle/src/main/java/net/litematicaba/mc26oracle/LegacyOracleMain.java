package net.litematicaba.mc26oracle;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import java.io.InputStream;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.Comparator;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.TreeMap;
import java.util.TreeSet;

/** Exact pre-flattening vanilla data queries. No game main, OpenGL, model
 * renderer or newer-version block data is executed. Java 8 compatible. */
public final class LegacyOracleMain {
    private static final Gson JSON = new GsonBuilder().disableHtmlEscaping().serializeNulls().create();
    private static final String[] DIRECTIONS = {"down", "up", "north", "south", "west", "east"};
    private static final int[][] STEPS = {{0,-1,0},{0,1,0},{0,0,-1},{0,0,1},{-1,0,0},{1,0,0}};
    private final MappingAccess access;
    private final Map<String, String> options;
    private final String version;
    private final Class<?> blockClass, stateClass, positionClass, facingClass, materialClass;
    private final Method fromMeta, stateId, opaque, fullCube, translucent, material, lightOpacity, lightValue, shade, side;
    private final Method collision, faceShape, sideSolid, liquidHeight, fluidBlocked;
    private final boolean collisionUsesWorld;
    private final Method legacyBounds;
    private final Object flowingWater;
    private final boolean stateArguments;
    private final EmptyAccess world;
    private final Object actualWorld;
    private final Map<String, State> states = new TreeMap<String, State>();
    private final Map<String, Map<String, Object>> fluids = new TreeMap<String, Map<String, Object>>();
    private final Map<String, TreeSet<String>> blockEntityBlocks = new TreeMap<String, TreeSet<String>>();
    private final Map<String, Long> blockEntityRawIds = new TreeMap<String, Long>();
    private final Map<Object, String> tileIds = new IdentityHashMap<Object, String>();
    private final Map<Object, double[]> offsetBounds = new IdentityHashMap<Object, double[]>();
    private Map<String, Object> registry;

    private LegacyOracleMain(Map<String, String> options) throws Exception {
        this.options = options;
        this.version = required(options, "minecraft");
        access = MappingAccess.fromMcp(Paths.get(required(options, "mcp-srg")), Paths.get(required(options, "mcp-csv")));
        access.invokeStatic(access.type("net.minecraft.init.Bootstrap"), "func_151354_b");
        blockClass = access.type("net.minecraft.block.Block");
        stateClass = access.type("net.minecraft.block.state.IBlockState");
        Class<?> position = access.typeOrNull("net.minecraft.util.math.BlockPos");
        positionClass = position != null ? position : access.type("net.minecraft.util.BlockPos");
        facingClass = access.type("net.minecraft.util.EnumFacing");
        materialClass = access.type("net.minecraft.block.material.Material");
        fromMeta = access.method(blockClass, "func_176203_a", int.class);
        stateId = access.method(blockClass, "func_176210_f", stateClass);
        stateArguments = access.hasMethod(blockClass, "func_149662_c", 1);
        opaque = blockMethod("func_149662_c"); fullCube = blockMethod("func_149686_d");
        translucent = blockMethod("func_149751_l"); material = blockMethod("func_149688_o");
        lightOpacity = blockMethod("func_149717_k"); lightValue = blockMethod("func_149750_m");
        shade = blockMethod(stateArguments ? "func_185485_f" : "func_149685_I");
        Class<?> blockAccess = access.type("net.minecraft.world.IBlockAccess");
        side = stateArguments ? access.method(blockClass, "func_176225_a", stateClass, blockAccess, positionClass, facingClass)
                : access.method(blockClass, "func_176225_a", blockAccess, positionClass, facingClass);
        Method targetCollision;
        if(stateArguments){
            try{targetCollision=access.method(blockClass,"func_180646_a",stateClass,blockAccess,positionClass);}
            catch(IllegalStateException missing){if(!(missing.getCause() instanceof NoSuchMethodException))throw missing;targetCollision=access.method(blockClass,"func_180646_a",stateClass,access.type("net.minecraft.world.World"),positionClass);}
        }else targetCollision=access.method(blockClass,"func_180640_a",access.type("net.minecraft.world.World"),positionClass,stateClass);
        collision=targetCollision;
        collisionUsesWorld=collision.getParameterTypes()[stateArguments?1:0]==access.type("net.minecraft.world.World");
        legacyBounds = !stateArguments ? access.method(blockClass,"func_180654_a",blockAccess,positionClass) : null;
        faceShape = access.hasMethod(blockClass, "func_193383_a", 4)
                ? access.method(blockClass, "func_193383_a", blockAccess, stateClass, positionClass, facingClass) : null;
        sideSolid = access.hasMethod(blockClass, "func_176212_b", 3)
                ? access.method(blockClass, "func_176212_b", blockAccess, positionClass, facingClass) : null;
        Class<?> liquidClass = access.type("net.minecraft.block.BlockLiquid");
        liquidHeight = access.method(liquidClass, "func_149801_b", int.class);
        flowingWater = access.invokeStatic(blockClass, "func_149729_e", 8);
        fluidBlocked = access.method(access.type("net.minecraft.block.BlockDynamicLiquid"), "func_176372_g",
                access.type("net.minecraft.world.World"), positionClass, stateClass);
        world = new EmptyAccess(blockAccess);
        actualWorld = LegacyWorldAccess.create(access, world, Paths.get(required(options,"generated-world-dir")).toAbsolutePath());
    }

    public static void main(String[] args) throws Exception {
        Map<String, String> options = new HashMap<String, String>();
        for (int i=0;i<args.length;i+=2) {
            if (!args[i].startsWith("--") || i+1>=args.length || options.put(args[i].substring(2), args[i+1]) != null)
                throw new IllegalArgumentException("expected unique --option value pairs");
        }
        String expectedSha = required(options, "client-sha256");
        if (!expectedSha.equals(sha256(Paths.get(required(options, "client-jar"))))) throw new IllegalArgumentException("exact legacy client identity differs");
        Path output = Paths.get(required(options, "output")).toAbsolutePath();
        if (Files.exists(output)) throw new IllegalArgumentException("output already exists: " + output);
        long started = System.nanoTime();
        LegacyOracleMain exporter = new LegacyOracleMain(options);
        exporter.export(output);
        System.out.println(json(map("minecraft", exporter.version, "state_count", exporter.states.size(), "elapsed_ms", (System.nanoTime()-started)/1000000, "status", "ready-headless")));
    }

    @SuppressWarnings("unchecked")
    private void export(Path output) throws Exception {
        Map<String, Object> supplement = JSON.fromJson(new String(Files.readAllBytes(Paths.get(required(options, "legacy-registry"))), StandardCharsets.UTF_8), Map.class);
        Map<String, Object> versionIdentity = (Map<String, Object>) ((Map<?, ?>) supplement.get("versions")).get(version);
        if (versionIdentity == null) throw new IllegalStateException("no exact legacy version registry: " + version);
        registry = (Map<String, Object>) ((Map<?, ?>) supplement.get("registries")).get(versionIdentity.get("registry"));
        Map<String, Object> legacy = (Map<String, Object>) registry.get("legacy_blocks");
        Map<String, Object> blocks = (Map<String, Object>) registry.get("blocks");
        for (Map.Entry<String, Object> entry : new TreeMap<String, Object>(legacy).entrySet()) {
            String[] parts = entry.getKey().split(":");
            int numeric = Integer.parseInt(parts[0]), metadata = Integer.parseInt(parts[1]);
            Object block = access.invokeStatic(blockClass, "func_149729_e", numeric);
            Object state = call(fromMeta, block, metadata);
            String canonical = entry.getValue().toString();
            if (states.containsKey(canonical)) continue;
            State value = new State(canonical, block, state, numeric, metadata);
            Map<String, Object> definition = (Map<String, Object>) blocks.get(value.blockId);
            if (definition == null) throw new IllegalStateException("legacy canonical block absent from exact registry: " + canonical);
            value.defaultState = canonical.equals(definition.get("default"));
            states.put(canonical, value);
        }
        if (states.isEmpty()) throw new IllegalStateException("legacy catalog is empty");
        // The fixed catalog stores the cutout profile. Actual fast/fancy pair
        // visibility is separately sampled below for both production profiles.
        Class<?> leafClass=access.type("net.minecraft.block.BlockLeaves");
        for(State state:states.values())if(leafClass.isInstance(state.block))access.invoke(state.block,leafClass,"func_150122_b",true);
        List<Map<String, Object>> baseRows = new ArrayList<Map<String, Object>>();
        List<Map<String, Object>> semanticRows = new ArrayList<Map<String, Object>>();
        int ordinal = 0;
        for (State state : states.values()) {
            state.ordinal = ordinal++;
            world.set(state, null, 0);
            sample(state);
            baseRows.add(state.base);
            semanticRows.add(state.semantics);
        }
        List<Map<String, Object>> tiles = new ArrayList<Map<String, Object>>();
        for (Map.Entry<String, TreeSet<String>> entry : blockEntityBlocks.entrySet()) {
            tiles.add(map("raw_id", blockEntityRawIds.get(entry.getKey()), "type_id", entry.getKey(), "valid_block_ids", new ArrayList<String>(entry.getValue())));
        }
        // The target's old culling is binary. Shape.block comes only from its
        // real isOpaqueCube, never selection or collision bounds. Pair queries
        // preserve the additional vanilla slab/leaves/glass/fluid decisions.
        List<Map<String, Object>> shapes = Arrays.asList(
                map("kind", "block", "rectangles", Collections.emptyList(), "shape_id", shapeId("block")),
                map("kind", "empty", "rectangles", Collections.emptyList(), "shape_id", shapeId("empty")));
        SkipResult skip = sampleVisibility();
        Files.createDirectories(output);
        List<Map<String, Object>> artifacts = new ArrayList<Map<String, Object>>();
        artifacts.add(writeArtifact(output, "block_states", baseRows, "state_id"));
        artifacts.add(writeArtifact(output, "fluid_states", new ArrayList<Map<String, Object>>(fluids.values()), "state_id"));
        artifacts.add(writeArtifact(output, "block_entity_types", tiles, "type_id"));
        artifacts.add(writeArtifact(output, "block_state_semantics", semanticRows, "state_id"));
        artifacts.add(writeArtifact(output, "face_occlusion_shapes", shapes, "shape_id"));
        artifacts.add(writeArtifact(output, "skip_render_rows", skip.rows, "profile", "source_catalog_state_index", "direction"));
        Map<String, Object> semantics = map(
                "block_tag_predicate_source", "legacy-runtime:BlockDynamicLiquid.isBlocked;Material.blocksMovement;World.top-solid-or-liquid",
                "block_tag_predicates", map("blocks_fluid_flow", "BlockDynamicLiquid.isBlocked(actual-empty-World,BlockPos.ZERO,IBlockState)", "blocks_motion_in_heightmap", "Material.blocksMovement()", "blocks_motion_in_heightmap_no_leaves", "Material.blocksMovement()&&!Material.LEAVES"),
                "catalog_index_order", "state_id_ordinal",
                "context_probe", map("block_getter", "legacy.IBlockAccess(empty-air;source-and-neighbor)", "block_pos", Arrays.asList(0,0,0)),
                "face_projection_axes", map("down","u=x,v=z","up","u=x,v=z","north","u=x,v=y","south","u=x,v=y","west","u=z,v=y","east","u=z,v=y"),
                "face_sturdy_dynamic_shape_policy", "legacy:actual-target-empty-context-support-query",
                "face_sturdy_not_occlusion_shape", true,
                "face_sturdy_source", faceShape != null ? "legacy:Block.getBlockFaceShape(IBlockAccess,IBlockState,BlockPos,EnumFacing)==SOLID" : "legacy:Block.isBlockSolid(IBlockAccess,BlockPos,EnumFacing)",
                "light_permeable_source", "legacy:Block.isTranslucent(actual-state)",
                "light_dampening_source", "legacy:min(15,Block.getLightOpacity(actual-state))",
                "offset_classifier", "legacy:Block.getOffsetType;actual-target-offset-query",
                "shape_kind_policy", "legacy:block_only_for_Block.isOpaqueCube;otherwise_empty;pair_visibility_from_actual_shouldSideBeRendered",
                "shape_record_count", shapes.size(), "state_count", states.size(),
                "skip_rendering", map("profiles", Arrays.asList("cutout","opaque"), "evaluated_comparisons", skip.comparisons,
                    "override_owners", new ArrayList<String>(skip.owners), "source_block_ids", new ArrayList<String>(skip.blocks),
                    "source_state_count", states.size(), "sparse_positive_row_count", skip.rows.size()));
        Map<String, Object> manifest = map("artifacts", artifacts, "block_state_semantics", semantics,
                "client_jar_sha256", required(options,"client-sha256"), "client_jar_size", Files.size(Paths.get(required(options,"client-jar"))),
                "java_major", 8, "minecraft", version, "pending_sets", Arrays.asList("client_reload"),
                "protocol_version", Integer.parseInt(required(options,"protocol-version")), "ready_headless_sets", artifacts.size(),
                "schema", "lba.oracle.mc26.runtime", "schema_version",3,"world_version",((Number)versionIdentity.get("data_version")).intValue());
        Files.write(output.resolve("oracle-manifest.json"), (json(manifest)+"\n").getBytes(StandardCharsets.UTF_8));
    }

    private void sample(State s) throws Exception {
        if(legacyBounds!=null)call(legacyBounds,s.block,world.proxy,world.zero);
        boolean isOpaque = bool(query(opaque,s)), solid = bool(access.invoke(query(material,s),materialClass,"func_76220_a"));
        int opacity = Math.min(15, number(query(lightOpacity,s)).intValue());
        int emission = number(query(lightValue,s)).intValue();
        if (opacity<0 || emission<0 || emission>15) throw new IllegalStateException("invalid legacy light values: "+s.id);
        Object mat = query(material,s);
        boolean motion = bool(access.invoke(mat,materialClass,"func_76230_c"));
        Object leafMaterial = access.staticField(materialClass,"field_151584_j");
        boolean noLeaves = motion && mat != leafMaterial;
        boolean fluid = bool(access.invoke(mat, materialClass,"func_76224_d"));
        String fluidId = "minecraft:empty";
        if (fluid) {
            boolean water = mat == access.staticField(materialClass,"field_151586_h");
            boolean lava = mat == access.staticField(materialClass,"field_151587_i");
            if (!water && !lava) throw new IllegalStateException("unknown actual legacy liquid material");
            String owner = water ? "minecraft:water" : "minecraft:lava";
            Map<String,String> properties = new TreeMap<String,String>(); properties.put("level",Integer.toString(s.meta));
            fluidId = owner+"[level="+s.meta+"]";
            double height = 1.0D-number(call(liquidHeight,null,s.meta)).doubleValue();
            if (!fluids.containsKey(fluidId)) fluids.put(fluidId,map("amount",s.meta>=8?8:8-s.meta,"default_state",s.meta==0,"empty",false,"fluid_id",owner,"is_source",s.meta==0,"own_height",height,"properties",properties,"raw_id",s.rawId,"state_id",fluidId));
        }
        if (!fluids.containsKey("minecraft:empty")) fluids.put("minecraft:empty",map("amount",0,"default_state",true,"empty",true,"fluid_id","minecraft:empty","is_source",false,"own_height",0.0D,"properties",Collections.emptyMap(),"raw_id",0,"state_id","minecraft:empty"));
        String render = renderShape(s,fluid);
        List<String> tileTypes = tileType(s);
        Map<String,Object> faceIds = new TreeMap<String,Object>(), support = new TreeMap<String,Object>();
        String shape = shapeId(isOpaque?"block":"empty");
        for (int direction=0;direction<6;direction++) {
            faceIds.put(DIRECTIONS[direction],shape);
            if (faceShape!=null) support.put(DIRECTIONS[direction], ((Enum<?>)call(faceShape,s.block,world.proxy,s.state,world.zero,world.facings[direction])).name().equals("SOLID"));
            else if (sideSolid!=null) support.put(DIRECTIONS[direction],bool(call(sideSolid,s.block,world.proxy,world.zero,world.facings[direction])));
            else throw new IllegalStateException("target has no implemented real support-face API");
        }
        Object box = stateArguments ? call(collision,s.block,s.state,collisionUsesWorld?actualWorld:world.proxy,world.zero) : call(collision,s.block,actualWorld,world.zero,s.state);
        boolean collisionFull = box!=null && boxValue(box,"minX")==0 && boxValue(box,"minY")==0 && boxValue(box,"minZ")==0 && boxValue(box,"maxX")==1 && boxValue(box,"maxY")==1 && boxValue(box,"maxZ")==1;
        boolean large = box!=null && (boxValue(box,"minX")<0 || boxValue(box,"minY")<0 || boxValue(box,"minZ")<0 || boxValue(box,"maxX")>1 || boxValue(box,"maxY")>1 || boxValue(box,"maxZ")>1);
        Object offset = access.invoke(s.block,blockClass,"func_176218_Q");
        String offsetKind = ((Enum<?>)offset).name().toLowerCase(Locale.ROOT);
        if (!Arrays.asList("none","xz","xyz").contains(offsetKind)) throw new IllegalStateException("unknown legacy offset policy: "+offsetKind);
        double[] bounds=offsetBounds.get(s.block);
        if(bounds==null){bounds=sampleOffsetBounds(s,offsetKind);offsetBounds.put(s.block,bounds);}
        double maxHorizontal = bounds[0], maxVertical=bounds[1];
        s.base = map("air",s.numeric==0,"block_id",s.blockId,"can_occlude",isOpaque,"default_state",s.defaultState,
                "fluid_state_id",fluidId,"light_emission",emission,"properties",s.properties,"raw_id",s.rawId,"render_shape",render,"solid",solid,"solid_render",isOpaque,"state_id",s.id);
        s.semantics = map("block_entity_type_ids",tileTypes,"block_id",s.blockId,"blocks_fluid_flow",bool(call(fluidBlocked,flowingWater,actualWorld,world.zero,s.state)),
                "blocks_motion_in_heightmap",motion,"blocks_motion_in_heightmap_no_leaves",noLeaves,"can_occlude",isOpaque,"catalog_state_index",s.ordinal,
                "collision_full_empty_context",collisionFull,"collision_full_scope","empty_context","dynamic_shape",true,"emissive_rendering",emission>0,
                "face_occlusion_shape_ids",faceIds,"face_sturdy_empty_context",support,"face_sturdy_scope","empty_context","face_sturdy_support_type","full",
                "fluid_state_id",fluidId,"has_block_entity",!tileTypes.isEmpty(),"has_large_collision_shape",large,"has_offset",!offsetKind.equals("none"),
                "light_dampening",opacity,"light_emission",emission,"light_permeable",bool(query(translucent,s)),"offset_kind",offsetKind,
                "offset_max_horizontal",maxHorizontal,"offset_max_vertical",maxVertical,"propagates_skylight_down",opacity==0,"render_shape",render,
                "shade_brightness_empty_context",number(query(shade,s)).doubleValue(),"shade_brightness_scope","empty_context","solid",solid,"solid_render",isOpaque,"state_id",s.id,"use_shape_for_light_occlusion",false);
    }

    private List<String> tileType(State s) throws Exception {
        if (!bool(access.invoke(s.block,blockClass,"func_149716_u"))) return Collections.emptyList();
        Class<?> provider = access.type("net.minecraft.block.ITileEntityProvider");
        if (!provider.isInstance(s.block)) throw new IllegalStateException("legacy tile block does not implement its actual factory: "+s.id);
        Object tile = access.invoke(s.block,provider,"func_149915_a",null,s.meta);
        boolean movingPayloadAbsent = tile == null && access.type("net.minecraft.block.BlockPistonMoving").isInstance(s.block);
        // Moving pistons intentionally return no generic tile. The real type
        // registry still records TileEntityPiston; absent captured payload stays
        // absent in the empty-context world, rather than inventing a moved block.
        if (movingPayloadAbsent) tile = access.construct(access.type("net.minecraft.tileentity.TileEntityPiston"));
        if (tile==null) throw new IllegalStateException("actual legacy tile factory returned null: "+s.id);
        Class<?> tileClass=access.type("net.minecraft.tileentity.TileEntity");
        s.tile = movingPayloadAbsent ? null : tile;
        if(access.hasMethod(tileClass,"func_145834_a",1))access.invoke(tile,tileClass,"func_145834_a",actualWorld);
        if(access.hasMethod(tileClass,"func_174878_a",1))access.invoke(tile,tileClass,"func_174878_a",world.zero);
        String type;
        if (access.hasMethod(tileClass,"func_190559_a",1)) type=access.invokeStatic(tileClass,"func_190559_a",tile.getClass()).toString();
        else {
            Map<?,?> actualNames=(Map<?,?>)access.staticField(tileClass,"field_145853_j");
            Object registered=actualNames.get(tile.getClass());
            if(registered==null)throw new IllegalStateException("unregistered actual legacy block entity class: "+tile.getClass());
            String old=registered.toString();
            type=blockEntityIdFix(old);
        }
        tileIds.put(s.state,type);
        TreeSet<String> valid=blockEntityBlocks.get(type);
        if(valid==null){
            valid=new TreeSet<String>();blockEntityBlocks.put(type,valid);
            if(access.hasField(tileClass,"field_190562_f")){
                Object actualRegistry=access.staticField(tileClass,"field_190562_f");
                Long actualId = access.hasMethod(actualRegistry.getClass(),"func_148757_b",1)
                        ? number(access.invoke(actualRegistry,actualRegistry.getClass(),"func_148757_b",tile.getClass())).longValue() : null;
                blockEntityRawIds.put(type,actualId != null && actualId >= 0 ? actualId : null);
            }else blockEntityRawIds.put(type,null);
        }
        valid.add(s.blockId);
        return Collections.singletonList(type);
    }

    private SkipResult sampleVisibility() throws Exception {
        SkipResult result=new SkipResult();
        Class<?> leaves=access.type("net.minecraft.block.BlockLeaves");
        Method graphics=access.method(leaves,"func_150122_b",boolean.class);
        Map<Object,Boolean> original=new IdentityHashMap<Object,Boolean>();
        String graphicsField = access.hasField(leaves,"leavesFancy") ? "leavesFancy" : "isTransparent";
        for(State s:states.values()) if(leaves.isInstance(s.block)&&!original.containsKey(s.block)) original.put(s.block,(Boolean)access.field(s.block,leaves,graphicsField));
        try {
            for(String profile:Arrays.asList("cutout","opaque")) {
                for(Object leaf:original.keySet()) call(graphics,leaf,profile.equals("cutout"));
                for(State source:states.values()) {
                    result.blocks.add(source.blockId);
                    String owner=access.canonicalName(effectiveSideOwner(source.block.getClass()));
                    result.owners.add(owner);
                    for(int direction=0;direction<6;direction++) {
                        List<Map<String,Object>> ranges=new ArrayList<Map<String,Object>>(); int start=-1,end=-1;
                        for(State neighbor:states.values()) {
                            world.set(source,neighbor,direction);
                            if(legacyBounds!=null)call(legacyBounds,source.block,world.proxy,world.zero);
                            boolean shown=stateArguments?bool(call(side,source.block,source.state,world.proxy,world.zero,world.facings[direction]))
                                    :bool(call(side,source.block,world.proxy,world.neighborPositions[direction],world.facings[direction]));
                            result.comparisons++;
                            if(!shown){if(start<0)start=neighbor.ordinal;end=neighbor.ordinal+1;}
                            else if(start>=0){ranges.add(map("start",start,"end_exclusive",end));start=-1;}
                        }
                        if(start>=0)ranges.add(map("start",start,"end_exclusive",end));
                        if(!ranges.isEmpty())result.rows.add(map("profile",profile,"source_catalog_state_index",source.ordinal,"source_state_id",source.id,"direction",DIRECTIONS[direction],"override_owner",owner,"neighbor_catalog_index_ranges",ranges));
                    }
                }
            }
        } finally { for(Map.Entry<Object,Boolean> e:original.entrySet())call(graphics,e.getKey(),e.getValue()); }
        return result;
    }

    private Class<?> effectiveSideOwner(Class<?> concrete) {
        for(Class<?> type=concrete;type!=null;type=type.getSuperclass())try{return type.getDeclaredMethod(side.getName(),side.getParameterTypes()).getDeclaringClass();}catch(NoSuchMethodException absent){}
        throw new IllegalStateException("missing effective legacy side method");
    }

    private double[] sampleOffsetBounds(State state,String kind)throws Exception{
        if(kind.equals("none"))return new double[]{0,0};
        Method offset=access.hasMethod(blockClass,"func_190949_e",3)?access.method(blockClass,"func_190949_e",stateClass,access.type("net.minecraft.world.IBlockAccess"),positionClass):null;
        Class<?> mathClass=access.typeOrNull("net.minecraft.util.math.MathHelper");if(mathClass==null)mathClass=access.type("net.minecraft.util.MathHelper");
        Class<?> vectorClass=access.typeOrNull("net.minecraft.util.math.Vec3i");if(vectorClass==null)vectorClass=access.type("net.minecraft.util.Vec3i");
        Method seed=offset==null?access.method(mathClass,"func_180186_a",vectorClass):null;
        TreeSet<Double>xs=new TreeSet<Double>(),ys=new TreeSet<Double>(),zs=new TreeSet<Double>();
        double horizontal=0,vertical=0;
        for(int x=0;x<48;x++)for(int z=0;z<48;z++){
            Object position=access.construct(positionClass,x,7,z);double dx,dy,dz;
            if(offset!=null){Object value=call(offset,state.block,state.state,world.proxy,position);dx=number(access.field(value,value.getClass(),"field_72450_a")).doubleValue();dy=number(access.field(value,value.getClass(),"field_72448_b")).doubleValue();dz=number(access.field(value,value.getClass(),"field_72449_c")).doubleValue();}
            else{
                // Actual pre-getOffset BlockModelRenderer four-bit offset
                // formula, fed by this client's real MathHelper position seed.
                long value=number(call(seed,null,position)).longValue();
                dx=(((double)((float)(value>>16&15L)/15.0F))-.5D)*.5D;
                dy=kind.equals("xyz")?(((double)((float)(value>>20&15L)/15.0F))-1.0D)*.2D:0;
                dz=(((double)((float)(value>>24&15L)/15.0F))-.5D)*.5D;
            }
            if(!Double.isFinite(dx)||!Double.isFinite(dy)||!Double.isFinite(dz))throw new IllegalStateException("non-finite actual legacy offset");
            xs.add(dx);ys.add(dy);zs.add(dz);horizontal=Math.max(horizontal,Math.max(Math.abs(dx),Math.abs(dz)));vertical=Math.max(vertical,Math.abs(dy));
        }
        if(xs.size()!=16||zs.size()!=16||(kind.equals("xyz")&&ys.size()!=16)||(!kind.equals("xyz")&&(ys.size()!=1||ys.first()!=0)))throw new IllegalStateException("actual legacy offset query did not cover its complete four-bit domains: "+state.id);
        if(horizontal!=.25D || (kind.equals("xyz")&&vertical!=.2D))throw new IllegalStateException("unexpected actual legacy offset extrema: "+state.id);
        return new double[]{horizontal,vertical};
    }

    private final class EmptyAccess implements InvocationHandler {
        final Object proxy,zero,air;
        final Object[] facings=facingClass.getEnumConstants(),neighborPositions=new Object[6];
        final Method getBlockState,getTileEntity,isAir,getLight,getBiome,getStrongPower,getWorldType;
        final Method x,y,z;
        State source,neighbor; int direction;
        EmptyAccess(Class<?> blockAccess) {
            zero=access.construct(positionClass,0,0,0);
            for(int i=0;i<6;i++)neighborPositions[i]=access.construct(positionClass,STEPS[i][0],STEPS[i][1],STEPS[i][2]);
            for(int i=0;i<6;i++)if(!((Enum<?>)facings[i]).name().equalsIgnoreCase(DIRECTIONS[i]))throw new IllegalStateException("unexpected actual direction order");
            Object airBlock=access.invokeStatic(blockClass,"func_149729_e",0);air=access.invoke(airBlock,blockClass,"func_176223_P");
            getBlockState=access.method(blockAccess,"func_180495_p",positionClass);getTileEntity=access.method(blockAccess,"func_175625_s",positionClass);
            isAir=access.method(blockAccess,"func_175623_d",positionClass);getLight=access.method(blockAccess,"func_175626_b",positionClass,int.class);
            getBiome=access.method(blockAccess,"func_180494_b",positionClass);getStrongPower=access.method(blockAccess,"func_175627_a",positionClass,facingClass);
            getWorldType=access.method(blockAccess,"func_175624_G");
            x=access.method(positionClass,"func_177958_n");y=access.method(positionClass,"func_177956_o");z=access.method(positionClass,"func_177952_p");
            proxy=Proxy.newProxyInstance(blockAccess.getClassLoader(),new Class<?>[]{blockAccess},this);
        }
        void set(State source,State neighbor,int direction){this.source=source;this.neighbor=neighbor;this.direction=direction;}
        Object state(Object p)throws Exception{
            int px=number(call(x,p)).intValue(),py=number(call(y,p)).intValue(),pz=number(call(z,p)).intValue();
            if(source!=null&&px==0&&py==0&&pz==0)return source.state;
            if(neighbor!=null&&px==STEPS[direction][0]&&py==STEPS[direction][1]&&pz==STEPS[direction][2])return neighbor.state;
            return air;
        }
        public Object invoke(Object self,Method method,Object[] arguments)throws Throwable{
            if(method.equals(getBlockState))return state(arguments[0]);
            if(method.equals(getTileEntity)){
                int px=number(call(x,arguments[0])).intValue(),py=number(call(y,arguments[0])).intValue(),pz=number(call(z,arguments[0])).intValue();
                if(source!=null&&px==0&&py==0&&pz==0)return source.tile;
                if(neighbor!=null&&px==STEPS[direction][0]&&py==STEPS[direction][1]&&pz==STEPS[direction][2])return neighbor.tile;
                return null;
            }
            if(method.equals(isAir))return state(arguments[0])==air;
            if(method.equals(getLight))return (15<<20)|(((Number)arguments[1]).intValue()<<4);
            if(method.equals(getStrongPower))return 0;
            if(method.equals(getWorldType))return access.staticField(access.type("net.minecraft.world.WorldType"),"field_77137_b");
            if(method.equals(getBiome)){
                Class<?> biomes=access.typeOrNull("net.minecraft.init.Biomes");
                return biomes!=null?access.staticField(biomes,"field_76772_c"):access.staticField(access.type("net.minecraft.world.biome.BiomeGenBase"),"field_76772_c");
            }
            if(method.getDeclaringClass()==Object.class){if(method.getName().equals("toString"))return "exact legacy empty IBlockAccess";if(method.getName().equals("hashCode"))return System.identityHashCode(self);if(method.getName().equals("equals"))return self==arguments[0];}
            throw new IllegalStateException("unimplemented actual IBlockAccess query: "+method);
        }
    }

    private final class State {
        final String id,blockId; final Object block,state;final int numeric,meta,rawId;final Map<String,String>properties=new TreeMap<String,String>();
        boolean defaultState;int ordinal;Object tile;Map<String,Object>base,semantics;
        State(String id,Object block,Object state,int numeric,int meta)throws Exception{
            this.id=id;this.block=block;this.state=state;this.numeric=numeric;this.meta=meta;this.rawId=number(call(stateId,null,state)).intValue();
            int open=id.indexOf('[');blockId=open<0?id:id.substring(0,open);
            if(open>=0)for(String pair:id.substring(open+1,id.length()-1).split(",")){String[]p=pair.split("=",2);properties.put(p[0],p[1]);}
        }
    }
    private static final class SkipResult {final List<Map<String,Object>>rows=new ArrayList<Map<String,Object>>();final TreeSet<String>owners=new TreeSet<String>(),blocks=new TreeSet<String>();long comparisons;}
    private Method blockMethod(String name){return stateArguments?access.method(blockClass,name,stateClass):access.method(blockClass,name);}
    private Object query(Method method,State state)throws Exception{return stateArguments?call(method,state.block,state.state):call(method,state.block);}
    private double boxValue(Object box,String field){return number(access.field(box,box.getClass(),field)).doubleValue();}
    private String renderShape(State state,boolean fluid)throws Exception{
        if(fluid)return "MODEL";
        Object render=stateArguments?access.invoke(state.block,blockClass,"func_149645_b",state.state):access.invoke(state.block,blockClass,"func_149645_b");
        if(render instanceof Enum<?>){String value=((Enum<?>)render).name();if(value.equals("ENTITYBLOCK_ANIMATED"))return value;if(value.equals("MODEL")||value.equals("INVISIBLE"))return value;}
        else {int value=number(render).intValue();if(value==-1)return "INVISIBLE";if(value==3)return "MODEL";if(value==2)return "ENTITYBLOCK_ANIMATED";}
        throw new IllegalStateException("unsupported actual legacy render type "+render+" for "+state.id);
    }
    private static String blockEntityIdFix(String id){
        // Actual vanilla BlockEntityIdFix(v704), not a class-name convention.
        String[]old={"Airportal","Banner","Beacon","Cauldron","Chest","Comparator","Control","DLDetector","Dropper","EnchantTable","EndGateway","EnderChest","FlowerPot","Furnace","Hopper","MobSpawner","Music","Piston","RecordPlayer","Sign","Skull","Structure","Trap"};
        String[]next={"end_portal","banner","beacon","brewing_stand","chest","comparator","command_block","daylight_detector","dropper","enchanting_table","end_gateway","ender_chest","flower_pot","furnace","hopper","mob_spawner","noteblock","piston","jukebox","sign","skull","structure_block","dispenser"};
        for(int i=0;i<old.length;i++)if(old[i].equals(id))return "minecraft:"+next[i];
        if(id.matches("[a-z0-9_.-]+:[a-z0-9_/.-]+"))return id;
        throw new IllegalStateException("unmapped actual legacy block entity ID: "+id);
    }
    private static Object call(Method method,Object receiver,Object...arguments)throws Exception{try{return method.invoke(receiver,arguments);}catch(InvocationTargetException e){Throwable cause=e.getCause();if(cause instanceof Exception)throw(Exception)cause;if(cause instanceof Error)throw(Error)cause;throw e;}}
    private static boolean bool(Object value){return(Boolean)value;}
    private static Number number(Object value){return(Number)value;}
    private static String required(Map<String,String>options,String key){String value=options.get(key);if(value==null||value.isEmpty())throw new IllegalArgumentException("missing --"+key);return value;}
    public static Map<String,Object> map(Object...pairs){Map<String,Object>result=new TreeMap<String,Object>();for(int i=0;i<pairs.length;i+=2)result.put(pairs[i].toString(),pairs[i+1]);return result;}
    public static String json(Object value){return JSON.toJson(value);}
    private static String shapeId(String kind)throws Exception{return "sha256:"+sha256((json(map("kind",kind,"rectangles",Collections.emptyList()))).getBytes(StandardCharsets.UTF_8));}
    public static String sha256(Path path)throws Exception{MessageDigest hash=MessageDigest.getInstance("SHA-256");try(InputStream input=Files.newInputStream(path)){byte[]buffer=new byte[65536];for(int count;(count=input.read(buffer))>=0;)hash.update(buffer,0,count);}return hex(hash.digest());}
    private static String sha256(byte[]bytes)throws Exception{return hex(MessageDigest.getInstance("SHA-256").digest(bytes));}
    private static String hex(byte[]bytes){StringBuilder text=new StringBuilder();for(byte value:bytes)text.append(String.format(Locale.ROOT,"%02x",value&255));return text.toString();}
    public static Map<String,Object> writeArtifact(Path directory,final String name,List<Map<String,Object>> rows,final String...keys)throws Exception{
        List<Map<String,Object>>sorted=new ArrayList<Map<String,Object>>(rows);Collections.sort(sorted,new Comparator<Map<String,Object>>(){@SuppressWarnings({"unchecked","rawtypes"})public int compare(Map<String,Object>a,Map<String,Object>b){for(String key:keys){Object x=a.get(key),y=b.get(key);int c=x instanceof Number&&y instanceof Number?Long.compare(((Number)x).longValue(),((Number)y).longValue()):((Comparable)x).compareTo(y);if(c!=0)return c;}return 0;}});
        StringBuilder payload=new StringBuilder();String previous=null;for(Map<String,Object>row:sorted){StringBuilder identity=new StringBuilder();for(String key:keys)identity.append(json(row.get(key))).append('\0');if(identity.toString().equals(previous))throw new IllegalStateException("duplicate artifact key "+name);previous=identity.toString();payload.append(json(row)).append('\n');}
        byte[]bytes=payload.toString().getBytes(StandardCharsets.UTF_8);Files.write(directory.resolve(name+".jsonl"),bytes);
        return map("name",name,"path",name+".jsonl","key_fields",Arrays.asList(keys),"record_count",rows.size(),"records_sha256",sha256(bytes));
    }
}
