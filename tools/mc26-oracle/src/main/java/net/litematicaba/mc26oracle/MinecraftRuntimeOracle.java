package net.litematicaba.mc26oracle;

import java.io.IOException;
import java.net.URISyntaxException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.stream.Stream;

final class MinecraftRuntimeOracle {
    private static final Class<?>[] NO_TYPES = new Class<?>[0];
    static final int MANIFEST_SCHEMA_VERSION = 3;
    private final String expectedMinecraft;
    private final String expectedClientJarSha256;
    private final boolean includeServerData;

    MinecraftRuntimeOracle(String minecraft, String clientJarSha256, boolean includeServerData) {
        expectedMinecraft = minecraft;
        this.includeServerData = includeServerData;
        expectedClientJarSha256 = clientJarSha256;
    }

    private final Class<?> sharedConstants = ReflectionAccess.type("net.minecraft.SharedConstants");
    private final Class<?> bootstrap = ReflectionAccess.type("net.minecraft.server.Bootstrap");

    {
        bootMinecraft();
    }

    private final Class<?> worldVersion = Boolean.getBoolean("nova.oracle.flattened") ? null : ReflectionAccess.type("net.minecraft.WorldVersion");
    private final Class<?> dataVersion =
            ReflectionAccess.MAPPING.typeOrNull("net.minecraft.world.level.storage.DataVersion");
    private final Class<?> builtInRegistries =
            ReflectionAccess.type("net.minecraft.core.registries.BuiltInRegistries");
    private final Class<?> registryType = ReflectionAccess.type("net.minecraft.core.Registry");
    private final Class<?> defaultedRegistry =
            ReflectionAccess.type("net.minecraft.core.DefaultedRegistry");
    private final Class<?> identifier = ReflectionAccess.type("net.minecraft.resources.Identifier");
    private final Class<?> stateDefinition =
            ReflectionAccess.type("net.minecraft.world.level.block.state.StateDefinition");
    private final Class<?> stateHolder =
            ReflectionAccess.type("net.minecraft.world.level.block.state.StateHolder");
    private final Class<?> block = ReflectionAccess.type("net.minecraft.world.level.block.Block");
    private final Class<?> blockStateBase =
            ReflectionAccess.type(
                    "net.minecraft.world.level.block.state.BlockBehaviour$BlockStateBase");
    private final Class<?> fluid = ReflectionAccess.type("net.minecraft.world.level.material.Fluid");
    private final Class<?> fluidState =
            ReflectionAccess.type("net.minecraft.world.level.material.FluidState");
    private final Class<?> idMapper = ReflectionAccess.type("net.minecraft.core.IdMapper");
    private final Class<?> blockEntityType =
            ReflectionAccess.type("net.minecraft.world.level.block.entity.BlockEntityType");
    private final Class<?> blockState =
            ReflectionAccess.type("net.minecraft.world.level.block.state.BlockState");
    private final Class<?> entityType = ReflectionAccess.type("net.minecraft.world.entity.EntityType");
    private final Class<?> entityDimensions =
            ReflectionAccess.MAPPING.typeOrNull("net.minecraft.world.entity.EntityDimensions");
    private final Class<?> particleType =
            ReflectionAccess.type("net.minecraft.core.particles.ParticleType");

