package net.litematicaba.mc26oracle;

import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.HashSet;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;

final class BlockStateSemanticsOracle {
    private static final int[][] OFFSET_PROBES = createOffsetProbes();

    private final MinecraftRuntimeOracle runtime;
    private final IdentityHashMap<Object, String> fluidStateIds;
    private final Class<?> block = ReflectionAccess.type("net.minecraft.world.level.block.Block");
    private final Class<?> blockBehaviour =
            ReflectionAccess.type("net.minecraft.world.level.block.state.BlockBehaviour");
    private final Class<?> blockState =
            ReflectionAccess.type("net.minecraft.world.level.block.state.BlockState");
    private final Class<?> blockStateBase =
            ReflectionAccess.type(
                    "net.minecraft.world.level.block.state.BlockBehaviour$BlockStateBase");
    private final Class<?> blockGetter =
            ReflectionAccess.type("net.minecraft.world.level.BlockGetter");
    private final Class<?> blockPos = ReflectionAccess.type("net.minecraft.core.BlockPos");
    private final Class<?> direction = ReflectionAccess.type("net.minecraft.core.Direction");
    private final Class<?> voxelShape =
            ReflectionAccess.type("net.minecraft.world.phys.shapes.VoxelShape");
    private final Class<?> shapes = ReflectionAccess.type("net.minecraft.world.phys.shapes.Shapes");
    private final Class<?> aabb = ReflectionAccess.type("net.minecraft.world.phys.AABB");
    private final Class<?> vec3 = ReflectionAccess.type("net.minecraft.world.phys.Vec3");
    private final Class<?> emptyBlockGetter =
            ReflectionAccess.type("net.minecraft.world.level.EmptyBlockGetter");
    private final Class<?> blockEntityType =
            ReflectionAccess.type("net.minecraft.world.level.block.entity.BlockEntityType");
    private final Class<?> leavesBlock = ReflectionAccess.type("net.minecraft.world.level.block.LeavesBlock");
    private final Class<?> blockTags = ReflectionAccess.type("net.minecraft.tags.BlockTags");
    private final boolean usesBlockTags = ReflectionAccess.MAPPING.hasField(blockTags, "BLOCKS_FLUID_FLOW")
            && ReflectionAccess.MAPPING.hasField(blockTags, "BLOCKS_MOTION_IN_HEIGHTMAP")
            && ReflectionAccess.MAPPING.hasField(blockTags, "BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES");

    private final java.util.function.Predicate<Object> noLeavesPredicate = createNoLeavesPredicate();
    @SuppressWarnings("unchecked") private java.util.function.Predicate<Object> createNoLeavesPredicate() {
        if (usesBlockTags) return null;
        Class<?> types = ReflectionAccess.type("net.minecraft.world.level.levelgen.Heightmap$Types");
        Object value = ReflectionAccess.staticField(types, "MOTION_BLOCKING_NO_LEAVES");
        if (ReflectionAccess.MAPPING.hasMethod(types, "isOpaque", 0))
            return (java.util.function.Predicate<Object>) ReflectionAccess.MAPPING.invoke(value, types, "isOpaque");
        return readerAwareHeightmapPredicate(value);
    }

    private java.util.function.Predicate<Object> readerAwareHeightmapPredicate(Object type) {
        Class<?> heightmap = ReflectionAccess.MAPPING.type("net.minecraft.world.gen.Heightmap");
        Object sample = ReflectionAccess.MAPPING.construct(heightmap, null, type);
        Object predicate = ReflectionAccess.MAPPING.field(sample, heightmap, "field_202276_b");
        Class<?> readerAware = ReflectionAccess.MAPPING.type("net.minecraft.block.state.pattern.IBlockMatcherReaderAware");
        return state -> (Boolean) ReflectionAccess.MAPPING.invoke(predicate, readerAware, "test", state,
                ReflectionAccess.staticField(emptyBlockGetter, "INSTANCE"), ReflectionAccess.staticField(blockPos, "ZERO"));
    }
    private final java.util.function.Predicate<Object> flattenedMotionPredicate = Boolean.getBoolean("nova.oracle.flattened")
            ? readerAwareHeightmapPredicate(ReflectionAccess.staticField(ReflectionAccess.type("net.minecraft.world.level.levelgen.Heightmap$Types"), "MOTION_BLOCKING")) : null;

    private boolean collisionFull(Object state) {
        if (isCollisionShapeFullBlock != null) return booleanValue(invoke(isCollisionShapeFullBlock, state, emptyGetter, zeroPos));
        Object actual = ReflectionAccess.MAPPING.invoke(state, blockStateBase, "getCollisionShape", emptyGetter, zeroPos);
        return (Boolean) ReflectionAccess.MAPPING.invokeStatic(block, "isShapeFullBlock", actual);
    }

    private boolean faceSturdy(Object state, Object blockValue, Object face) {
        if (isFaceSturdy != null) return booleanValue(invoke(isFaceSturdy, state, emptyGetter, zeroPos, face));
        if (ReflectionAccess.MAPPING.hasMethod(block, "func_220056_d", 4))
            return (Boolean) ReflectionAccess.MAPPING.invoke(blockValue, block, "func_220056_d", state, emptyGetter, zeroPos, face);
        Object actual = ReflectionAccess.MAPPING.invoke(state, blockStateBase, "func_193401_d", emptyGetter, zeroPos, face);
        return ((Enum<?>) actual).name().equals("SOLID");
    }

