package net.litematicaba.mc26oracle;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.Executor;
import java.util.stream.Stream;

final class MinecraftServerDataOracle {
    private static final Class<?>[] NO_TYPES = new Class<?>[0];

    private final Class<?> serverPacksSource =
            ReflectionAccess.type("net.minecraft.server.packs.repository.ServerPacksSource");
    private final Class<?> packResources =
            ReflectionAccess.type("net.minecraft.server.packs.PackResources");
    private final Class<?> packType = ReflectionAccess.type("net.minecraft.server.packs.PackType");
    private final Class<?> multiPackResourceManager =
            ReflectionAccess.type(
                    "net.minecraft.server.packs.resources.MultiPackResourceManager");
    private final Class<?> resourceManager =
            ReflectionAccess.type("net.minecraft.server.packs.resources.ResourceManager");
    private final Class<?> resource =
            ReflectionAccess.type("net.minecraft.server.packs.resources.Resource");
    private final Class<?> registryLayer = ReflectionAccess.type("net.minecraft.server.RegistryLayer");
    private final Class<?> layeredRegistryAccess =
            ReflectionAccess.type("net.minecraft.core.LayeredRegistryAccess");
    private final Class<?> registryAccess = ReflectionAccess.type("net.minecraft.core.RegistryAccess");
    private final Class<?> frozenRegistryAccess =
            ReflectionAccess.type("net.minecraft.core.RegistryAccess$Frozen");
    private final Class<?> registryAccessEntry =
            ReflectionAccess.type("net.minecraft.core.RegistryAccess$RegistryEntry");
    private final Class<?> registry = ReflectionAccess.type("net.minecraft.core.Registry");
    private final Class<?> pendingTags =
            ReflectionAccess.type("net.minecraft.core.Registry$PendingTags");
    private final Class<?> tagLoader = ReflectionAccess.type("net.minecraft.tags.TagLoader");
    private final Class<?> registryDataLoader =
            ReflectionAccess.type("net.minecraft.resources.RegistryDataLoader");
    private final Class<?> resourceKey = ReflectionAccess.type("net.minecraft.resources.ResourceKey");
    private final Class<?> identifier = ReflectionAccess.type("net.minecraft.resources.Identifier");
    private final Class<?> fileToIdConverter =
            ReflectionAccess.type("net.minecraft.resources.FileToIdConverter");
    private final Class<?> registries =
            ReflectionAccess.type("net.minecraft.core.registries.Registries");
    private final Class<?> holderSet = ReflectionAccess.type("net.minecraft.core.HolderSet");
    private final Class<?> namedHolderSet =
            ReflectionAccess.type("net.minecraft.core.HolderSet$Named");
    private final Class<?> holder = ReflectionAccess.type("net.minecraft.core.Holder");
    private final Class<?> tagKey = ReflectionAccess.type("net.minecraft.tags.TagKey");

    private static Object compatibleField(Class<?> owner, String modern, String previous) {
        try { owner.getDeclaredField(modern); return ReflectionAccess.staticField(owner, modern); }
        catch (NoSuchFieldException missing) { return ReflectionAccess.staticField(owner, previous); }
    }

