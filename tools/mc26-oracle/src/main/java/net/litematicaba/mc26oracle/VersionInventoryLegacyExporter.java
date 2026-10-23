package net.litematicaba.mc26oracle;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

/** Pre-flattening inventory extraction. Legacy numeric/meta normalization comes
 * from this project's exact-version registry, never a newer item-name table. */
public final class VersionInventoryLegacyExporter {
    private final MappingAccess access;
    private final Map<String, String> options;
    private final String version;
    private Class<?> blockClass, itemClass, stackClass;
    private Map<String, Object> registry;
    private Map<String, Object> itemNames;
    private Object shulkerSlot;
    private String defaultBedItem;
    private final Map<String, String> actualVariantNames = new HashMap<String, String>();
    private final Map<Object, Object> placementItems = new IdentityHashMap<Object, Object>();
    private final Map<Object, Integer> placementMetadata = new IdentityHashMap<Object, Integer>();

    private VersionInventoryLegacyExporter(MappingAccess access, Map<String, String> options) {
        this.access = access; this.options = options; this.version = options.get("version");
    }

    public static Map<String, Object> export(MappingAccess access, Map<String, String> options) throws Exception {
        return new VersionInventoryLegacyExporter(access, options).run();
    }

    @SuppressWarnings("unchecked")
    private Map<String, Object> run() throws Exception {
        Class<?> gson = Class.forName("com.google.gson.Gson");
        Object parser = gson.getConstructor().newInstance();
        Map<String, Object> supplement = (Map<String, Object>) gson.getMethod("fromJson", String.class, Class.class).invoke(parser,
                new String(Files.readAllBytes(Paths.get(options.get("legacy-registry"))), StandardCharsets.UTF_8), Map.class);
        Map<String, Object> versions = (Map<String, Object>) supplement.get("versions");
        if (versions != null) {
            Map<String, Object> identity = (Map<String, Object>) versions.get(version);
            if (identity == null) throw new IllegalStateException("legacy registry has no exact version " + version);
            registry = (Map<String, Object>) ((Map<?, ?>) supplement.get("registries")).get(identity.get("registry"));
        } else registry = supplement;
        if (registry == null) throw new IllegalStateException("missing exact legacy registry");
        Class<?> bootstrap = access.typeOrNull("net.minecraft.init.Bootstrap");
        if (bootstrap == null) bootstrap = access.type("net.minecraft.util.registry.Bootstrap");
        access.invokeStatic(bootstrap, "func_151354_b");
        blockClass = access.type("net.minecraft.block.Block");
        itemClass = access.type("net.minecraft.item.Item");
        stackClass = access.type("net.minecraft.item.ItemStack");
        itemNames = (Map<String, Object>) registry.get("legacy_items");
        if (itemNames == null) return runFlattened();
        itemNames = new TreeMap<String, Object>(itemNames);
        // The pre-colour bed item is red, not a white bed. Mojang's existing
        // BedItemColorFix converts its historical Damage=0 to colour index 14.
        // Keep underlying legacy metadata at 0: no coloured variants are invented.
        if (!version.startsWith("1.12") && itemNames.containsKey("355:0")) itemNames.put("355:0", "minecraft:red_bed");
        Class<?> bedEntityType = access.typeOrNull("net.minecraft.tileentity.TileEntityBed");
        if (bedEntityType != null) {
            Object bedEntity = access.construct(bedEntityType);
            Object defaultStack = access.invoke(bedEntity, bedEntityType, "func_193049_f");
            Object defaultItem = access.invoke(defaultStack, stackClass, "func_77973_b");
            int numeric = ((Number) access.invokeStatic(itemClass, "func_150891_b", defaultItem)).intValue();
            int metadata = ((Number) access.invoke(defaultStack, stackClass, "func_77960_j")).intValue();
            Object name = itemNames.get(numeric + ":" + metadata);
            if (name == null) throw new IllegalStateException("target default bed stack lacks an exact item alias");
            defaultBedItem = name.toString();
        }

        Map<String, Object> items = new TreeMap<String, Object>();
        for (Map.Entry<String, Object> entry : new TreeMap<String, Object>(itemNames).entrySet()) {
            int[] identity = numeric(entry.getKey());
            Object item = access.invokeStatic(itemClass, "func_150899_d", identity[0]);
            if (item == null) throw new IllegalStateException("missing actual legacy item " + entry.getKey());
            if (identity[1] != 0 && !(Boolean) access.invoke(item, itemClass, "func_77614_k")) continue;
            Class<?> skullItem = access.typeOrNull("net.minecraft.item.ItemSkull");
            if (skullItem != null && skullItem.isInstance(item)) {
                String[] actualTypes = (String[]) access.staticField(skullItem, "field_82807_a");
                if (identity[1] < 0 || identity[1] >= actualTypes.length) continue;
            }
            Object stack = access.construct(stackClass, item, 1, identity[1]);
            Map<String, Object> row = new TreeMap<String, Object>();
            row.put("stack_size", access.invoke(item, itemClass, "func_77639_j"));
            row.put("translation_key", legacyTranslationKey(item, stack));
            row.put("item_model", "");
            row.put("can_fit_container", containerFit(stack));
            row.put("legacy_numeric_id", identity[0]);
            row.put("legacy_metadata", identity[1]);
            String name = entry.getValue().toString();
            String variantKey = identity[0] + "#" + row.get("translation_key");
            String priorVariant = actualVariantNames.get(variantKey);
            actualVariantNames.put(variantKey, priorVariant == null || priorVariant.equals(name) ? name : "");
            Object prior = items.put(name, row);
            if (prior != null && !prior.equals(row)) throw new IllegalStateException("conflicting exact legacy item variants for " + name);
        }
        Map<String, Object> blocks = new TreeMap<String, Object>();
        Map<String, Object> placements = new TreeMap<String, Object>();
        Map<String, Object> legacyStates = (Map<String, Object>) registry.get("legacy_blocks");
        if (legacyStates == null || legacyStates.isEmpty()) throw new IllegalStateException("missing exact legacy state/meta mapping");
        buildPlacementItems();
        for (Map.Entry<String, Object> entry : new TreeMap<String, Object>(legacyStates).entrySet()) {
            int[] identity = numeric(entry.getKey());
            Object block = access.invokeStatic(blockClass, "func_149729_e", identity[0]);
            Object state = access.invoke(block, blockClass, "func_176203_a", identity[1]);
            Object item = access.invokeStatic(itemClass, "func_150898_a", block);
            int runtimeItemId = item == null ? 0 : ((Number) access.invokeStatic(itemClass, "func_150891_b", item)).intValue();
            if (runtimeItemId == 0) {
                // Placement relationships come from the actual placement Item's
                // Block field or the exact Blocks constants used by onItemUse.
                // Never substitute loot: no getItemDropped/Random call is used.
                item = placementItems.get(block);
            }
            String normalizedState = entry.getValue().toString();
            String normalizedBlock = normalizedState.split("\\[", 2)[0];
            List<String> classes = classes(block);
            String itemId = "minecraft:air";
            if (item != null) {
                int itemNumeric = ((Number) access.invokeStatic(itemClass, "func_150891_b", item)).intValue();
                int damage = ((Number) access.invoke(block, blockClass, "func_180651_a", state)).intValue();
                if (placementMetadata.containsKey(block)) damage = placementMetadata.get(block);
                Object mapped = itemNames.get(itemNumeric + ":" + damage);
                if (mapped == null && access.hasMethod(itemClass, "func_77614_k", 0)
                        && !(Boolean) access.invoke(item, itemClass, "func_77614_k")) mapped = itemNames.get(itemNumeric + ":0");
                if (mapped == null) {
                    // Legacy leaves use a placement metadata offset (e.g. 4/5)
                    // while their creative Item variants are 0/1. Resolve the
                    // exact virtual Item variant label to this version's aliases.
                    Object stack = access.construct(stackClass, item, 1, damage);
                    String translation = legacyTranslationKey(item, stack);
                    String equivalent = actualVariantNames.get(itemNumeric + "#" + translation);
                    if (equivalent != null && !equivalent.isEmpty()) mapped = equivalent;
                }
                Class<?> itemBlock = access.typeOrNull("net.minecraft.item.ItemBlock");
                if (mapped == null && itemBlock != null && itemBlock.isInstance(item)) {
                    Object placedBlock = access.invoke(item, itemBlock, "func_179223_d");
                    int placedNumeric = ((Number) access.invokeStatic(blockClass, "func_149682_b", placedBlock)).intValue();
                    int placedMeta = ((Number) access.invoke(item, itemClass, "func_77647_b", damage)).intValue();
                    Object placedState = legacyStates.get(placedNumeric + ":" + placedMeta);
                    if (placedState == null) {
                        Object normalized = access.invoke(placedBlock, blockClass, "func_176203_a", placedMeta);
                        int normalizedMeta = ((Number) access.invoke(placedBlock, blockClass, "func_176201_c", normalized)).intValue();
                        placedState = legacyStates.get(placedNumeric + ":" + normalizedMeta);
                    }
                    if (placedState != null) {
                        String canonicalItem = placedState.toString().split("\\[", 2)[0];
                        // This is an actual ItemBlock placement through its own
                        // metadata adapter plus an exact-version state alias.
                        if (((Map<?, ?>) registry.get("blocks")).containsKey(canonicalItem)) {
                            mapped = canonicalItem;
                            if (!items.containsKey(canonicalItem)) {
                                // Legal old ItemStack variants are not limited to
                                // the creative tab (e.g. petrified oak slab 44:2).
                                Object actualStack = access.construct(stackClass, item, 1, damage);
                                Map<String, Object> extra = new TreeMap<String, Object>();
                                extra.put("stack_size", access.invoke(item, itemClass, "func_77639_j"));
                                extra.put("translation_key", legacyTranslationKey(item, actualStack));
                                extra.put("item_model", ""); extra.put("can_fit_container", containerFit(actualStack));
                                extra.put("legacy_numeric_id", itemNumeric); extra.put("legacy_metadata", damage);
                                extra.put("legacy_variant_source", "actual ItemBlock placement + exact legacy state alias");
                                items.put(canonicalItem, extra);
                            }
                        }
                    }
                }
                // AIR was not an Item before 1.11; later clients register it as id 0.
                if (itemNumeric != 0 && mapped == null) throw new IllegalStateException("unmapped actual legacy item/meta " + itemNumeric + ":" + damage + " for " + normalizedState);
                if (mapped != null) itemId = mapped.toString();
            }
            if (classes.contains("AbstractBedBlock") && defaultBedItem != null) itemId = defaultBedItem;
            Map<String, Object> row = (Map<String, Object>) blocks.get(normalizedBlock);
            if (row == null) {
                row = new TreeMap<String, Object>(); row.put("item_id", itemId); row.put("classes", new ArrayList<String>(classes)); blocks.put(normalizedBlock, row);
            } else {
                List<String> existing = (List<String>) row.get("classes");
                for (String type : classes) if (!existing.contains(type)) existing.add(type);
                if ("minecraft:air".equals(row.get("item_id")) && !"minecraft:air".equals(itemId)) row.put("item_id", itemId);
            }
            Map<String, Object> placement = new TreeMap<String, Object>();
            placement.put("item_id", itemId);
            long count = 1;
            Class<?> slab = access.typeOrNull("net.minecraft.block.BlockSlab");
            if (slab != null && slab.isInstance(block) && (Boolean) access.invoke(block, slab, "func_176552_j")) count = 2;
            placement.put("count", count);
            placements.put(normalizedState, placement);
        }
        // Legacy powered/unpowered IDs are explicitly normalized to one block
        // by this version's existing state aliases. Share only an unambiguous
        // proven placement identity among those states (not among similar names).
        Map<String, String> placedAliases = new HashMap<String, String>();
        for (Map.Entry<String, Object> entry : placements.entrySet()) {
            Map<String, Object> placement = (Map<String, Object>) entry.getValue();
            String id = placement.get("item_id").toString();
            if (id.equals("minecraft:air")) continue;
            String block = entry.getKey().split("\\[", 2)[0];
            String prior = placedAliases.get(block);
            placedAliases.put(block, prior == null || prior.equals(id) ? id : "");
        }
        for (Map.Entry<String, Object> entry : placements.entrySet()) {
            Map<String, Object> placement = (Map<String, Object>) entry.getValue();
            if (!"minecraft:air".equals(placement.get("item_id"))) continue;
            String block = entry.getKey().split("\\[", 2)[0];
            String placed = placedAliases.get(block);
            if (placed != null && !placed.isEmpty()) {
                placement.put("item_id", placed);
                placement.put("placement_alias_source", "same exact-version canonical block/state aliases");
            }
        }
        Map<String, Object> output = new TreeMap<String, Object>();
        output.put("schema", 1); output.put("minecraft_version", version);
        output.put("client_sha256", options.get("client-sha256"));
        output.put("language_sha1", options.containsKey("language-sha1") ? options.get("language-sha1") : "");
        output.put("blocks", blocks); output.put("items", items); output.put("legacy_state_placements", placements);
        if (defaultBedItem != null) {
            Map<String, Object> bedItems = new TreeMap<String, Object>();
            for (int color = 0; color < 16; color++) {
                Object mapped = itemNames.get("355:" + color);
                if (mapped == null) throw new IllegalStateException("target missing actual bed item colour " + color);
                bedItems.put(Integer.toString(color), mapped);
            }
            output.put("legacy_bed_items", bedItems);
            output.put("legacy_bed_default_item", defaultBedItem);
        }
        for (String[] spec : new String[][]{{"legacy_banner_items", "425", "16"}, {"legacy_skull_items", "397", "6"}}) {
            Map<String, Object> variants = new TreeMap<String, Object>();
            for (int data = 0; data < Integer.parseInt(spec[2]); data++) {
                Object id = itemNames.get(spec[1] + ":" + data);
                if (id != null && items.containsKey(id.toString())) variants.put(Integer.toString(data), id);
            }
            if (!variants.isEmpty()) output.put(spec[0], variants);
        }
        Map<String, Object> adapters = new TreeMap<String, Object>();
        adapters.put("registries", "target MCP SRG/TSRG + exact-version NOVAST04 legacy numeric/meta aliases");
        adapters.put("item_mapping", "actual Item.getItemFromBlock/placement Item Block associations + verified ItemBlock metadata");
        adapters.put("container", shulkerSlot == null ? "target has no ShulkerBoxSlot/container nesting restriction" : "actual SlotShulkerBox.isItemValid(ItemStack)");
        adapters.put("potted_contents", "legacy BlockFlowerPot contents require source flower-pot block-entity Item/Data NBT");
        output.put("adapters", adapters);
        return output;
    }