    ExportResult export(Path root) throws IOException {
        RuntimeIdentity identity = verifyRuntimeIdentity();
        List<Artifact> artifacts = new ArrayList<>();
        IdentityHashMap<Object, String> fluidStateIds = new IdentityHashMap<>();
        if (!Boolean.getBoolean("nova.oracle.flattened")) artifacts.add(exportBuiltinRegistries(root));
        artifacts.add(exportFluidStates(root, fluidStateIds));
        artifacts.add(exportBlockStates(root, fluidStateIds));
        artifacts.add(exportBlockEntityTypes(root));
        if (includeServerData) {
        artifacts.add(exportEntityTypes(root));
        artifacts.add(exportParticleTypes(root));
        MinecraftServerDataOracle.ExportData serverData =
                new MinecraftServerDataOracle().load();
        artifacts.add(
                write(
                        root,
                        "data_registries",
                        serverData.dataRegistries(),
                        List.of("registry_id", "entry_id")));
        artifacts.add(
                write(
                        root,
                        "tags",
                        serverData.tags(),
                        List.of("registry_id", "tag_id", "member_index")));
        }
        BlockStateSemanticsOracle.ExportData semantics =
                new BlockStateSemanticsOracle(this, fluidStateIds).export();
        artifacts.add(
                write(
                        root,
                        "block_state_semantics",
                        semantics.semantics(),
                        List.of("state_id")));
        artifacts.add(
                write(
                        root,
                        "face_occlusion_shapes",
                        semantics.shapes(),
                        List.of("shape_id")));
        artifacts.add(
                write(
                        root,
                        "skip_render_rows",
                        semantics.skipRows(),
                        List.of("profile", "source_catalog_state_index", "direction")));
        writeManifest(root, artifacts, identity, semantics.manifest());
        return new ExportResult(
                List.copyOf(artifacts),
                semantics.skipEvaluatedComparisons(),
                semantics.skipElapsedMillis(),
                semantics.elapsedMillis());
    }

    private void bootMinecraft() {
        if (Boolean.getBoolean("nova.oracle.flattened")) {
            ReflectionAccess.invokeStatic(bootstrap, "bootStrap", NO_TYPES); return;
        }
        if (ReflectionAccess.MAPPING.hasMethod(sharedConstants, "tryDetectVersion", 0)) {
            ReflectionAccess.invokeStatic(sharedConstants, "tryDetectVersion", NO_TYPES);
        } else {
            // Older clients initialize this lazily; official mappings can also
            // contain a server-only setVersion declaration absent from the client.
            ReflectionAccess.MAPPING.invokeStatic(sharedConstants, "getCurrentVersion");
        }
        ReflectionAccess.invokeStatic(bootstrap, "bootStrap", NO_TYPES);
    }

    private RuntimeIdentity verifyRuntimeIdentity() throws IOException {
        String minecraft; int protocol; int world;
        if (Boolean.getBoolean("nova.oracle.flattened")) {
            Class<?> constants = ReflectionAccess.type("net.minecraft.realms.RealmsSharedConstants");
            minecraft = (String) ReflectionAccess.staticField(constants, "VERSION_STRING");
            protocol = (Integer) ReflectionAccess.staticField(constants, "NETWORK_PROTOCOL_VERSION");
            world = Integer.parseInt(System.getProperty("nova.oracle.worldVersion"));
        } else {
            Object currentVersion = ReflectionAccess.invokeStatic(sharedConstants, "getCurrentVersion", NO_TYPES);
            minecraft = (String) ReflectionAccess.invokeCompatible(currentVersion, worldVersion, new String[]{"name", "getName"});
            protocol = (Integer) ReflectionAccess.invokeCompatible(currentVersion, worldVersion, new String[]{"protocolVersion", "getProtocolVersion"});
            if (dataVersion != null && (ReflectionAccess.MAPPING.hasMethod(worldVersion, "dataVersion", 0)
                    || ReflectionAccess.MAPPING.hasMethod(worldVersion, "getDataVersion", 0))) {
                Object currentDataVersion = ReflectionAccess.invokeCompatible(currentVersion, worldVersion, new String[]{"dataVersion", "getDataVersion"});
                world = (Integer) ReflectionAccess.invokeCompatible(currentDataVersion, dataVersion, new String[]{"version", "getVersion"});
            } else world = (Integer) ReflectionAccess.MAPPING.invoke(currentVersion, worldVersion, "getWorldVersion");
        }
        if (!expectedMinecraft.equals(minecraft) || protocol <= 0 || world <= 0) {
            throw new IllegalStateException(
                    "unexpected Minecraft runtime: name="
                            + minecraft
                            + " protocol="
                            + protocol
                            + " world="
                            + world);
        }

        Path clientJar;
        try {
            var codeSource = sharedConstants.getProtectionDomain().getCodeSource();
            if (codeSource == null) {
                throw new IllegalStateException("Minecraft runtime has no code source");
            }
            clientJar = Path.of(codeSource.getLocation().toURI()).toAbsolutePath().normalize();
        } catch (URISyntaxException error) {
            throw new IOException("invalid Minecraft client JAR location", error);
        }
        if (!Files.isRegularFile(clientJar)) {
            throw new IllegalStateException(
                    "Minecraft runtime code source is not a JAR: " + clientJar);
        }
        long clientJarSize = Files.size(clientJar);
        String clientJarSha256 = CanonicalJsonl.sha256(clientJar);
        if (!expectedClientJarSha256.equals(clientJarSha256)) {
            throw new IllegalStateException(
                    "unexpected Minecraft client JAR: size="
                            + clientJarSize
                            + " sha256="
                            + clientJarSha256);
        }
        return new RuntimeIdentity(
                minecraft, world, protocol, clientJarSize, clientJarSha256);
    }

