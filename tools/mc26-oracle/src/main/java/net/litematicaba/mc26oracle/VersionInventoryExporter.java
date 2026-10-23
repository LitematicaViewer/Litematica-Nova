package net.litematicaba.mc26oracle;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.TreeMap;

/** Extracts one real vanilla client at a time. Does not launch Minecraft's game,
 * window, render system or Java main. No data table is borrowed from another version.
 * Compile with --release 8; the caller supplies that client's JVM and dependencies. */
public final class VersionInventoryExporter {
    private final MappingAccess access;
    private final String version;
    private final Map<String, String> adapters = new TreeMap<String, String>();
    private Class<?> blockClass;
    private Class<?> itemClass;
    private Object blockRegistry;
    private Object itemRegistry;
    private Object containerSlot;

    private VersionInventoryExporter(MappingAccess access, String version) {
        this.access = access;
        this.version = version;
    }

    public static void main(String[] args) throws Exception {
        Map<String, String> options = parse(args);
        String version = required(options, "version");
        String clientSha = required(options, "client-sha256").toLowerCase(Locale.ROOT);
        if (!clientSha.matches("[0-9a-f]{64}")) throw new IllegalArgumentException("client-sha256 must identify the exact original client JAR");
        String languageSha = options.containsKey("language-sha1") ? options.get("language-sha1") : "";
        if (!languageSha.isEmpty() && !languageSha.matches("[0-9a-fA-F]{40}")) throw new IllegalArgumentException("invalid language-sha1");
        if (options.containsKey("client-jar") && !clientSha.equals(sha256(Paths.get(options.get("client-jar"))))) {
            throw new IllegalArgumentException("client-sha256 does not match --client-jar");
        }
        MappingAccess access = options.containsKey("mcp-srg")
                ? MappingAccess.fromMcp(Paths.get(options.get("mcp-srg")), Paths.get(required(options, "mcp-csv")))
                : new MappingAccess(options.containsKey("mappings") ? Paths.get(options.get("mappings")) : null);
        // Check the classpath actually executes the declared client, not merely
        // a correctly hashed file passed beside another version's runtime.
        Class<?> sourceClass = access.uninitializedType(options.containsKey("mcp-srg")
                ? "net.minecraft.block.Block" : "net.minecraft.SharedConstants");
        URL source = sourceClass.getProtectionDomain().getCodeSource().getLocation();
        if (!source.getProtocol().equals("file") || !clientSha.equals(sha256(Paths.get(source.toURI())))) {
            throw new IllegalArgumentException("loaded target runtime JAR differs from client-sha256");
        }
        Map<String, Object> result;
        if (options.containsKey("mcp-srg")) {
            required(options, "legacy-registry");
            result = VersionInventoryLegacyExporter.export(access, options);
        } else {
            VersionInventoryExporter exporter = new VersionInventoryExporter(access, version);
            exporter.verifyRuntimeVersion();
            exporter.bootstrap();
            result = exporter.export(clientSha, languageSha);
        }
        Map<String, Object> provenance = new TreeMap<String, Object>();
        provenance.put("method", "execute target client's actual vanilla registry and implementation");
        for (String input : new String[]{"mappings", "mcp-srg", "mcp-csv"}) {
            if (!options.containsKey(input)) continue;
            Path path = Paths.get(options.get(input));
            provenance.put(input + "_file", path.getFileName().toString());
            provenance.put(input + "_sha256", sha256(path));
        }
        if (version.equals("1.10.1") && options.containsKey("mcp-srg")) {
            provenance.put("mapping_reuse", "1.10 inventory mapping: 510 relevant class files identical in actual 1.10.1; Farmland's only removed collision overload is not used by inventory extraction");
        }
        result.put("source_provenance", provenance);
        Path output = Paths.get(required(options, "output")).toAbsolutePath();
        Files.createDirectories(output.getParent());
        Path temporary = Files.createTempFile(output.getParent(), output.getFileName().toString(), ".pending");
        try {
            Files.write(temporary, (json(result) + "\n").getBytes(StandardCharsets.UTF_8));
            Files.move(temporary, output, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
        } finally { Files.deleteIfExists(temporary); }
        System.out.println("MATERIAL_CATALOG version=" + version + " blocks="
                + ((Map<?, ?>) result.get("blocks")).size() + " items=" + ((Map<?, ?>) result.get("items")).size());
    }

    private static Map<String, String> parse(String[] args) {
        Map<String, String> result = new LinkedHashMap<String, String>();
        for (int i = 0; i < args.length; i += 2) {
            if (!args[i].startsWith("--") || i + 1 >= args.length) throw new IllegalArgumentException(usage());
            String key = args[i].substring(2);
            if (!(key.equals("version") || key.equals("client-sha256") || key.equals("client-jar")
                    || key.equals("language-sha1") || key.equals("mappings") || key.equals("output")
                    || key.equals("mcp-srg") || key.equals("mcp-csv") || key.equals("legacy-registry"))) {
                throw new IllegalArgumentException("unknown option " + args[i] + "; " + usage());
            }
            if (result.put(key, args[i + 1]) != null) throw new IllegalArgumentException("duplicate --" + key);
        }
        return result;
    }

    private static String usage() {
        return "VersionInventoryExporter --version <target> --client-sha256 <sha256> --output <json> "
                + "[--client-jar <original.jar>] [--mappings <official-client-mappings.txt>] [--language-sha1 <sha1>]";
    }

    private static String required(Map<String, String> options, String key) {
        String value = options.get(key);
        if (value == null || value.length() == 0) throw new IllegalArgumentException("missing --" + key + "; " + usage());
        return value;
    }

    @SuppressWarnings("unchecked")
    private void verifyRuntimeVersion() throws Exception {
        Class<?> shared = access.type("net.minecraft.SharedConstants");
        URL resource = shared.getResource("/version.json");
        if (resource == null) throw new IllegalStateException("target runtime has no version.json; an explicit legacy adapter is required");
        byte[] bytes;
        try (InputStream input = resource.openStream()) { bytes = readAll(input); }
        Class<?> gson = Class.forName("com.google.gson.Gson");
        Object parser = gson.getConstructor().newInstance();
        Map<String, Object> metadata = (Map<String, Object>) gson.getMethod("fromJson", String.class, Class.class)
                .invoke(parser, new String(bytes, StandardCharsets.UTF_8), Map.class);
        if (!version.equals(metadata.get("id"))) throw new IllegalStateException("runtime version " + metadata.get("id") + " differs from requested " + version);
        adapters.put("runtime_version_resource", "target JAR /version.json id=" + version);
    }

    private void bootstrap() {
        Class<?> shared = access.type("net.minecraft.SharedConstants");
        if (access.hasMethod(shared, "tryDetectVersion", 0)) access.invokeStatic(shared, "tryDetectVersion");
        Class<?> bootstrap = access.type("net.minecraft.server.Bootstrap");
        access.invokeStatic(bootstrap, "bootStrap");
        Class<?> registries = access.typeOrNull("net.minecraft.core.registries.BuiltInRegistries");
        if (registries == null) registries = access.type("net.minecraft.core.Registry");
        itemRegistry = access.staticField(registries, "ITEM");
        blockRegistry = access.staticField(registries, "BLOCK");
        itemClass = access.type("net.minecraft.world.item.Item");
        blockClass = access.type("net.minecraft.world.level.block.Block");
        // 26.x moved default components into deferred registry initializers. Read
        // their actual implementation, not 26.3 item defaults copied to old clients.
        Class<?> initializers = access.typeOrNull("net.minecraft.core.component.DataComponentInitializers");
        if (initializers != null) {
            Class<?> vanilla = access.type("net.minecraft.data.registries.VanillaRegistries");
            Object context;
            if (access.hasMethod(vanilla, "createWorldLookup", 0)) {
                Object world = access.invokeStatic(vanilla, "createWorldLookup");
                context = access.invokeStatic(vanilla, "createReloadableLookup", world);
            } else {
                // 26.1/26.2 RegistryComponentsReport passes createLookup directly.
                context = access.invokeStatic(vanilla, "createLookup");
            }
            Object defaults = access.staticField(registries, "DATA_COMPONENT_INITIALIZERS");
            Object entries = access.invoke(defaults, initializers, "build", context);
            Class<?> pending = access.type("net.minecraft.core.component.DataComponentInitializers$PendingComponents");
            for (Object entry : (Iterable<?>) entries) access.invoke(entry, pending, "apply");
            adapters.put("item_components", "DataComponentInitializers.build/apply");
        } else {
            adapters.put("item_components", "target Item constructor/default stack");
        }
        adapters.put("registries", access.canonicalName(registries));
    }

    private Map<String, Object> export(String clientSha, String languageSha) {
        Map<String, Object> items = new TreeMap<String, Object>();
        if (!(itemRegistry instanceof Iterable<?>) || !(blockRegistry instanceof Iterable<?>)) {
            throw new IllegalStateException("target registries are not iterable; explicit legacy registry adapter required");
        }
        for (Object item : (Iterable<?>) itemRegistry) {
            String id = key(itemRegistry, item);
            Map<String, Object> row = new TreeMap<String, Object>();
            row.put("stack_size", stackSize(item));
            row.put("translation_key", access.invoke(item, itemClass, "getDescriptionId"));
            row.put("item_model", itemModel(item));
            row.put("can_fit_container", containerFit(item));
            items.put(id, row);
        }
        Class<?> potClass = access.typeOrNull("net.minecraft.world.level.block.FlowerPotBlock");
        Class<?> vines = access.typeOrNull("net.minecraft.world.level.block.CaveVinesBlock");
        Class<?> vinesPlant = access.typeOrNull("net.minecraft.world.level.block.CaveVinesPlantBlock");
        Map<String, Object> blocks = new TreeMap<String, Object>();
        for (Object block : (Iterable<?>) blockRegistry) {
            Object item = access.invoke(block, blockClass, "asItem");
            Map<String, Object> row = new TreeMap<String, Object>();
            row.put("item_id", key(itemRegistry, item));
            List<String> classes = new ArrayList<String>();
            for (Class<?> type = block.getClass(); type != null && type != blockClass; type = type.getSuperclass()) {
                String name = access.simpleName(type);
                classes.add(name);
                // BedBlock is the actual pre-26.x placement class. The runtime
                // inventory consumer uses AbstractBedBlock as its stable role.
                if (access.canonicalName(type).equals("net.minecraft.world.level.block.BedBlock")) classes.add("AbstractBedBlock");
            }
            row.put("classes", classes);
            if ((vines != null && vines.isInstance(block)) || (vinesPlant != null && vinesPlant.isInstance(block))) {
                Object berries = access.staticField(access.type("net.minecraft.world.item.Items"), "GLOW_BERRIES");
                row.put("creative_item_id", key(itemRegistry, berries));
            }
            if (potClass != null && potClass.isInstance(block)) {
                String accessor = access.hasMethod(potClass, "getPotted", 0) ? "getPotted" : "getContent";
                Object plant = access.invoke(block, potClass, accessor);
                adapters.put("potted_item", "FlowerPotBlock." + accessor + "/Block.asItem");
                row.put("potted_item", key(itemRegistry, access.invoke(plant, blockClass, "asItem")));
            }
            blocks.put(key(blockRegistry, block), row);
        }
        if (blocks.size() == 0 || items.size() == 0 || !blocks.containsKey("minecraft:air") || !items.containsKey("minecraft:air")) {
            throw new IllegalStateException("incomplete target vanilla registries");
        }
        Map<String, Object> output = new TreeMap<String, Object>();
        output.put("schema", 1);
        output.put("minecraft_version", version);
        output.put("client_sha256", clientSha);
        output.put("language_sha1", languageSha.toLowerCase(Locale.ROOT));
        output.put("adapters", adapters);
        output.put("blocks", blocks);
        output.put("items", items);
        return output;
    }

    private int stackSize(Object item) {
        Object size;
        if (access.hasMethod(itemClass, "getDefaultMaxStackSize", 0)) {
            adapters.put("stack_size", "Item.getDefaultMaxStackSize");
            size = access.invoke(item, itemClass, "getDefaultMaxStackSize");
        } else if (access.hasMethod(itemClass, "getMaxStackSize", 0)) {
            adapters.put("stack_size", "Item.getMaxStackSize");
            size = access.invoke(item, itemClass, "getMaxStackSize");
        } else if (access.hasMethod(itemClass, "getDefaultInstance", 0)) {
            Object stack = access.invoke(item, itemClass, "getDefaultInstance");
            size = access.invoke(stack, access.type("net.minecraft.world.item.ItemStack"), "getMaxStackSize");
            adapters.put("stack_size", "Item.getDefaultInstance/ItemStack.getMaxStackSize");
        } else throw new IllegalStateException("target Item has no supported real maximum stack API");
        int result = ((Number) size).intValue();
        if (result <= 0) throw new IllegalStateException("invalid target maximum stack size: " + result);
        return result;
    }

    private boolean containerFit(Object item) {
        if (!access.hasMethod(itemClass, "canFitInsideContainerItems", 0)) {
            // Before bundles introduced the Item hook, shulker nesting is decided
            // by the real container slot. Execute it rather than infer from IDs.
            Class<?> slot = access.type("net.minecraft.world.inventory.ShulkerBoxSlot");
            if (containerSlot == null) {
                Object container = access.construct(access.type("net.minecraft.world.SimpleContainer"), 1);
                containerSlot = access.construct(slot, container, 0, 0, 0);
            }
            Object stack = access.construct(access.type("net.minecraft.world.item.ItemStack"), item);
            adapters.put("can_fit_container", "ShulkerBoxSlot.mayPlace(ItemStack)");
            return (Boolean) access.invoke(containerSlot, slot, "mayPlace", stack);
        }
        adapters.put("can_fit_container", "Item.canFitInsideContainerItems (virtual dispatch)");
        return (Boolean) access.invoke(item, itemClass, "canFitInsideContainerItems");
    }

    private String itemModel(Object item) {
        Class<?> components = access.typeOrNull("net.minecraft.core.component.DataComponents");
        if (components == null || !access.hasMethod(itemClass, "components", 0)) return "";
        Object itemModel;
        try { itemModel = access.staticField(components, "ITEM_MODEL"); }
        catch (IllegalStateException missing) {
            if (missing.getCause() instanceof NoSuchFieldException) return "";
            throw missing;
        }
        Object map = access.invoke(item, itemClass, "components");
        Object model = access.invoke(map, map.getClass(), "get", itemModel);
        return model == null ? "" : model.toString();
    }

    private String key(Object registry, Object entry) {
        if (entry == null) throw new IllegalStateException("target registry item mapping returned null");
        Object key = access.invoke(registry, registry.getClass(), "getKey", entry);
        if (key == null) throw new IllegalStateException("unregistered target value " + entry.getClass().getName());
        return key.toString();
    }

    private static String json(Object value) throws Exception {
        // Preserve the existing deterministic writer on current JDKs; older
        // supported clients use their bundled Gson and sorted TreeMaps.
        try {
            Class<?> writer = Class.forName("net.litematicaba.mc26oracle.CanonicalJsonl");
            return (String) writer.getMethod("json", Object.class).invoke(null, value);
        } catch (ClassNotFoundException absent) {
            return gsonJson(value);
        } catch (UnsupportedClassVersionError olderClientJvm) {
            return gsonJson(value);
        }
    }

    private static String gsonJson(Object value) throws Exception {
        Class<?> gson = Class.forName("com.google.gson.Gson");
        Object writer = gson.getConstructor().newInstance();
        return (String) gson.getMethod("toJson", Object.class).invoke(writer, value);
    }

    private static byte[] readAll(InputStream input) throws Exception {
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        byte[] buffer = new byte[65536];
        for (int count; (count = input.read(buffer)) >= 0;) output.write(buffer, 0, count);
        return output.toByteArray();
    }

    private static String sha256(Path input) throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        try (InputStream stream = Files.newInputStream(input)) {
            byte[] buffer = new byte[65536];
            for (int count; (count = stream.read(buffer)) >= 0;) digest.update(buffer, 0, count);
        }
        StringBuilder result = new StringBuilder();
        for (byte item : digest.digest()) result.append(String.format(Locale.ROOT, "%02x", item & 255));
        return result.toString();
    }
}