    private void buildPlacementItems() {
        Object actualItems = access.staticField(itemClass, "field_150901_e");
        Class<?> blocks = access.type("net.minecraft.init.Blocks");
        for (Object item : (Iterable<?>) actualItems) {
            for (String[] rule : new String[][]{
                    {"ItemDoor", "field_179236_a"}, {"ItemSeeds", "field_150925_a"},
                    {"ItemSeedFood", "field_150908_b"}, {"ItemReed", "field_150935_a"},
                    {"ItemBlockSpecial", "field_150935_a"}}) {
                Class<?> kind = access.typeOrNull("net.minecraft.item." + rule[0]);
                if (kind != null && kind.isInstance(item)) {
                    Object target = access.field(item, kind, rule[1]);
                    if (!blockClass.isInstance(target)) throw new IllegalStateException("placement Item field is not a Block: " + rule[0]);
                    placementItems.put(target, item);
                }
            }
            Class<?> dye = access.typeOrNull("net.minecraft.item.ItemDye");
            if (dye != null && dye.isInstance(item)) {
                Object cocoa = access.staticField(blocks, "field_150375_by");
                Class<?> color = access.type("net.minecraft.item.EnumDyeColor");
                Object brown = access.staticField(color, "BROWN");
                int metadata = ((Number) access.invoke(brown, color, "func_176767_b")).intValue();
                placementItems.put(cocoa, item); placementMetadata.put(cocoa, metadata);
            }
            for (String[] rule : new String[][]{
                    {"ItemBed", "field_150324_C"}, {"ItemRedstone", "field_150488_af"},
                    {"ItemSign", "field_150472_an", "field_150444_as"},
                    {"ItemSkull", "field_150465_bP"},
                    {"ItemBanner", "field_180393_cK", "field_180394_cL"}}) {
                Class<?> kind = access.typeOrNull("net.minecraft.item." + rule[0]);
                if (kind != null && kind.isInstance(item)) {
                    for (int i = 1; i < rule.length; i++) placementItems.put(access.staticField(blocks, rule[i]), item);
                }
            }
        }
    }