    private Artifact exportBuiltinRegistries(Path root) throws IOException {
        Object registries = registry("REGISTRY");
        List<Map<String, ?>> records = new ArrayList<>();
        for (RegistryEntry registryEntry : registryEntries(registries)) {
            Object nested = registryEntry.value();
            String defaultId =
                    defaultedRegistry.isInstance(nested)
                            ? String.valueOf(
                                    ReflectionAccess.invoke(
                                            nested,
                                            defaultedRegistry,
                                            "getDefaultKey",
                                            NO_TYPES))
                            : null;
            for (RegistryEntry entry : registryEntries(nested)) {
                Map<String, Object> record = new LinkedHashMap<>();
                record.put("default_entry_id", defaultId);
                record.put("entry_id", entry.id());
                record.put("raw_id", entry.rawId());
                record.put("registry_id", registryEntry.id());
                records.add(record);
            }
        }
        return write(root, "builtin_registries", records, List.of("registry_id", "entry_id"));
    }

    private Object fluidOwnHeight(Object state) {
        if (ReflectionAccess.MAPPING.hasMethod(fluidState, "getOwnHeight", 0))
            return ReflectionAccess.invoke(state, fluidState, "getOwnHeight", NO_TYPES);
        if (ReflectionAccess.MAPPING.hasMethod(fluidState, "func_223408_f", 0))
            return ReflectionAccess.MAPPING.invoke(state, fluidState, "func_223408_f");
        // Early 1.14 exposes only context-dependent height. Empty space above
        // the target state produces that state's actual own height.
        return ReflectionAccess.MAPPING.invoke(state, fluidState, "func_215679_a",
                ReflectionAccess.staticField(ReflectionAccess.type("net.minecraft.world.level.EmptyBlockGetter"), "INSTANCE"),
                ReflectionAccess.staticField(ReflectionAccess.type("net.minecraft.core.BlockPos"), "ZERO"));
    }