    private Object faceOcclusion(Object state, Object face) {
        if (getFaceOcclusionShape != null) return invoke(getFaceOcclusionShape, state, face);
        if (!booleanValue(invoke(canOcclude, state))) return emptyShape;
        Object actual = ReflectionAccess.MAPPING.invoke(state, blockStateBase, "getOcclusionShape", emptyGetter, zeroPos);
        // The early 1.13 sliced voxel implementation cannot enumerate some
        // face views (negative BitSet indices). Read the unsliced real shape;
        // recordFaceShape takes its exact boundary cross-section instead.
        return actual;
    }

    @SuppressWarnings("unchecked") private boolean largeCollision(Object state, Object blockValue) {
        if (hasLargeCollisionShape != null) return booleanValue(invoke(hasLargeCollisionShape, state));
        if (booleanValue(invoke(hasDynamicShape, blockValue))) return true;
        Object shape = ReflectionAccess.MAPPING.invoke(state, blockStateBase, "getCollisionShape", emptyGetter, zeroPos);
        for (Object box : (List<Object>) invoke(shapeToAabbs, shape))
            if (fieldDouble(aabbMinX, box) < 0 || fieldDouble(aabbMinY, box) < 0 || fieldDouble(aabbMinZ, box) < 0
                || fieldDouble(aabbMaxX, box) > 1 || fieldDouble(aabbMaxY, box) > 1 || fieldDouble(aabbMaxZ, box) > 1) return true;
        return false;
    }

    private boolean stateSolid(Object state) {
        if (isSolid != null && !Boolean.getBoolean("nova.oracle.flattened")) return (Boolean) invoke(isSolid, state);
        Object material = ReflectionAccess.MAPPING.invoke(state, blockStateBase, "getMaterial");
        return (Boolean) ReflectionAccess.MAPPING.invoke(material, material.getClass(), "isSolid");
    }

    private boolean blocksMotion(Object state) {
        if (ReflectionAccess.MAPPING.hasMethod(blockStateBase, "blocksMotion", 0))
            return (Boolean) ReflectionAccess.MAPPING.invoke(state, blockStateBase, "blocksMotion");
        Object material = ReflectionAccess.MAPPING.invoke(state, blockStateBase, "getMaterial");
        return (Boolean) ReflectionAccess.MAPPING.invoke(material, material.getClass(), "blocksMotion");
    }

    private boolean blockTag(Object state, String field) {
        Object tag = ReflectionAccess.staticField(blockTags, field);
        return (Boolean) ReflectionAccess.MAPPING.invoke(state, blockStateBase, "is", tag);
    }


    private final Method getFaceOcclusionShape = optionalFaceOcclusion();
    private Method optionalFaceOcclusion() {
        try { return publicMethod(blockStateBase, "getFaceOcclusionShape", direction); }
        catch (IllegalStateException missing) { if (Boolean.getBoolean("nova.oracle.flattened")) return null; throw missing; }
    }
    private final Method getLightDampening = publicMethod(blockStateBase, "getLightDampening");
    private final Method hasLargeCollisionShape = optionalMethod(blockStateBase, "hasLargeCollisionShape");
    private final Method useShapeForLightOcclusion = optionalMethod(blockStateBase, "useShapeForLightOcclusion");
    private final Method getLightEmission = publicMethod(blockStateBase, "getLightEmission");
    private final Method getRenderShape = publicMethod(blockStateBase, "getRenderShape");
    private final Method emissiveRendering = optionalContextMethod(blockStateBase, "emissiveRendering");
    private final Method getShadeBrightness =
            publicMethod(blockStateBase, "getShadeBrightness", blockGetter, blockPos);
    private final Method isLightPermeable = optionalMethod(blockStateBase, "isLightPermeable");
    private final Method isFaceSturdy = optionalMethod(blockStateBase, "isFaceSturdy", blockGetter, blockPos, direction);
    private final Method isCollisionShapeFullBlock = optionalMethod(blockStateBase, "isCollisionShapeFullBlock", blockGetter, blockPos);
    private final Method getOffset = publicMethod(blockStateBase, "getOffset", blockPos);
    private final Method hasOffsetFunction = optionalMethod(blockStateBase, "hasOffsetFunction");
    private final Method hasBlockEntity = optionalMethod(blockStateBase, "hasBlockEntity");
    private final Method getFluidState = publicMethod(blockStateBase, "getFluidState");
    private final Method propagatesSkylightDown =
            publicMethod(blockStateBase, "propagatesSkylightDown");
    private final Method isSolid = optionalMethod(blockStateBase, "isSolid");
    private final Method isSolidRender = publicMethod(blockStateBase, "isSolidRender");
    private final Method canOcclude = publicMethod(blockStateBase, "canOcclude");
    private final Method hasDynamicShape = publicMethod(block, "hasDynamicShape");
    private final Method shapeToAabbs = publicMethod(voxelShape, "toAabbs");