    ExportData load() {
        Object vanilla =
                ReflectionAccess.invokeStatic(
                        serverPacksSource, "createVanillaPackSource", NO_TYPES);
        // 26.3 VanillaPackResources is a holder, not itself PackResources.
        if (!packResources.isInstance(vanilla)) {
            vanilla = ReflectionAccess.invoke(vanilla,
                    ReflectionAccess.type("net.minecraft.server.packs.VanillaPackResources"),
                    "fullResources", NO_TYPES);
        }
        Object resources;
        try {
            resources =
                    ReflectionAccess.construct(
                            multiPackResourceManager,
                            new Class<?>[] {packType, List.class},
                            ReflectionAccess.staticField(packType, "SERVER_DATA"),
                            List.of(vanilla));
        } catch (RuntimeException | Error failure) {
            try {
                ReflectionAccess.invoke(vanilla, packResources, "close", NO_TYPES);
            } catch (RuntimeException | Error cleanupFailure) {
                failure.addSuppressed(cleanupFailure);
            }
            throw failure;
        }

        Throwable primaryFailure = null;
        try {
            Object initialLayers =
                    ReflectionAccess.invokeStatic(
                            registryLayer, "createRegistryAccess", NO_TYPES);
            Object staticLayer = ReflectionAccess.staticField(registryLayer, "STATIC");
            Object worldgenLayer = compatibleField(registryLayer, "WORLD", "WORLDGEN");
            Object staticAccess =
                    ReflectionAccess.invoke(
                            initialLayers,
                            layeredRegistryAccess,
                            "getLayer",
                            new Class<?>[] {Object.class},
                            staticLayer);

            @SuppressWarnings("unchecked")
            List<Object> staticTags =
                    (List<Object>)
                            ReflectionAccess.invokeStatic(
                                    tagLoader,
                                    "loadTagsForExistingRegistries",
                                    new Class<?>[] {resourceManager, registryAccess},
                                    resources,
                                    staticAccess);
            Object worldgenLoadContext =
                    ReflectionAccess.invoke(
                            initialLayers,
                            layeredRegistryAccess,
                            "getAccessForLoading",
                            new Class<?>[] {Object.class},
                            worldgenLayer);
            @SuppressWarnings("unchecked")
            List<Object> worldgenContext =
                    (List<Object>)
                            ReflectionAccess.invokeStatic(
                                    tagLoader,
                                    "buildUpdatedLookups",
                                    new Class<?>[] {frozenRegistryAccess, List.class},
                                    worldgenLoadContext,
                                    staticTags);

            Executor directExecutor = Runnable::run;
            Object loadFuture =
                    ReflectionAccess.invokeStatic(
                            registryDataLoader,
                            "load",
                            new Class<?>[] {
                                resourceManager, List.class, List.class, Executor.class
                            },
                            resources,
                            worldgenContext,
                            compatibleField(
                                    registryDataLoader, "WORLD_REGISTRIES", "WORLDGEN_REGISTRIES"),
                            directExecutor);
            Object worldgenAccess = ((CompletableFuture<?>) loadFuture).join();
            staticTags.forEach(
                    pending -> ReflectionAccess.invoke(pending, pendingTags, "apply", NO_TYPES));

            List<Map<String, Object>> dataRecords =
                    exportDataRegistries(worldgenAccess, resources);
            List<Map<String, Object>> tagRecords =
                    exportTags(List.of(staticAccess, worldgenAccess), resources);
            return new ExportData(dataRecords, tagRecords);
        } catch (RuntimeException | Error failure) {
            primaryFailure = failure;
            throw failure;
        } finally {
            try {
                ReflectionAccess.invoke(resources, multiPackResourceManager, "close", NO_TYPES);
            } catch (RuntimeException | Error cleanupFailure) {
                if (primaryFailure != null) {
                    primaryFailure.addSuppressed(cleanupFailure);
                } else {
                    throw cleanupFailure;
                }
            }
        }
    }

    private List<Map<String, Object>> exportDataRegistries(
            Object access, Object resources) {
        List<Map<String, Object>> records = new ArrayList<>();
        for (AccessEntry accessEntry : registryEntries(access)) {
            Map<String, String> sources =
                    winningResourceSources(accessEntry.key(), resources);
            for (ValueEntry valueEntry : valueEntries(accessEntry.value())) {
                String sourcePack = sources.get(valueEntry.id());
                if (sourcePack == null) {
                    throw new IllegalStateException(
                            "missing winning resource for "
                                    + accessEntry.id()
                                    + "/"
                                    + valueEntry.id());
                }
                Map<String, Object> record = new LinkedHashMap<>();
                record.put("entry_id", valueEntry.id());
                record.put("raw_id", valueEntry.rawId());
                record.put("registry_id", accessEntry.id());
                record.put("source_pack_id", sourcePack);
                records.add(record);
            }
        }
        return List.copyOf(records);
    }