    private Artifact exportFluidStates(Path root, IdentityHashMap<Object, String> stateIds)
            throws IOException {
        List<Map<String, ?>> records = new ArrayList<>();
        Object rawStates = ReflectionAccess.staticField(fluid, "FLUID_STATE_REGISTRY");
        for (RegistryEntry fluidEntry : registryEntries(registry("FLUID"))) {
            Object fluidValue = fluidEntry.value();
            Object definition =
                    ReflectionAccess.invoke(fluidValue, fluid, "getStateDefinition", NO_TYPES);
            Object defaultState =
                    ReflectionAccess.invoke(fluidValue, fluid, "defaultFluidState", NO_TYPES);
            for (Object state : possibleStates(definition)) {
                ReflectionAccess.setProbeState(state);
                Map<String, String> properties = stateProperties(state);
                String stateId = StateIds.canonical(fluidEntry.id(), properties);
                stateIds.put(state, stateId);

                Map<String, Object> record = new LinkedHashMap<>();
                record.put("amount", ReflectionAccess.invoke(state, fluidState, "getAmount", NO_TYPES));
                record.put("default_state", state == defaultState);
                record.put("empty", ReflectionAccess.invoke(state, fluidState, "isEmpty", NO_TYPES));
                record.put("fluid_id", fluidEntry.id());
                record.put(
                        "own_height",
                        fluidOwnHeight(state));
                record.put("properties", properties);
                record.put(
                        "raw_id",
                        ReflectionAccess.invoke(
                                rawStates,
                                idMapper,
                                "getId",
                                new Class<?>[] {Object.class},
                                state));
                record.put("is_source", ReflectionAccess.invoke(state, fluidState, "isSource", NO_TYPES));
                record.put("state_id", stateId);
                records.add(record);
            }
        }
        return write(root, "fluid_states", records, List.of("state_id"));
    }

    private Artifact exportBlockStates(
            Path root, IdentityHashMap<Object, String> fluidStateIds) throws IOException {
        List<Map<String, ?>> records = new ArrayList<>();
        Object rawStates = ReflectionAccess.staticField(block, "BLOCK_STATE_REGISTRY");
        for (RegistryEntry blockEntry : registryEntries(registry("BLOCK"))) {
            Object blockValue = blockEntry.value();
            Object definition =
                    ReflectionAccess.invoke(blockValue, block, "getStateDefinition", NO_TYPES);
            Object defaultState =
                    ReflectionAccess.invoke(blockValue, block, "defaultBlockState", NO_TYPES);
            for (Object state : possibleStates(definition)) {
                ReflectionAccess.setProbeState(state);
                Map<String, String> properties = stateProperties(state);
                String stateId = StateIds.canonical(blockEntry.id(), properties);
                Object fluid =
                        ReflectionAccess.invoke(state, blockStateBase, "getFluidState", NO_TYPES);
                String fluidId = fluidStateIds.get(fluid);
                if (fluidId == null) {
                    throw new IllegalStateException("unmapped fluid state for " + stateId);
                }

                Map<String, Object> record = new LinkedHashMap<>();
                record.put("air", ReflectionAccess.invoke(state, blockStateBase, "isAir", NO_TYPES));
                record.put("block_id", blockEntry.id());
                record.put(
                        "can_occlude",
                        ReflectionAccess.invoke(state, blockStateBase, "canOcclude", NO_TYPES));
                record.put("default_state", state == defaultState);
                record.put("fluid_state_id", fluidId);
                record.put(
                        "light_emission",
                        ReflectionAccess.invoke(state, blockStateBase, "getLightEmission", NO_TYPES));
                record.put("properties", properties);
                record.put(
                        "raw_id",
                        ReflectionAccess.invoke(
                                rawStates,
                                idMapper,
                                "getId",
                                new Class<?>[] {Object.class},
                                state));
                record.put(
                        "render_shape",
                        String.valueOf(
                                ReflectionAccess.invoke(
                                        state, blockStateBase, "getRenderShape", NO_TYPES)));
                record.put("solid", ReflectionAccess.invoke(state, blockStateBase, "isSolid", NO_TYPES));
                record.put(
                        "solid_render",
                        ReflectionAccess.invoke(state, blockStateBase, "isSolidRender", NO_TYPES));
                record.put("state_id", stateId);
                records.add(record);
            }
        }
        return write(root, "block_states", records, List.of("state_id"));
    }