    private final Constructor<?> blockPosConstructor = constructor(blockPos, int.class, int.class, int.class);
    private final Object emptyGetter = ReflectionAccess.staticField(emptyBlockGetter, "INSTANCE");
    private final Object zeroPos = ReflectionAccess.staticField(blockPos, "ZERO");
    private final Object emptyShape = invokeStatic(publicMethod(shapes, "empty"));
    private final Object blockShape = invokeStatic(publicMethod(shapes, "block"));
    private final Object[] directions = direction.getEnumConstants();
    private final Field aabbMinX = publicField(aabb, "minX");
    private final Field aabbMinY = publicField(aabb, "minY");
    private final Field aabbMinZ = publicField(aabb, "minZ");
    private final Field aabbMaxX = publicField(aabb, "maxX");
    private final Field aabbMaxY = publicField(aabb, "maxY");
    private final Field aabbMaxZ = publicField(aabb, "maxZ");
    private final Field vec3X = publicField(vec3, "x");
    private final Field vec3Y = publicField(vec3, "y");
    private final Field vec3Z = publicField(vec3, "z");
    private final Map<Class<?>, Method> maxHorizontalOffsetMethods = new HashMap<>();
    private final Map<Class<?>, Method> maxVerticalOffsetMethods = new HashMap<>();

    BlockStateSemanticsOracle(
            MinecraftRuntimeOracle runtime, IdentityHashMap<Object, String> fluidStateIds) {
        this.runtime = runtime;
        this.fluidStateIds = fluidStateIds;
        if (directions == null || directions.length != 6) {
            throw new IllegalStateException(
                    "expected six Minecraft directions, got "
                            + (directions == null ? "null" : directions.length));
        }
    }