    private String legacyTranslationKey(Object item, Object stack) {
        // Legacy Item.getItemStackDisplayName appends .name to the variant's
        // unlocalized key; these are the actual keys in the target .lang files.
        return access.invoke(item, itemClass, "func_77667_c", stack).toString() + ".name";
    }

    private boolean containerFit(Object stack) {
        Class<?> slot = access.typeOrNull("net.minecraft.inventory.SlotShulkerBox");
        if (slot == null) slot = access.typeOrNull("net.minecraft.inventory.container.ShulkerBoxSlot");
        if (slot == null) {
            if (!(version.equals("1.8") || version.startsWith("1.9") || version.startsWith("1.10"))) {
                throw new IllegalStateException("target should contain its historical shulker slot");
            }
            return true;
        }
        if (shulkerSlot == null) {
            Object inventory;
            if (version.startsWith("1.14")) {
                inventory = access.construct(access.type("net.minecraft.inventory.Inventory"), 1);
            } else if (version.startsWith("1.13")) {
                Class<?> literal = access.typeOrNull("net.minecraft.util.text.TextComponentString");
                if (literal == null) literal = access.type("net.minecraft.util.text.StringTextComponent");
                Object title = access.construct(literal, "Nova inventory export");
                inventory = access.construct(access.type("net.minecraft.inventory.InventoryBasic"), title, 1);
            } else inventory = access.construct(access.type("net.minecraft.inventory.InventoryBasic"), "Nova inventory export", false, 1);
            shulkerSlot = access.construct(slot, inventory, 0, 0, 0);
        }
        return (Boolean) access.invoke(shulkerSlot, slot, "func_75214_a", stack);
    }