    private Artifact exportBlockEntityTypes(Path root) throws IOException {
        List<RegistryEntry> blocks = registryEntries(registry("BLOCK"));
        List<Map<String, ?>> records = new ArrayList<>();
        for (RegistryEntry typeEntry : registryEntries(registry("BLOCK_ENTITY_TYPE"))) {
            List<String> validBlocks = new ArrayList<>();
            for (RegistryEntry blockEntry : blocks) {
                Object defaultState =
                        ReflectionAccess.invoke(
                                blockEntry.value(), block, "defaultBlockState", NO_TYPES);
                boolean valid =
                        isValidBlockEntity(typeEntry.value(), defaultState, blockEntry.value());
                if (valid) {
                    validBlocks.add(blockEntry.id());
                }
            }
            validBlocks.sort(String::compareTo);
            records.add(
                    Map.of(
                            "raw_id", typeEntry.rawId(),
                            "type_id", typeEntry.id(),
                            "valid_block_ids", validBlocks));
        }
        return write(root, "block_entity_types", records, List.of("type_id"));
    }

    private Artifact exportEntityTypes(Path root) throws IOException {
        List<Map<String, ?>> records = new ArrayList<>();
        for (RegistryEntry entry : registryEntries(registry("ENTITY_TYPE"))) {
            Object value = entry.value();
            Object dimensions =
                    ReflectionAccess.invoke(value, entityType, "getDimensions", NO_TYPES);
            Map<String, Object> record = new LinkedHashMap<>();
            record.put(
                    "category",
                    String.valueOf(
                            ReflectionAccess.invoke(value, entityType, "getCategory", NO_TYPES)));
            record.put(
                    "client_tracking_range",
                    ReflectionAccess.invoke(value, entityType, "clientTrackingRange", NO_TYPES));
            record.put(
                    "eye_height",
                    ReflectionAccess.invoke(dimensions, entityDimensions, "eyeHeight", NO_TYPES));
            record.put(
                    "fixed",
                    ReflectionAccess.invoke(dimensions, entityDimensions, "fixed", NO_TYPES));
            record.put(
                    "height",
                    ReflectionAccess.invoke(dimensions, entityDimensions, "height", NO_TYPES));
            record.put("raw_id", entry.rawId());
            record.put(
                    "track_deltas",
                    ReflectionAccess.invoke(value, entityType, "trackDeltas", NO_TYPES));
            record.put("type_id", entry.id());
            record.put(
                    "update_interval",
                    ReflectionAccess.invoke(value, entityType, "updateInterval", NO_TYPES));
            record.put(
                    "width",
                    ReflectionAccess.invoke(dimensions, entityDimensions, "width", NO_TYPES));
            records.add(record);
        }
        return write(root, "entity_types", records, List.of("type_id"));
    }

    private Artifact exportParticleTypes(Path root) throws IOException {
        List<Map<String, ?>> records = new ArrayList<>();
        for (RegistryEntry entry : registryEntries(registry("PARTICLE_TYPE"))) {
            records.add(
                    Map.of(
                            "override_limiter",
                            ReflectionAccess.invoke(
                                    entry.value(), particleType, "getOverrideLimiter", NO_TYPES),
                            "raw_id",
                            entry.rawId(),
                            "type_id",
                            entry.id()));
        }
        return write(root, "particle_types", records, List.of("type_id"));
    }