    ExportData export() {
        long started = System.nanoTime();
        List<CatalogState> catalog = catalogStates(buildUnindexedCatalog());
        List<MinecraftRuntimeOracle.RegistryEntry> blockEntityTypes =
                runtime.registryEntries(runtime.registry("BLOCK_ENTITY_TYPE"));
        VoxelShapeRecords.Collector shapeCollector = new VoxelShapeRecords.Collector();
        IdentityHashMap<Object, OffsetSemantics> offsetsByBlock = new IdentityHashMap<>();
        List<Map<String, ?>> records = new ArrayList<>(catalog.size());

        for (CatalogState catalogState : catalog) {
            Object state = catalogState.state();
            ReflectionAccess.setProbeState(state);
            Object blockValue = catalogState.block();
            Map<String, String> faceShapeIds = new LinkedHashMap<>();
            Map<String, Boolean> faceSturdy = new LinkedHashMap<>();
            for (Object directionValue : directions) {
                Object faceShape = faceOcclusion(state, directionValue);
                faceSturdy.put(
                        directionName(directionValue),
                        faceSturdy(state, blockValue, directionValue));
                faceShapeIds.put(
                        directionName(directionValue),
                        recordFaceShape(shapeCollector, directionValue, faceShape));
            }

            Object stateFluid = invoke(getFluidState, state);
            String fluidStateId = fluidStateIds.get(stateFluid);
            if (fluidStateId == null) {
                throw new IllegalStateException(
                        "unmapped fluid state for " + catalogState.stateId());
            }

            boolean stateHasBlockEntity = hasBlockEntity != null ? booleanValue(invoke(hasBlockEntity, state))
                    : ReflectionAccess.type("net.minecraft.world.level.block.EntityBlock").isInstance(blockValue);
            List<String> validBlockEntityTypes = new ArrayList<>();
            for (MinecraftRuntimeOracle.RegistryEntry type : blockEntityTypes) {
                if (runtime.isValidBlockEntity(type.value(), state, blockValue)) {
                    validBlockEntityTypes.add(type.id());
                }
            }
            if (stateHasBlockEntity != !validBlockEntityTypes.isEmpty()) {
                throw new IllegalStateException(
                        "block entity semantic mismatch for "
                                + catalogState.stateId()
                                + ": hasBlockEntity="
                                + stateHasBlockEntity
                                + " validTypes="
                                + validBlockEntityTypes);
            }

            OffsetSemantics offset =
                    offsetsByBlock.computeIfAbsent(
                            blockValue, ignored -> classifyOffset(blockValue, state));
            int lightEmission = intValue(invoke(getLightEmission, state));
            int lightDampening = intValue(invoke(getLightDampening, state));
            if (lightEmission < 0 || lightEmission > 15) {
                throw new IllegalStateException(
                        "light emission outside [0,15] for "
                                + catalogState.stateId()
                                + ": "
                                + lightEmission);
            }
            if (lightDampening < 0 || lightDampening > 15) {
                throw new IllegalStateException(
                        "light dampening outside [0,15] for "
                                + catalogState.stateId()
                                + ": "
                                + lightDampening);
            }
            double shadeBrightness =
                    finiteNumber(
                            invoke(getShadeBrightness, state, emptyGetter, zeroPos),
                            "shade brightness",
                            catalogState.stateId());

            Map<String, Object> record = new LinkedHashMap<>();
            record.put("block_entity_type_ids", List.copyOf(validBlockEntityTypes));
            record.put("block_id", catalogState.blockId());
            boolean motion = !usesBlockTags && blocksMotion(state);
            record.put("blocks_fluid_flow", usesBlockTags ? blockTag(state, "BLOCKS_FLUID_FLOW") : motion);
            record.put("blocks_motion_in_heightmap", usesBlockTags ? blockTag(state, "BLOCKS_MOTION_IN_HEIGHTMAP") : flattenedMotionPredicate != null ? flattenedMotionPredicate.test(state) : motion);
            record.put("blocks_motion_in_heightmap_no_leaves", usesBlockTags ? blockTag(state, "BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES") : noLeavesPredicate.test(state));
            record.put("can_occlude", invoke(canOcclude, state));
            record.put("catalog_state_index", catalogState.catalogStateIndex());
            record.put(
                    "collision_full_empty_context",
                    collisionFull(state));
            record.put("collision_full_scope", "empty_context");
            record.put("dynamic_shape", invoke(hasDynamicShape, blockValue));
            record.put("emissive_rendering", emissiveRendering != null && booleanValue(invoke(emissiveRendering, state)));
            record.put("face_occlusion_shape_ids", faceShapeIds);
            record.put("face_sturdy_empty_context", faceSturdy);
            record.put("face_sturdy_scope", "empty_context");
            record.put("face_sturdy_support_type", "full");
            record.put("fluid_state_id", fluidStateId);
            record.put("has_block_entity", stateHasBlockEntity);
            record.put("has_large_collision_shape", largeCollision(state, blockValue));
            record.put("has_offset", offset.hasOffset());
            record.put("light_dampening", lightDampening);
            record.put("light_emission", lightEmission);
            record.put("light_permeable", isLightPermeable != null ? invoke(isLightPermeable, state) : !booleanValue(invoke(isSolidRender, state)) || lightDampening == 0);
            record.put("offset_kind", offset.kind());
            record.put("offset_max_horizontal", offset.maxHorizontal());
            record.put("offset_max_vertical", offset.maxVertical());
            record.put("propagates_skylight_down", invoke(propagatesSkylightDown, state));
            record.put("render_shape", String.valueOf(invoke(getRenderShape, state)));
            record.put("shade_brightness_empty_context", shadeBrightness);
            record.put("shade_brightness_scope", "empty_context");
            record.put("solid", stateSolid(state));
            record.put("solid_render", invoke(isSolidRender, state));
            record.put("state_id", catalogState.stateId());
            record.put("use_shape_for_light_occlusion", useShapeForLightOcclusion != null && booleanValue(invoke(useShapeForLightOcclusion, state)));
            records.add(record);
        }

        SkipRenderingOracle.ExportData skip = new SkipRenderingOracle().export(catalog);
        Map<String, Object> manifest = new LinkedHashMap<>();
        manifest.put("catalog_index_order", "state_id_ordinal");
        manifest.put("light_permeable_source", Boolean.getBoolean("nova.oracle.flattened")
                ? "compatibility_field:!IBlockState.isOpaqueCube(air-neighborhood,ORIGIN)||IBlockState.getOpacity(air-neighborhood,ORIGIN)==0;actual-runtime-values"
                : isLightPermeable != null ? "BlockBehaviour.BlockStateBase.isLightPermeable()" :
                ReflectionAccess.MAPPING.hasMethod(blockStateBase, "getLightDampening", 0)
                ? "compatibility_field:!BlockState.isSolidRender()||BlockState.getLightDampening()==0;actual-runtime-values"
                : getLightDampening.getParameterCount() == 0
                ? "compatibility_field:!BlockState.isSolidRender()||BlockState.getLightBlock()==0;actual-runtime-values"
                : "compatibility_field:!BlockState.isSolidRender(EmptyBlockGetter,ZERO)||BlockState.getLightBlock(EmptyBlockGetter,ZERO)==0;actual-runtime-values");
        manifest.put("light_dampening_source", Boolean.getBoolean("nova.oracle.flattened")
                ? "IBlockState.getOpacity(air-neighborhood,ORIGIN)"
                : ReflectionAccess.MAPPING.hasMethod(blockStateBase, "getLightDampening", 0)
                ? "BlockState.getLightDampening()" : getLightDampening.getParameterCount() == 0
                ? "BlockState.getLightBlock()" : "BlockState.getLightBlock(EmptyBlockGetter,ZERO)");
        manifest.put(
                "face_sturdy_source",
                isFaceSturdy != null ? "BlockBehaviour.BlockStateBase.isFaceSturdy(BlockGetter,BlockPos,Direction);SupportType.FULL"
                        : Boolean.getBoolean("nova.oracle.flattened")
                        ? "IBlockState.getBlockFaceShape(IBlockReader,BlockPos,EnumFacing)==BlockFaceShape.SOLID"
                        : "Block.hasSolidSide(BlockState,BlockGetter,BlockPos,Direction);MCP.func_220056_d");
        manifest.put(
                "face_sturdy_dynamic_shape_policy",
                "dynamic_shape=true:empty_context_probe_only;dynamic_shape=false:vanilla_cached_support");
        manifest.put("face_sturdy_not_occlusion_shape", true);
        manifest.put(
                "context_probe",
                Map.of(
                        "block_getter", Boolean.getBoolean("nova.oracle.flattened")
                                ? "MCP.IBlockReader(air-neighborhood;source-state;default-block-entity)"
                                : "net.minecraft.world.level.EmptyBlockGetter.INSTANCE",
                        "block_pos", List.of(0, 0, 0)));
        manifest.put(
                "face_projection_axes",
                Map.of(
                        "down", "u=x,v=z",
                        "east", "u=z,v=y",
                        "north", "u=x,v=y",
                        "south", "u=x,v=y",
                        "up", "u=x,v=z",
                        "west", "u=z,v=y"));
        manifest.put(
                "shape_kind_policy",
                "block_only_for_Shapes.block_identity;empty_only_for_Shapes.empty_identity;otherwise_rects");
        manifest.put(
                "offset_classifier",
                "bit_exact_vanilla_Mth.getSeed_XZ_XYZ_probe_without_block_id_rules");
        manifest.put("emissive_rendering_source", emissiveRendering == null
                ? "feature_absent_in_target_BlockState" : "target_BlockState.emissiveRendering");
        manifest.put("face_occlusion_source", getFaceOcclusionShape == null
                ? "IBlockState.isSolid()?boundary_cross_section_of_actual_IBlockState.getRenderShape_voxel_boxes:VoxelShapes.empty()"
                : "target_BlockState.getFaceOcclusionShape");
        manifest.put("light_shape_occlusion_source", useShapeForLightOcclusion == null ? "feature_absent_in_target_light_engine" : "target_BlockState.useShapeForLightOcclusion");
        manifest.put("offset_limit_source", "target_max_methods_or_complete_four_bit_runtime_offset_extrema");
        manifest.put("shape_record_count", shapeCollector.size());
        manifest.put("skip_rendering", skip.manifest());
        manifest.put("block_tag_predicate_source", Boolean.getBoolean("nova.oracle.flattened")
                ? "actual-runtime-MCP:IBlockState.getMaterial().blocksMovement();Heightmap.Type.readerAwarePredicates"
                : usesBlockTags
                ? "TypedInstance.is(TagKey);vanilla_server_data_tags"
                : ReflectionAccess.MAPPING.hasMethod(blockStateBase, "blocksMotion", 0)
                  ? "actual-runtime-methods:BlockState.blocksMotion();Heightmap.NO_LEAVES:LeavesBlock"
                  : "actual-runtime-methods:BlockState.getMaterial().blocksMotion();Heightmap.NO_LEAVES:LeavesBlock");
        String motionSource = ReflectionAccess.MAPPING.hasMethod(blockStateBase, "blocksMotion", 0)
                ? "BlockState.blocksMotion()" : "BlockState.getMaterial().blocksMotion()";
        manifest.put("block_tag_predicates", Boolean.getBoolean("nova.oracle.flattened")
                ? Map.of("blocks_fluid_flow", "IBlockState.getMaterial().blocksMovement()",
                         "blocks_motion_in_heightmap", "Heightmap.Type.MOTION_BLOCKING.readerAwarePredicate.test(IBlockState)",
                         "blocks_motion_in_heightmap_no_leaves", "Heightmap.Type.MOTION_BLOCKING_NO_LEAVES.readerAwarePredicate.test(IBlockState)")
                : usesBlockTags
                ? Map.of("blocks_fluid_flow", "minecraft:blocks_fluid_flow",
                         "blocks_motion_in_heightmap", "minecraft:blocks_motion_in_heightmap",
                         "blocks_motion_in_heightmap_no_leaves", "minecraft:blocks_motion_in_heightmap_no_leaves")
                : Map.of("blocks_fluid_flow", motionSource,
                         "blocks_motion_in_heightmap", motionSource,
                         "blocks_motion_in_heightmap_no_leaves", "Heightmap.Types.MOTION_BLOCKING_NO_LEAVES.isOpaque().test(BlockState)"));
        manifest.put("state_count", catalog.size());
        return new ExportData(
                List.copyOf(records),
                shapeCollector.records(),
                skip.records(),
                manifest,
                skip.evaluatedComparisons(),
                skip.elapsedMillis(),
                Duration.ofNanos(System.nanoTime() - started).toMillis());
    }