    private Map<String, Object> runFlattened() {
        Object blocksRegistry, itemsRegistry;
        if (access.hasField(blockClass, "field_149771_c")) {
            blocksRegistry = access.staticField(blockClass, "field_149771_c");
            itemsRegistry = access.staticField(itemClass, "field_150901_e");
        } else {
            Class<?> registryClass = access.typeOrNull("net.minecraft.util.registry.IRegistry");
            if (registryClass == null) registryClass = access.type("net.minecraft.util.registry.Registry");
            blocksRegistry = access.staticField(registryClass, "field_212618_g");
            itemsRegistry = access.staticField(registryClass, "field_212630_s");
        }
        Map<String, Object> items = new TreeMap<String, Object>();
        for (Object item : (Iterable<?>) itemsRegistry) {
            Map<String, Object> row = new TreeMap<String, Object>();
            row.put("stack_size", access.invoke(item, itemClass, "func_77639_j"));
            row.put("translation_key", access.invoke(item, itemClass, "func_77658_a"));
            row.put("item_model", "");
            Object stack = access.invoke(item, itemClass, "func_190903_i");
            row.put("can_fit_container", containerFit(stack));
            items.put(flatKey(itemsRegistry, item), row);
        }
        Map<String, Object> blocks = new TreeMap<String, Object>();
        Class<?> pot = access.typeOrNull("net.minecraft.block.BlockFlowerPot");
        if (pot == null) pot = access.type("net.minecraft.block.FlowerPotBlock");
        for (Object block : (Iterable<?>) blocksRegistry) {
            Map<String, Object> row = new TreeMap<String, Object>();
            Object item = access.hasMethod(blockClass, "func_199767_j", 0)
                    ? access.invoke(block, blockClass, "func_199767_j")
                    : access.invokeStatic(itemClass, "func_150898_a", block);
            if (item != null && "minecraft:air".equals(flatKey(itemsRegistry, item))) {
                for (String kind : new String[]{"BlockCrops", "BlockStem", "BlockAttachedStem", "BlockNetherWart", "BlockCocoa"}) {
                    Class<?> plant = access.typeOrNull("net.minecraft.block." + kind);
                    if (plant == null || !plant.isInstance(block)) continue;
                    Object state = access.invoke(block, blockClass, "func_176223_P");
                    Object position = access.construct(access.type("net.minecraft.util.math.BlockPos"), 0, 0, 0);
                    Object stack = access.invoke(block, blockClass, "func_185473_a", null, position, state);
                    item = access.invoke(stack, stackClass, "func_77973_b");
                    break;
                }
            }
            row.put("item_id", item == null ? "minecraft:air" : flatKey(itemsRegistry, item));
            row.put("classes", classes(block));
            if (pot.isInstance(block)) {
                Object potted = access.field(block, pot, "field_196452_c");
                Object plantItem = access.hasMethod(blockClass, "func_199767_j", 0)
                        ? access.invoke(potted, blockClass, "func_199767_j")
                        : access.invokeStatic(itemClass, "func_150898_a", potted);
                row.put("potted_item", plantItem == null ? "minecraft:air" : flatKey(itemsRegistry, plantItem));
            }
            blocks.put(flatKey(blocksRegistry, block), row);
        }
        Map<String, Object> output = new TreeMap<String, Object>();
        output.put("schema", 1); output.put("minecraft_version", version);
        output.put("client_sha256", options.get("client-sha256"));
        output.put("language_sha1", options.containsKey("language-sha1") ? options.get("language-sha1") : "");
        output.put("blocks", blocks); output.put("items", items);
        Map<String, Object> adapters = new TreeMap<String, Object>();
        adapters.put("registries", "target MCP exact flattened RegistryNamespaced/IRegistry");
        adapters.put("item_mapping", "actual Item.getItemFromBlock");
        adapters.put("container", "actual SlotShulkerBox.isItemValid(ItemStack)");
        adapters.put("potted_item", "actual BlockFlowerPot.potted field via exact MCP mapping");
        output.put("adapters", adapters);
        return output;
    }

