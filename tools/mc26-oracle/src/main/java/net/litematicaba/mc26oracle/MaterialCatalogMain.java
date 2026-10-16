package net.litematicaba.mc26oracle;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

/** Exports inventory semantics from the pinned vanilla registries; never starts a renderer. */
public final class MaterialCatalogMain {
    private static final Class<?>[] NONE = new Class<?>[0];

    public static void main(String[] args) throws Exception {
        if (args.length != 2 || !args[0].equals("--output") || Runtime.version().feature() != 25) {
            throw new IllegalArgumentException("Java 25: MaterialCatalogMain --output <json>");
        }
        Class<?> shared = ReflectionAccess.type("net.minecraft.SharedConstants");
        ReflectionAccess.invokeStatic(shared, "tryDetectVersion", NONE);
        ReflectionAccess.invokeStatic(ReflectionAccess.type("net.minecraft.server.Bootstrap"), "bootStrap", NONE);
        Class<?> registries = ReflectionAccess.type("net.minecraft.core.registries.BuiltInRegistries");
        // 26.3 binds item defaults only after world/reloadable registries exist.
        // This is the same initializer used by RegistryComponentsReport and server reloads.
        Class<?> vanilla = ReflectionAccess.type("net.minecraft.data.registries.VanillaRegistries");
        Class<?> provider = ReflectionAccess.type("net.minecraft.core.HolderLookup$Provider");
        Object world = ReflectionAccess.invokeStatic(vanilla, "createWorldLookup", NONE);
        Object context = ReflectionAccess.invokeStatic(vanilla, "createReloadableLookup", new Class<?>[]{provider}, world);
        Class<?> initializers = ReflectionAccess.type("net.minecraft.core.component.DataComponentInitializers");
        Object defaults = ReflectionAccess.staticField(registries, "DATA_COMPONENT_INITIALIZERS");
        Class<?> pending = ReflectionAccess.type("net.minecraft.core.component.DataComponentInitializers$PendingComponents");
        for (Object entry : (List<?>) ReflectionAccess.invoke(defaults, initializers, "build", new Class<?>[]{provider}, context)) {
            ReflectionAccess.invoke(entry, pending, "apply");
        }
        Class<?> registry = ReflectionAccess.type("net.minecraft.core.Registry");
        Class<?> itemClass = ReflectionAccess.type("net.minecraft.world.item.Item");
        Class<?> blockClass = ReflectionAccess.type("net.minecraft.world.level.block.Block");
        Class<?> componentType = ReflectionAccess.type("net.minecraft.core.component.DataComponentType");
        Class<?> componentMap = ReflectionAccess.type("net.minecraft.core.component.DataComponentGetter");
        Object itemModel = ReflectionAccess.staticField(ReflectionAccess.type("net.minecraft.core.component.DataComponents"), "ITEM_MODEL");
        Object itemRegistry = ReflectionAccess.staticField(registries, "ITEM");
        Object blockRegistry = ReflectionAccess.staticField(registries, "BLOCK");
        Map<String, Object> items = new TreeMap<>();
        for (Object item : (Iterable<?>) itemRegistry) {
            String id = key(registry, itemRegistry, item);
            Object components = ReflectionAccess.invoke(item, itemClass, "components");
            Object model = ReflectionAccess.invoke(components, componentMap, "get", new Class<?>[]{componentType}, itemModel);
            items.put(id, Map.of(
                    "stack_size", ReflectionAccess.invoke(item, itemClass, "getDefaultMaxStackSize"),
                    "translation_key", ReflectionAccess.invoke(item, itemClass, "getDescriptionId"),
                    "item_model", model == null ? "" : model.toString(),
                    "can_fit_container", item.getClass().getMethod("canFitInsideContainerItems").invoke(item)));
        }
        Map<String, Object> blocks = new TreeMap<>();
        Class<?> pot = ReflectionAccess.type("net.minecraft.world.level.block.FlowerPotBlock");
        for (Object block : (Iterable<?>) blockRegistry) {
            Object item = ReflectionAccess.invoke(block, blockClass, "asItem");
            Map<String, Object> row = new TreeMap<>();
            row.put("item_id", key(registry, itemRegistry, item));
            List<String> classes = new ArrayList<>();
            for (Class<?> type = block.getClass(); type != null && type != blockClass; type = type.getSuperclass()) {
                classes.add(type.getSimpleName());
            }
            row.put("classes", classes);
            // Both vanilla head/body clone methods return GLOW_BERRIES. This
            // creative identity must not change AIR in the body material row.
            if (classes.contains("CaveVinesBlock") || classes.contains("CaveVinesPlantBlock")) {
                Object berries = ReflectionAccess.staticField(ReflectionAccess.type("net.minecraft.world.item.Items"), "GLOW_BERRIES");
                row.put("creative_item_id", key(registry, itemRegistry, berries));
            }
            if (pot.isInstance(block)) {
                Object plant = ReflectionAccess.invoke(block, pot, "getPotted");
                row.put("potted_item", key(registry, itemRegistry, ReflectionAccess.invoke(plant, blockClass, "asItem")));
            }
            blocks.put(key(registry, blockRegistry, block), row);
        }
        Map<String, Object> output = Map.of(
                "schema", 1, "minecraft_version", "26.3",
                "client_sha256", "4508d006323f24fa02876310c192d739af56516eb259000ac50f0909a68c9a2d",
                "language_sha1", "f87510f4509890eaf176e0de1430f6bb326a6800",
                "blocks", blocks, "items", items);
        Files.writeString(Path.of(args[1]), CanonicalJsonl.json(output) + "\n");
        System.out.println("MATERIAL_CATALOG blocks=" + blocks.size() + " items=" + items.size());
    }

    private static String key(Class<?> registry, Object instance, Object value) {
        return ReflectionAccess.invoke(instance, registry, "getKey", new Class<?>[]{Object.class}, value).toString();
    }
}