    private List<Map<String, Object>> exportTags(
            List<Object> accesses, Object resources) {
        List<Map<String, Object>> records = new ArrayList<>();
        for (Object access : accesses) {
            for (AccessEntry accessEntry : registryEntries(access)) {
                Map<String, List<String>> sourceStacks =
                        tagDefinitionSourceStacks(accessEntry.key(), resources);
                @SuppressWarnings("unchecked")
                Stream<Object> tags =
                        (Stream<Object>)
                                ReflectionAccess.invoke(
                                        accessEntry.value(), registry, "getTags", NO_TYPES);
                try (tags) {
                    tags.forEach(
                            named -> {
                                Object key =
                                        ReflectionAccess.invoke(
                                                named, namedHolderSet, "key", NO_TYPES);
                                String tagId =
                                        String.valueOf(
                                                ReflectionAccess.invoke(
                                                        key, tagKey, "location", NO_TYPES));
                                int size =
                                        (Integer)
                                                ReflectionAccess.invoke(
                                                        named, holderSet, "size", NO_TYPES);
                                List<String> memberIds = new ArrayList<>(size);
                                for (int index = 0; index < size; index++) {
                                    Object member =
                                            ReflectionAccess.invoke(
                                                    named,
                                                    holderSet,
                                                    "get",
                                                    new Class<?>[] {int.class},
                                                    index);
                                    @SuppressWarnings("unchecked")
                                    Optional<Object> memberKey =
                                            (Optional<Object>)
                                                    ReflectionAccess.invoke(
                                                            member,
                                                            holder,
                                                            "unwrapKey",
                                                            NO_TYPES);
                                    Object registeredKey =
                                            memberKey.orElseThrow(
                                                    () ->
                                                            new IllegalStateException(
                                                                    "unregistered tag member in "
                                                                            + accessEntry.id()
                                                                            + "#"
                                                                            + tagId));
                                    memberIds.add(
                                            String.valueOf(
                                                    ReflectionAccess.invoke(
                                                            registeredKey,
                                                            resourceKey,
                                                            "identifier",
                                                            NO_TYPES)));
                                }
                                List<String> definitionSources = sourceStacks.get(tagId);
                                if (definitionSources == null || definitionSources.isEmpty()) {
                                    throw new IllegalStateException(
                                            "missing tag definition sources for "
                                                    + accessEntry.id()
                                                    + "#"
                                                    + tagId);
                                }
                                records.addAll(
                                        RegistryOracleRecords.tagMembers(
                                                accessEntry.id(),
                                                tagId,
                                                memberIds,
                                                definitionSources));
                            });
                }
            }
        }
        return List.copyOf(records);
    }

    private Map<String, String> winningResourceSources(
            Object registryKey, Object resources) {
        Object converter =
                ReflectionAccess.invokeStatic(
                        fileToIdConverter,
                        "registry",
                        new Class<?>[] {resourceKey},
                        registryKey);
        @SuppressWarnings("unchecked")
        Map<Object, Object> matching =
                (Map<Object, Object>)
                        ReflectionAccess.invoke(
                                converter,
                                fileToIdConverter,
                                "listMatchingResources",
                                new Class<?>[] {resourceManager},
                                resources);
        Map<String, String> result = new LinkedHashMap<>();
        matching.forEach(
                (resourceId, selectedResource) -> {
                    String entryId =
                            String.valueOf(
                                    ReflectionAccess.invoke(
                                            converter,
                                            fileToIdConverter,
                                            "fileToId",
                                            new Class<?>[] {identifier},
                                            resourceId));
                    String source =
                            (String)
                                    ReflectionAccess.invoke(
                                            selectedResource,
                                            resource,
                                            "sourcePackId",
                                            NO_TYPES);
                    if (result.put(entryId, source) != null) {
                        throw new IllegalStateException(
                                "duplicate winning resource " + entryId);
                    }
                });
        return Map.copyOf(result);
    }