    private List<UnindexedCatalogState> buildUnindexedCatalog() {
        List<UnindexedCatalogState> result = new ArrayList<>();
        for (MinecraftRuntimeOracle.RegistryEntry blockEntry :
                runtime.registryEntries(runtime.registry("BLOCK"))) {
            Object blockValue = blockEntry.value();
            Object definition =
                    ReflectionAccess.invoke(
                            blockValue, block, "getStateDefinition", new Class<?>[0]);
            for (Object state : runtime.possibleStates(definition)) {
                String stateId =
                        StateIds.canonical(blockEntry.id(), runtime.stateProperties(state));
                result.add(
                        new UnindexedCatalogState(
                                stateId, blockEntry.id(), blockValue, state));
            }
        }
        return result;
    }

    static List<CatalogState> catalogStates(List<UnindexedCatalogState> unindexed) {
        List<UnindexedCatalogState> ordered = new ArrayList<>(unindexed);
        ordered.sort(Comparator.comparing(UnindexedCatalogState::stateId));
        Set<String> ids = new HashSet<>();
        List<CatalogState> result = new ArrayList<>(ordered.size());
        for (int index = 0; index < ordered.size(); index++) {
            UnindexedCatalogState state = ordered.get(index);
            if (!ids.add(state.stateId())) {
                throw new IllegalArgumentException("duplicate catalog state_id: " + state.stateId());
            }
            result.add(
                    new CatalogState(
                            index,
                            state.stateId(),
                            state.blockId(),
                            state.block(),
                            state.state()));
        }
        return List.copyOf(result);
    }