    private final IdentityHashMap<Object,Object> legacyBlockEntityTypes = new IdentityHashMap<>();
    boolean isValidBlockEntity(Object type, Object state, Object blockValue) {
        if (!ReflectionAccess.MAPPING.hasMethod(blockEntityType, "isValid", 1)) {
            Class<?> provider = ReflectionAccess.type("net.minecraft.world.level.block.EntityBlock");
            if (!provider.isInstance(blockValue)) return false;
            Object actual = legacyBlockEntityTypes.computeIfAbsent(blockValue, value -> {
                Object context = ReflectionAccess.staticField(ReflectionAccess.type("net.minecraft.world.level.EmptyBlockGetter"), "INSTANCE");
                Object instance = ReflectionAccess.MAPPING.invoke(value, provider, "newBlockEntity", context);
                if (instance == null && ReflectionAccess.MAPPING.hasMethod(value.getClass(), "func_196343_a", 4)) {
                    // Moving pistons intentionally return null from the generic
                    // provider; their actual stateful vanilla factory owns the type.
                    Object facing = ReflectionAccess.staticField(ReflectionAccess.type("net.minecraft.core.Direction"), "NORTH");
                    instance = ReflectionAccess.MAPPING.invokeStatic(value.getClass(), "func_196343_a", state, facing, false, false);
                }
                if (instance == null) throw new IllegalStateException("vanilla block entity provider returned null: " + value);
                return ReflectionAccess.MAPPING.invoke(instance, ReflectionAccess.type("net.minecraft.world.level.block.entity.BlockEntity"), "getType");
            });
            return actual == type;
        }
        try {
            java.lang.reflect.Method method = ReflectionAccess.method(blockEntityType, "isValid", blockState);
            return (Boolean) method.invoke(type, state);
        } catch (IllegalStateException missingStateSignature) {
            return (Boolean) ReflectionAccess.invoke(type, blockEntityType, "isValid", new Class<?>[]{block}, blockValue);
        } catch (ReflectiveOperationException failure) { throw new IllegalStateException("block entity validity probe failed", failure); }
    }

    Object registry(String field) {
        if (ReflectionAccess.MAPPING.hasField(builtInRegistries, field)) return ReflectionAccess.staticField(builtInRegistries, field);
        if (Boolean.getBoolean("nova.oracle.flattened")) {
            Class<?> owner = switch(field) {
                case "BLOCK" -> block;
                case "FLUID" -> fluid;
                case "BLOCK_ENTITY_TYPE" -> blockEntityType;
                default -> throw new IllegalStateException("target has no required registry " + field);
            };
            return ReflectionAccess.staticField(owner, "REGISTRY");
        }
        throw new IllegalStateException("target has no required registry " + field);
    }

    @SuppressWarnings("unchecked")
    List<RegistryEntry> registryEntries(Object registry) {
        Set<Object> keys =
                (Set<Object>) ReflectionAccess.invokeCompatible(registry, registry.getClass(), new String[]{"keySet", "func_148742_b"});
        List<RegistryEntry> entries = new ArrayList<>(keys.size());
        for (Object key : keys) {
            Object value =
                    ReflectionAccess.invokeCompatible(registry, registry.getClass(),
                            Boolean.getBoolean("nova.oracle.flattened") ? new String[]{"func_212608_b", "func_82594_a"} : new String[]{"getValue", "get"}, key);
            if (value instanceof java.util.Optional<?> optional) value = optional.orElseThrow(() -> new IllegalStateException("registry returned empty Optional for " + key));
            if (value == null) {
                throw new IllegalStateException("registry returned null for " + key);
            }
            int rawId =
                    (Integer)
                            ReflectionAccess.invokeCompatible(registry, registry.getClass(), new String[]{"getId", "func_148757_b"}, value);
            entries.add(new RegistryEntry(key.toString(), value, rawId));
        }
        entries.sort(java.util.Comparator.comparing(RegistryEntry::id));
        return entries;
    }

    @SuppressWarnings("unchecked")
    List<Object> possibleStates(Object definition) {
        return (List<Object>)
                ReflectionAccess.invoke(
                        definition, stateDefinition, "getPossibleStates", NO_TYPES);
    }

