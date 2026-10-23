package net.litematicaba.mc26oracle;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;

/** API-name bridge verified by both mappings of the identical 1.14.4 client.
 * Each older runtime still resolves every SRG member through its own exact map.
 * No semantic records or implementation bytecode come from that reference. */
final class McpOracleAliases {
    private McpOracleAliases() {}
    @SuppressWarnings("unchecked") static void install(MappingAccess access, Path bridge) {
        try {
            Class<?> gson = Class.forName("com.google.gson.Gson");
            Object parser = gson.getConstructor().newInstance();
            Map<String,Object> source = (Map<String,Object>) gson.getMethod("fromJson",String.class,Class.class)
                    .invoke(parser, Files.readString(bridge, StandardCharsets.UTF_8), Map.class);
            if (!"nova.oracle-api-role-bridge.v1".equals(source.get("schema")))
                throw new IllegalArgumentException("unrecognized API role bridge");
            for (List<String> alias : (List<List<String>>) source.get("classes")) {
                try { access.aliasClass(alias.get(0), alias.get(1)); }
                catch (IllegalArgumentException notPresentInTarget) { /* An unused reference API need not exist. */ }
            }
            java.util.Map<String,String> oldOwners = new java.util.HashMap<>();
            if (Boolean.getBoolean("nova.oracle.flattened")) {
                oldOwners.put("net.minecraft.util.registry.Bootstrap", "net.minecraft.init.Bootstrap");
                oldOwners.put("net.minecraft.util.registry.Registry", "net.minecraft.util.registry.IRegistry");
                oldOwners.put("net.minecraft.util.registry.DefaultedRegistry", "net.minecraft.util.registry.RegistryNamespacedDefaultedByKey");
                oldOwners.put("net.minecraft.block.BlockState", "net.minecraft.block.state.IBlockState");
                oldOwners.put("net.minecraft.state.StateHolder", "net.minecraft.state.AbstractStateHolder");
                oldOwners.put("net.minecraft.util.Direction", "net.minecraft.util.EnumFacing");
                oldOwners.put("net.minecraft.block.LeavesBlock", "net.minecraft.block.BlockLeaves");
                oldOwners.put("net.minecraft.world.EmptyBlockReader", "net.minecraft.world.IBlockReader");
                if (access.typeOrNull("net.minecraft.util.math.shapes.VoxelShapes") == null)
                    oldOwners.put("net.minecraft.util.math.shapes.VoxelShapes", "net.minecraft.util.math.shapes.ShapeUtils");
                for (List<String> alias : (List<List<String>>) source.get("classes")) {
                    String actual = oldOwners.get(alias.get(1));
                    if (actual != null) access.aliasClass(alias.get(0), actual);
                }
            }
            java.util.Set<String> consumed = java.util.Set.of(
                "net.minecraft.SharedConstants", "net.minecraft.DetectedVersion", "net.minecraft.server.Bootstrap",
                "net.minecraft.core.registries.BuiltInRegistries", "net.minecraft.core.Registry", "net.minecraft.core.DefaultedRegistry",
                "net.minecraft.resources.ResourceLocation", "net.minecraft.world.level.block.state.StateDefinition",
                "net.minecraft.world.level.block.state.StateHolder", "net.minecraft.world.level.block.state.BlockState",
                "net.minecraft.world.level.block.Block", "net.minecraft.world.level.material.Fluid", "net.minecraft.world.level.material.FluidState",
                "net.minecraft.core.IdMapper", "net.minecraft.core.IdMap", "net.minecraft.world.level.block.entity.BlockEntityType",
                "net.minecraft.world.level.block.state.properties.Property", "net.minecraft.world.level.BlockGetter",
                "net.minecraft.core.BlockPos", "net.minecraft.core.Direction", "net.minecraft.world.phys.shapes.VoxelShape",
                "net.minecraft.world.phys.shapes.Shapes", "net.minecraft.world.phys.AABB", "net.minecraft.world.phys.Vec3",
                "net.minecraft.world.level.EmptyBlockGetter", "net.minecraft.world.level.material.Material",
                "net.minecraft.world.level.block.EntityBlock", "net.minecraft.world.level.block.entity.BlockEntity", "net.minecraft.world.level.block.LeavesBlock",
                "net.minecraft.world.level.levelgen.Heightmap$Types", "net.minecraft.tags.BlockTags");
            for (String category : List.of("fields","methods")) {
                for (List<String> alias : (List<List<String>>) source.get(category)) {
                    if (!consumed.contains(alias.get(0))) continue;
                    try {
                        String actualOwner = oldOwners.getOrDefault(alias.get(2), alias.get(2));
                        if (category.equals("fields")) access.aliasField(alias.get(0),alias.get(1),actualOwner,alias.get(3));
                        else access.aliasMethod(alias.get(0),alias.get(1),actualOwner,alias.get(3));
                    } catch (IllegalArgumentException notPresentInTarget) { /* Actual callers still fail if required. */ }
                }
            }
            if (Boolean.getBoolean("nova.oracle.flattened")) {

                access.aliasMethod("net.minecraft.server.Bootstrap", "bootStrap", "net.minecraft.init.Bootstrap", "func_151354_b");
                access.aliasMethod("net.minecraft.world.level.block.state.BlockState", "getShadeBrightness", "net.minecraft.block.state.IBlockState", "func_185892_j");
                access.aliasMethod("net.minecraft.world.level.block.state.BlockState", "canOcclude", "net.minecraft.block.state.IBlockState", "func_200132_m");
                access.aliasMethod("net.minecraft.world.level.block.state.BlockState", "getLightBlock", "net.minecraft.block.state.IBlockState", "func_200016_a");
                access.aliasMethod("net.minecraft.world.level.block.Block", "skipRendering", "net.minecraft.block.Block", "func_200122_a");
                access.aliasMethod("net.minecraft.world.level.block.state.BlockState", "skipRendering", "net.minecraft.block.state.IBlockState", "func_200017_a");
                access.aliasMethod("net.minecraft.world.level.block.Block", "hasDynamicShape", "net.minecraft.block.Block", "func_208619_r");
                access.aliasMethod("net.minecraft.world.level.material.FluidState", "getOwnHeight", "net.minecraft.fluid.IFluidState", "func_206885_f");
            }
            Class<?> fluidState = access.type("net.minecraft.world.level.material.FluidState");
            if (!access.hasMethod(fluidState, "getOwnHeight", 0) && access.hasMethod(fluidState, "func_206885_f", 0))
                access.aliasMethod("net.minecraft.world.level.material.FluidState", "getOwnHeight", access.canonicalName(fluidState), "func_206885_f");
        } catch (ReflectiveOperationException | java.io.IOException failure) {
            throw new IllegalStateException("cannot initialize exact MCP API role bridge", failure);
        }
    }
}