    private String flatKey(Object registry, Object entry) {
        Object key = access.invoke(registry, registry.getClass(), "func_177774_c", entry);
        if (key == null) throw new IllegalStateException("target flattened registry entry has no key");
        return key.toString();
    }

    private List<String> classes(Object block) {
        List<String> result = new ArrayList<String>();
        Map<String, String> aliases = new HashMap<String, String>();
        aliases.put("net.minecraft.block.BlockAir", "AirBlock");
        aliases.put("net.minecraft.block.BlockDoor", "DoorBlock");
        aliases.put("net.minecraft.block.BlockDoublePlant", "DoublePlantBlock");
        aliases.put("net.minecraft.block.BlockBed", "AbstractBedBlock");
        aliases.put("net.minecraft.block.BlockSlab", "SlabBlock");
        if (!version.startsWith("1.13")) aliases.put("net.minecraft.block.BlockSnow", "SnowLayerBlock");
        aliases.put("net.minecraft.block.BlockSnowLayer", "SnowLayerBlock");
        aliases.put("net.minecraft.block.BlockLiquid", "LiquidBlock");
        aliases.put("net.minecraft.block.BlockFlowingFluid", "LiquidBlock");
        aliases.put("net.minecraft.block.BlockSeaGrassTall", "TallSeagrassBlock");
        aliases.put("net.minecraft.block.BlockSeaPickle", "SeaPickleBlock");
        aliases.put("net.minecraft.block.BlockTurtleEgg", "TurtleEggBlock");
        aliases.put("net.minecraft.block.BedBlock", "AbstractBedBlock");
        aliases.put("net.minecraft.block.FlowingFluidBlock", "LiquidBlock");
        aliases.put("net.minecraft.block.TallSeaGrassBlock", "TallSeagrassBlock");
        if (version.startsWith("1.14")) aliases.put("net.minecraft.block.SnowBlock", "SnowLayerBlock");
        for (Class<?> type = block.getClass(); type != null && type != blockClass; type = type.getSuperclass()) {
            result.add(access.simpleName(type));
            String alias = aliases.get(access.canonicalName(type));
            if (alias != null && !result.contains(alias)) result.add(alias);
        }
        return result;
    }

    private static int[] numeric(String value) {
        String[] values = value.split(":", 2);
        return new int[]{Integer.parseInt(values[0]), values.length == 1 ? 0 : Integer.parseInt(values[1])};
    }
}