    Map<String, String> stateProperties(Object state) {
        TreeMap<String, String> properties = new TreeMap<>();
        Object rawValues = ReflectionAccess.invoke(state, state.getClass(), "getValues", NO_TYPES);
        if (rawValues instanceof Map<?, ?> values) {
            Class<?> property = ReflectionAccess.type("net.minecraft.world.level.block.state.properties.Property");
            for (var entry : values.entrySet()) {
                properties.put((String) ReflectionAccess.invoke(entry.getKey(), property, "getName"),
                        (String) ReflectionAccess.invoke(entry.getKey(), property, "getName", new Class<?>[]{Comparable.class}, entry.getValue()));
            }
            return properties;
        }
        @SuppressWarnings("unchecked") Stream<Object> values = (Stream<Object>) rawValues;
        try (values) {
            values.forEach(
                    value -> {
                        String encoded = value.toString();
                        int separator = encoded.indexOf('=');
                        if (separator <= 0 || separator == encoded.length() - 1) {
                            throw new IllegalStateException("invalid state property " + encoded);
                        }
                        properties.put(
                                encoded.substring(0, separator), encoded.substring(separator + 1));
                    });
        }
        return properties;
    }

    private Artifact write(
            Path root,
            String name,
            List<? extends Map<String, ?>> records,
            List<String> keyFields)
            throws IOException {
        String relative = name + ".jsonl";
        CanonicalJsonl.Result result =
                CanonicalJsonl.write(root.resolve(relative), records, keyFields);
        return new Artifact(
                name, relative, keyFields, result.recordCount(), result.recordsSha256());
    }

    private void writeManifest(
            Path root,
            List<Artifact> artifacts,
            RuntimeIdentity identity,
            Map<String, Object> blockStateSemantics)
            throws IOException {
        List<Map<String, Object>> artifactRecords =
                artifacts.stream().map(Artifact::asRecord).toList();
        Map<String, Object> manifest = new LinkedHashMap<>();
        manifest.put("artifacts", artifactRecords);
        manifest.put("block_state_semantics", blockStateSemantics);
        manifest.put("client_jar_sha256", identity.clientJarSha256());
        manifest.put("client_jar_size", identity.clientJarSize());
        manifest.put("java_major", Runtime.version().feature());
        if (Boolean.getBoolean("nova.oracle.flattened")) {
            manifest.put("world_version_source", "exact_project_version_registry:" + identity.minecraft());
            manifest.put("world_version_registry_sha256", System.getProperty("nova.oracle.registrySha256"));
        }
        manifest.put("minecraft", identity.minecraft());
        manifest.put(
                "pending_sets",
                List.of("client_reload"));
        manifest.put("protocol_version", identity.protocolVersion());
        manifest.put("ready_headless_sets", artifacts.size());
        manifest.put("schema", "lba.oracle.mc26.runtime");
        manifest.put("schema_version", MANIFEST_SCHEMA_VERSION);
        manifest.put("world_version", identity.worldVersion());
        writeTextAtomically(
                root.resolve("oracle-manifest.json"),
                CanonicalJsonl.json(manifest) + "\n");
    }

    private static void writeTextAtomically(Path output, String text) throws IOException {
        Path temporary =
                Files.createTempFile(output.getParent(), output.getFileName().toString(), ".tmp");
        boolean moved = false;
        try {
            Files.writeString(temporary, text, StandardCharsets.UTF_8);
            Files.move(temporary, output, StandardCopyOption.REPLACE_EXISTING);
            moved = true;
        } finally {
            if (!moved) {
                Files.deleteIfExists(temporary);
            }
        }
    }

    record ExportResult(
            List<Artifact> artifacts,
            long skipEvaluatedComparisons,
            long skipElapsedMillis,
            long semanticsElapsedMillis) {}

    private record RuntimeIdentity(
            String minecraft,
            int worldVersion,
            int protocolVersion,
            long clientJarSize,
            String clientJarSha256) {}

    record Artifact(
            String name,
            String path,
            List<String> keyFields,
            int recordCount,
            String recordsSha256) {
        Map<String, Object> asRecord() {
            return Map.of(
                    "key_fields", keyFields,
                    "name", name,
                    "path", path,
                    "record_count", recordCount,
                    "records_sha256", recordsSha256);
        }
    }

    record RegistryEntry(String id, Object value, int rawId) {}
}