    private String recordFaceShape(
            VoxelShapeRecords.Collector collector, Object directionValue, Object shape) {
        if (shape == blockShape) {
            return collector.add("block", List.of());
        }
        if (shape == emptyShape) {
            return collector.add("empty", List.of());
        }
        @SuppressWarnings("unchecked")
        List<Object> boxes = (List<Object>) invoke(shapeToAabbs, shape);
        List<VoxelShapeRecords.Rectangle> rectangles = new ArrayList<>(boxes.size());
        String face = directionName(directionValue);
        for (Object box : boxes) {
            double minX = fieldDouble(aabbMinX, box);
            double minY = fieldDouble(aabbMinY, box);
            double minZ = fieldDouble(aabbMinZ, box);
            double maxX = fieldDouble(aabbMaxX, box);
            double maxY = fieldDouble(aabbMaxY, box);
            double maxZ = fieldDouble(aabbMaxZ, box);
            if (getFaceOcclusionShape == null) {
                boolean intersects = switch(face) {
                    case "down" -> minY <= 0.0 && maxY > 0.0;
                    case "up" -> minY < 1.0 && maxY >= 1.0;
                    case "north" -> minZ <= 0.0 && maxZ > 0.0;
                    case "south" -> minZ < 1.0 && maxZ >= 1.0;
                    case "west" -> minX <= 0.0 && maxX > 0.0;
                    case "east" -> minX < 1.0 && maxX >= 1.0;
                    default -> throw new IllegalStateException("unknown face " + face);
                };
                if (!intersects) continue;
            }
            rectangles.add(
                    switch (face) {
                        case "down", "up" ->
                                VoxelShapeRecords.rectangle(minX, minZ, maxX, maxZ);
                        case "north", "south" ->
                                VoxelShapeRecords.rectangle(minX, minY, maxX, maxY);
                        case "west", "east" ->
                                VoxelShapeRecords.rectangle(minZ, minY, maxZ, maxY);
                        default -> throw new IllegalStateException("unknown direction " + face);
                    });
        }
        return collector.add("rects", rectangles);
    }

    private OffsetSemantics classifyOffset(Object blockValue, Object representativeState) {
        boolean hasOffset;
        if (hasOffsetFunction != null) hasOffset = booleanValue(invoke(hasOffsetFunction, representativeState));
        else {
            Object kind = ReflectionAccess.MAPPING.hasMethod(blockStateBase, "getOffsetType", 0)
                    ? ReflectionAccess.MAPPING.invoke(representativeState, blockStateBase, "getOffsetType")
                    : ReflectionAccess.MAPPING.invoke(blockValue, blockValue.getClass(), "getOffsetType");
            hasOffset = !((Enum<?>) kind).name().equals("NONE");
        }
        if (!hasOffset) {
            return new OffsetSemantics(false, "none", 0.0D, 0.0D);
        }
        double maxHorizontal;
        double maxVertical;
        if (ReflectionAccess.MAPPING.hasMethod(blockValue.getClass(), "getMaxHorizontalOffset", 0)
                && ReflectionAccess.MAPPING.hasMethod(blockValue.getClass(), "getMaxVerticalOffset", 0)) {
            maxHorizontal = finiteNumber(invoke(maxHorizontalOffsetMethods.computeIfAbsent(blockValue.getClass(),
                    type -> effectiveMethod(type, "getMaxHorizontalOffset")), blockValue), "maximum horizontal offset", blockValue.getClass().getName());
            maxVertical = finiteNumber(invoke(maxVerticalOffsetMethods.computeIfAbsent(blockValue.getClass(),
                    type -> effectiveMethod(type, "getMaxVerticalOffset")), blockValue), "maximum vertical offset", blockValue.getClass().getName());
        } else {
            // Older vanilla embeds the limits in getOffset. Measure its actual
            // full 4-bit component domains, rather than borrowing later limits.
            maxHorizontal = 0; maxVertical = 0;
            int xDomain = 0, yDomain = 0, zDomain = 0;
            for (int[] probe : OFFSET_PROBES) {
                long seed = minecraftSeed(probe[0], 0, probe[2]);
                xDomain |= 1 << (int)(seed & 15); yDomain |= 1 << (int)((seed >> 4) & 15); zDomain |= 1 << (int)((seed >> 8) & 15);
                Object offset = invoke(getOffset, representativeState, constructBlockPos(probe[0], 0, probe[2]));
                maxHorizontal = Math.max(maxHorizontal, Math.max(Math.abs(fieldDouble(vec3X, offset)), Math.abs(fieldDouble(vec3Z, offset))));
                maxVertical = Math.max(maxVertical, Math.abs(fieldDouble(vec3Y, offset)));
            }
            if (xDomain != 65535 || yDomain != 65535 || zDomain != 65535 || maxHorizontal <= 0)
                throw new IllegalStateException("incomplete actual runtime offset domain for " + blockValue.getClass().getName());
        }
        boolean matchesXz = true;
        boolean matchesXyz = true;
        for (int[] probe : OFFSET_PROBES) {
            Object position = constructBlockPos(probe[0], probe[1], probe[2]);
            Object offset = invoke(getOffset, representativeState, position);
            double x = fieldDouble(vec3X, offset);
            double y = fieldDouble(vec3Y, offset);
            double z = fieldDouble(vec3Z, offset);
            double[] xz = vanillaOffset(probe[0], probe[2], maxHorizontal, maxVertical, false);
            double[] xyz = vanillaOffset(probe[0], probe[2], maxHorizontal, maxVertical, true);
            matchesXz &= sameVector(x, y, z, xz);
            matchesXyz &= sameVector(x, y, z, xyz);
        }
        // With an actual zero vertical range, XZ and XYZ are numerically
        // identical. XZ stores the same complete geometric behavior in that case.
        if (matchesXz && matchesXyz && maxVertical == 0.0D) matchesXyz = false;
        if (matchesXz == matchesXyz) {
            throw new IllegalStateException(
                    "offset function does not uniquely match vanilla XZ/XYZ for "
                            + blockValue.getClass().getName()
                            + ": matchesXz="
                            + matchesXz
                            + " matchesXyz="
                            + matchesXyz);
        }
        return new OffsetSemantics(
                true, matchesXyz ? "xyz" : "xz", maxHorizontal, matchesXyz ? maxVertical : 0.0D);
    }