    private Map<String, List<String>> tagDefinitionSourceStacks(
            Object registryKey, Object resources) {
        String directory =
                (String)
                        ReflectionAccess.invokeStatic(
                                registries,
                                "tagsDirPath",
                                new Class<?>[] {resourceKey},
                                registryKey);
        Object converter =
                ReflectionAccess.invokeStatic(
                        fileToIdConverter,
                        "json",
                        new Class<?>[] {String.class},
                        directory);
        @SuppressWarnings("unchecked")
        Map<Object, List<Object>> stacks =
                (Map<Object, List<Object>>)
                        ReflectionAccess.invoke(
                                converter,
                                fileToIdConverter,
                                "listMatchingResourceStacks",
                                new Class<?>[] {resourceManager},
                                resources);
        Map<String, List<String>> result = new LinkedHashMap<>();
        stacks.forEach(
                (resourceId, stack) -> {
                    String tagId =
                            String.valueOf(
                                    ReflectionAccess.invoke(
                                            converter,
                                            fileToIdConverter,
                                            "fileToId",
                                            new Class<?>[] {identifier},
                                            resourceId));
                    List<String> sources =
                            stack.stream()
                                    .map(
                                            entry ->
                                                    (String)
                                                            ReflectionAccess.invoke(
                                                                    entry,
                                                                    resource,
                                                                    "sourcePackId",
                                                                    NO_TYPES))
                                    .toList();
                    result.put(tagId, sources);
                });
        return Map.copyOf(result);
    }

    private List<AccessEntry> registryEntries(Object access) {
        @SuppressWarnings("unchecked")
        Stream<Object> entries =
                (Stream<Object>)
                        ReflectionAccess.invoke(
                                access, registryAccess, "registries", NO_TYPES);
        try (entries) {
            return entries
                    .map(
                            entry -> {
                                Object key =
                                        ReflectionAccess.invoke(
                                                entry,
                                                registryAccessEntry,
                                                "key",
                                                NO_TYPES);
                                String id =
                                        String.valueOf(
                                                ReflectionAccess.invoke(
                                                        key,
                                                        resourceKey,
                                                        "identifier",
                                                        NO_TYPES));
                                Object value =
                                        ReflectionAccess.invoke(
                                                entry,
                                                registryAccessEntry,
                                                "value",
                                                NO_TYPES);
                                return new AccessEntry(id, key, value);
                            })
                    .sorted(java.util.Comparator.comparing(AccessEntry::id))
                    .toList();
        }
    }

    private List<ValueEntry> valueEntries(Object registryValue) {
        @SuppressWarnings("unchecked")
        Set<Object> keys =
                (Set<Object>)
                        ReflectionAccess.invoke(
                                registryValue, registry, "keySet", NO_TYPES);
        List<ValueEntry> entries = new ArrayList<>(keys.size());
        for (Object key : keys) {
            Object value =
                    ReflectionAccess.invoke(
                            registryValue,
                            registry,
                            "getValue",
                            new Class<?>[] {identifier},
                            key);
            if (value == null) {
                throw new IllegalStateException("registry returned null for " + key);
            }
            int rawId =
                    (Integer)
                            ReflectionAccess.invoke(
                                    registryValue,
                                    registry,
                                    "getId",
                                    new Class<?>[] {Object.class},
                                    value);
            entries.add(new ValueEntry(key.toString(), rawId));
        }
        entries.sort(java.util.Comparator.comparing(ValueEntry::id));
        return List.copyOf(entries);
    }

    record ExportData(
            List<Map<String, Object>> dataRegistries,
            List<Map<String, Object>> tags) {}

    private record AccessEntry(String id, Object key, Object value) {}

    private record ValueEntry(String id, int rawId) {}
}