    private static double[] vanillaOffset(
            int x, int z, double maxHorizontal, double maxVertical, boolean xyz) {
        long seed = minecraftSeed(x, 0, z);
        double offsetX =
                clamp(
                        ((double) ((float) (seed & 15L) / 15.0F) - 0.5D) * 0.5D,
                        -maxHorizontal,
                        maxHorizontal);
        double offsetZ =
                clamp(
                        ((double) ((float) (seed >> 8 & 15L) / 15.0F) - 0.5D) * 0.5D,
                        -maxHorizontal,
                        maxHorizontal);
        double offsetY =
                xyz
                        ? ((double) ((float) (seed >> 4 & 15L) / 15.0F) - 1.0D)
                                * maxVertical
                        : 0.0D;
        return new double[] {
            coordinate(offsetX), coordinate(offsetY), coordinate(offsetZ)
        };
    }

    private static long minecraftSeed(int x, int y, int z) {
        long seed = (long) (x * 3_129_871) ^ (long) z * 116_129_781L ^ (long) y;
        seed = seed * seed * 42_317_861L + seed * 11L;
        return seed >> 16;
    }

    private static double clamp(double value, double minimum, double maximum) {
        return value < minimum ? minimum : Math.min(value, maximum);
    }

    private static boolean sameVector(double x, double y, double z, double[] expected) {
        return Double.doubleToLongBits(x) == Double.doubleToLongBits(expected[0])
                && Double.doubleToLongBits(y) == Double.doubleToLongBits(expected[1])
                && Double.doubleToLongBits(z) == Double.doubleToLongBits(expected[2]);
    }

    private Method effectiveMethod(Class<?> concreteClass, String name) {
        Class<?> current = concreteClass;
        while (current != null) {
            try {
                Method method = ReflectionAccess.method(current, name);
                method.setAccessible(true);
                return method;
            } catch (IllegalStateException ignored) {
                if (current == blockBehaviour) {
                    break;
                }
                current = current.getSuperclass();
            }
        }
        throw new IllegalStateException(
                "cannot resolve " + name + " for " + concreteClass.getName());
    }

    private Object constructBlockPos(int x, int y, int z) {
        try {
            return blockPosConstructor.newInstance(x, y, z);
        } catch (InvocationTargetException error) {
            throw propagate("BlockPos construction failed", error.getCause());
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("cannot construct BlockPos", error);
        }
    }

    private static Method optionalContextMethod(Class<?> owner, String name) {
        try { return publicMethod(owner, name); }
        catch (IllegalStateException missing) { return null; }
    }

    private static Method optionalMethod(Class<?> owner, String name, Class<?>... parameters) {
        try { return ReflectionAccess.method(owner, name, parameters); }
        catch (IllegalStateException missing) { return null; }
    }

    private static Method publicMethod(Class<?> owner, String name, Class<?>... parameterTypes) {
        try {
            return ReflectionAccess.method(owner, name, parameterTypes);
        } catch (IllegalStateException error) {
            String legacyName = name.equals("getLightDampening") ? "getLightBlock" : name;
            if (name.equals("getShadeBrightness") && parameterTypes.length == 2) {
                try { return ReflectionAccess.method(owner, name); }
                catch (IllegalStateException missing) { error.addSuppressed(missing); }
            }
            Class<?> getter = ReflectionAccess.type("net.minecraft.world.level.BlockGetter");
            Class<?> pos = ReflectionAccess.type("net.minecraft.core.BlockPos");
            boolean hasPosition = java.util.Arrays.asList(parameterTypes).contains(pos);
            int prefix = hasPosition ? 1 : 2;
            Class<?>[] contextual = new Class<?>[parameterTypes.length + prefix];
            contextual[0] = getter;
            if (!hasPosition) contextual[1] = pos;
            System.arraycopy(parameterTypes, 0, contextual, prefix, parameterTypes.length);
            try { return ReflectionAccess.method(owner, legacyName, parameterTypes); }
            catch (IllegalStateException missing) { error.addSuppressed(missing); }
            try { return ReflectionAccess.method(owner, legacyName, contextual); }
            catch (IllegalStateException missing) { error.addSuppressed(missing); }
            throw new IllegalStateException("missing method " + owner.getName() + "#" + name, error);
        }
    }

    private static Constructor<?> constructor(Class<?> owner, Class<?>... parameterTypes) {
        try {
            return owner.getConstructor(parameterTypes);
        } catch (NoSuchMethodException error) {
            throw new IllegalStateException("missing constructor " + owner.getName(), error);
        }
    }

    private static Field publicField(Class<?> owner, String name) {
        try {
            return ReflectionAccess.fieldHandle(owner, name);
        } catch (IllegalStateException error) {
            throw new IllegalStateException("missing field " + owner.getName() + "#" + name, error);
        }
    }

    private static Object invokeStatic(Method method, Object... arguments) {
        return invoke(method, null, arguments);
    }

    private static Object invoke(Method method, Object target, Object... arguments) {
        try {
            if (method.getParameterCount() > arguments.length) {
                int extra = method.getParameterCount() - arguments.length;
                Object[] contextual = new Object[method.getParameterCount()];
                contextual[0] = ReflectionAccess.staticField(ReflectionAccess.type("net.minecraft.world.level.EmptyBlockGetter"), "INSTANCE");
                if (extra == 2) contextual[1] = ReflectionAccess.staticField(ReflectionAccess.type("net.minecraft.core.BlockPos"), "ZERO");
                System.arraycopy(arguments, 0, contextual, extra, arguments.length);
                return method.invoke(target, contextual);
            }
            if (method.getParameterCount() == 0 && arguments.length == 2) return method.invoke(target);
            return method.invoke(target, arguments);
        } catch (InvocationTargetException error) {
            throw propagate(
                    "runtime invocation failed: "
                            + method.getDeclaringClass().getName()
                            + "#"
                            + method.getName(),
                    error.getCause());
        } catch (IllegalAccessException error) {
            throw new IllegalStateException(
                    "cannot invoke "
                            + method.getDeclaringClass().getName()
                            + "#"
                            + method.getName(),
                    error);
        }
    }

    private static IllegalStateException propagate(String message, Throwable cause) {
        if (cause instanceof RuntimeException runtimeException) {
            throw runtimeException;
        }
        if (cause instanceof Error invocationError) {
            throw invocationError;
        }
        return new IllegalStateException(message, cause);
    }

    private static boolean booleanValue(Object value) {
        return (Boolean) value;
    }

    private static int intValue(Object value) {
        return ((Number) value).intValue();
    }

    private static double finiteNumber(Object value, String field, String stateId) {
        double number = ((Number) value).doubleValue();
        if (!Double.isFinite(number)) {
            throw new IllegalStateException(
                    field + " is non-finite for " + stateId + ": " + number);
        }
        return VoxelShapeRecords.coordinate(number);
    }

    private static double fieldDouble(Field field, Object target) {
        try {
            return VoxelShapeRecords.coordinate(field.getDouble(target));
        } catch (IllegalAccessException error) {
            throw new IllegalStateException(
                    "cannot read " + field.getDeclaringClass().getName() + "#" + field.getName(),
                    error);
        }
    }

    private static String directionName(Object directionValue) {
        return ((Enum<?>) directionValue).name().toLowerCase(Locale.ROOT);
    }

    static String classifyOffsetKind(List<double[]> offsets) {
        boolean horizontal = false;
        boolean vertical = false;
        for (double[] offset : offsets) {
            if (offset.length != 3) {
                throw new IllegalArgumentException("offset probe must have three coordinates");
            }
            horizontal |= coordinate(offset[0]) != 0.0D || coordinate(offset[2]) != 0.0D;
            vertical |= coordinate(offset[1]) != 0.0D;
        }
        if (!horizontal) {
            throw new IllegalArgumentException("offset probes contain no horizontal displacement");
        }
        return vertical ? "xyz" : "xz";
    }

    private static double coordinate(double value) {
        return VoxelShapeRecords.coordinate(value);
    }

    private static int[][] createOffsetProbes() {
        int[] values = {-31, -17, -7, -1, 0, 1, 2, 5, 11, 23};
        int[][] result = new int[values.length * values.length][3];
        int index = 0;
        for (int x : values) {
            for (int z : values) {
                result[index++] = new int[] {x, 0, z};
            }
        }
        return result;
    }

    record ExportData(
            List<Map<String, ?>> semantics,
            List<Map<String, ?>> shapes,
            List<Map<String, ?>> skipRows,
            Map<String, Object> manifest,
            long skipEvaluatedComparisons,
            long skipElapsedMillis,
            long elapsedMillis) {}

    record UnindexedCatalogState(String stateId, String blockId, Object block, Object state) {}

    record CatalogState(
            int catalogStateIndex,
            String stateId,
            String blockId,
            Object block,
            Object state) {}

    private record OffsetSemantics(
            boolean hasOffset, String kind, double maxHorizontal, double maxVertical) {}
}
