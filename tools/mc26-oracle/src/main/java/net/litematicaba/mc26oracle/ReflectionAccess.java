package net.litematicaba.mc26oracle;

import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;

public final class ReflectionAccess {
    private ReflectionAccess() {}

    static final MappingAccess MAPPING = loadMappings();
    private static MappingAccess loadMappings() {
        try {
            String mcp = System.getProperty("nova.oracle.mcp.srg");
            if (mcp != null) {
                MappingAccess access = MappingAccess.fromMcp(java.nio.file.Path.of(mcp), java.nio.file.Path.of(System.getProperty("nova.oracle.mcp.csv")));
                McpOracleAliases.install(access, java.nio.file.Path.of(System.getProperty("nova.oracle.mcp.bridge")));
                return access;
            }
            String path = System.getProperty("nova.oracle.mappings");
            return new MappingAccess(path == null || path.isBlank() ? null : java.nio.file.Path.of(path));
        } catch (java.io.IOException failure) { throw new IllegalStateException("invalid exact runtime mappings", failure); }
    }

    static Method method(Class<?> owner, String name, Class<?>... parameterTypes) {
        return MAPPING.method(owner, name, parameterTypes);
    }
    static Field fieldHandle(Class<?> owner, String name) { return MAPPING.fieldHandle(owner, name); }

    static Object invokeCompatible(Object target, Class<?> owner, String[] names, Object... args) {
        for (String name : names) {
            if (MAPPING.hasMethod(owner, name, args.length)) return MAPPING.invoke(target, owner, name, args);
        }
        throw new IllegalStateException("missing exact runtime methods " + owner.getName() + java.util.Arrays.toString(names));
    }


    public static Class<?> type(String className) {
        Class<?> type = MAPPING.typeOrNull(className);
        if (type == null && className.equals("net.minecraft.resources.Identifier"))
            type = MAPPING.typeOrNull("net.minecraft.resources.ResourceLocation");
        if (type == null && className.equals("net.minecraft.core.registries.BuiltInRegistries"))
            type = MAPPING.typeOrNull("net.minecraft.core.Registry");
        if (type == null && className.equals("net.minecraft.WorldVersion"))
            type = MAPPING.typeOrNull("com.mojang.bridge.game.GameVersion");
        if (type == null && className.equals("net.minecraft.world.level.block.state.BlockBehaviour"))
            type = MAPPING.typeOrNull("net.minecraft.world.level.block.Block");
        if (type == null && className.equals("net.minecraft.world.level.block.state.BlockBehaviour$BlockStateBase"))
            type = MAPPING.typeOrNull("net.minecraft.world.level.block.state.BlockState");
        if (type == null) throw new IllegalStateException("missing runtime class " + className);
        return type;
    }

    private static Object flattenedEmptyContext;
    private static Object flattenedProbeState;
    private static Object flattenedAirState;
    private static final java.util.IdentityHashMap<Object,Object> flattenedProbeEntities = new java.util.IdentityHashMap<>();
    static void setProbeState(Object state) { if (Boolean.getBoolean("nova.oracle.flattened")) flattenedProbeState = state; }
    private static boolean isProbePosition(Object position) {
        return ((Number) MAPPING.invoke(position, type("net.minecraft.core.BlockPos"), "getX")).intValue() == 0
            && ((Number) MAPPING.invoke(position, type("net.minecraft.core.BlockPos"), "getY")).intValue() == 0
            && ((Number) MAPPING.invoke(position, type("net.minecraft.core.BlockPos"), "getZ")).intValue() == 0;
    }
    private static Object probeBlockEntity() {
        if (flattenedProbeState == null) return null;
        Object block = MAPPING.invoke(flattenedProbeState, type("net.minecraft.world.level.block.state.BlockState"), "getBlock");
        Class<?> provider = type("net.minecraft.world.level.block.EntityBlock");
        if (!provider.isInstance(block)) return null;
        Object entity = flattenedProbeEntities.computeIfAbsent(block, value -> {
            Object instance = MAPPING.invoke(value, provider, "newBlockEntity", flattenedEmptyContext);
            if (instance == null && MAPPING.hasMethod(value.getClass(), "func_196343_a", 4))
                instance = MAPPING.invokeStatic(value.getClass(), "func_196343_a", flattenedAirState, staticField(type("net.minecraft.core.Direction"), "NORTH"), false, false);
            if (instance == null) throw new IllegalStateException("actual block entity provider requires unavailable state: " + value);
            return instance;
        });
        try { MAPPING.fieldHandle(type("net.minecraft.world.level.block.entity.BlockEntity"), "cachedBlockState").set(entity, flattenedProbeState); }
        catch (IllegalAccessException failure) { throw new IllegalStateException(failure); }
        return entity;
    }
    public static Object staticField(Class<?> owner, String name) {
        if (Boolean.getBoolean("nova.oracle.flattened") && name.equals("INSTANCE")
                && MAPPING.canonicalName(owner).equals("net.minecraft.world.IBlockReader")) {
            if (flattenedEmptyContext == null) flattenedEmptyContext = createFlattenedEmptyContext(owner);
            return flattenedEmptyContext;
        }
        return MAPPING.staticField(owner, name);
    }

    private static Object createFlattenedEmptyContext(Class<?> reader) {
        Class<?> registries = type("net.minecraft.core.Registry");
        Object registry = MAPPING.hasField(registries, "BLOCK") ? MAPPING.staticField(registries, "BLOCK")
                : MAPPING.staticField(type("net.minecraft.world.level.block.Block"), "REGISTRY");
        Object air = null;
        for (Object value : (Iterable<?>) registry) {
            if (invokeCompatible(registry, registry.getClass(), new String[]{"getKey", "func_177774_c"}, value).toString().equals("minecraft:air")) { air = value; break; }
        }
        if (air == null) throw new IllegalStateException("target has no actual air block");
        Object emptyState = MAPPING.invoke(air, type("net.minecraft.world.level.block.Block"), "defaultBlockState");
        flattenedAirState = emptyState;
        Object emptyFluid = MAPPING.invoke(emptyState, type("net.minecraft.world.level.block.state.BlockState"), "getFluidState");
        return java.lang.reflect.Proxy.newProxyInstance(reader.getClassLoader(), new Class<?>[]{reader}, (proxy, method, arguments) -> {
            if (method.isDefault()) return java.lang.reflect.InvocationHandler.invokeDefault(proxy, method, arguments);
            if (method.getDeclaringClass() == Object.class) return switch(method.getName()) {
                case "toString" -> "ExactVanillaEmptyBlockReader";
                case "hashCode" -> System.identityHashCode(proxy);
                case "equals" -> proxy == arguments[0];
                default -> throw new IllegalStateException(method.toString());
            };
            String result = MAPPING.canonicalName(method.getReturnType());
            boolean own = arguments != null && arguments.length == 1 && flattenedProbeState != null && isProbePosition(arguments[0]);
            if (result.equals("net.minecraft.block.state.IBlockState")) return own ? flattenedProbeState : emptyState;
            if (result.equals("net.minecraft.fluid.IFluidState")) return own ? MAPPING.invoke(flattenedProbeState, type("net.minecraft.world.level.block.state.BlockState"), "getFluidState") : emptyFluid;
            if (result.equals("net.minecraft.tileentity.TileEntity")) return own ? probeBlockEntity() : null;
            throw new IllegalStateException("unsupported empty reader operation: " + method);
        });
    }

    public static Object invokeStatic(
            Class<?> owner, String name, Class<?>[] parameterTypes, Object... arguments) {
        return invokeMethod(null, owner, name, parameterTypes, arguments);
    }

    public static Object invoke(
            Object target,
            Class<?> owner,
            String name,
            Class<?>[] parameterTypes,
            Object... arguments) {
        if (target == null) {
            throw new IllegalArgumentException("target must not be null");
        }
        return invokeMethod(target, owner, name, parameterTypes, arguments);
    }

    public static Object invoke(Object target, Class<?> owner, String name) {
        return invoke(target, owner, name, new Class<?>[0]);
    }

    public static <T> T construct(
            Class<T> owner, Class<?>[] parameterTypes, Object... arguments) {
        try {
            Constructor<T> constructor = owner.getDeclaredConstructor(parameterTypes);
            constructor.setAccessible(true);
            return constructor.newInstance(arguments);
        } catch (InvocationTargetException error) {
            Throwable cause = error.getCause();
            if (cause instanceof RuntimeException runtimeException) {
                throw runtimeException;
            }
            if (cause instanceof Error invocationError) {
                throw invocationError;
            }
            throw failure(owner, "<init>", cause);
        } catch (ReflectiveOperationException error) {
            throw failure(owner, "<init>", error);
        }
    }

    private static Object invokeMethod(
            Object target,
            Class<?> owner,
            String name,
            Class<?>[] parameterTypes,
            Object[] arguments) {
        try {
            if (name.equals("isSolid") && Boolean.getBoolean("nova.oracle.flattened")) {
                Object material = MAPPING.invoke(target, owner, "getMaterial");
                return MAPPING.invoke(material, material.getClass(), "isSolid");
            }
            Method method;
            try { method = method(owner, name, parameterTypes); }
            catch (IllegalStateException missing) {
                if (name.equals("isSolid")) {
                    Object material = MAPPING.invoke(target, owner, "getMaterial");
                    return MAPPING.invoke(material, material.getClass(), "isSolid");
                }
                if (!(name.equals("isSolidRender") || name.equals("getFaceOcclusionShape")
                    || name.equals("emissiveRendering") || name.equals("propagatesSkylightDown")
                    || name.equals("getOffset") || name.equals("getLightDampening"))) throw missing;
                Class<?>[] contextual = new Class<?>[parameterTypes.length + 2];
                contextual[0] = type("net.minecraft.world.level.BlockGetter");
                contextual[1] = type("net.minecraft.core.BlockPos");
                System.arraycopy(parameterTypes, 0, contextual, 2, parameterTypes.length);
                method = method(owner, name.equals("getLightDampening") ? "getLightBlock" : name, contextual);
                Object[] values = new Object[arguments.length + 2];
                values[0] = staticField(type("net.minecraft.world.level.EmptyBlockGetter"), "INSTANCE");
                values[1] = staticField(type("net.minecraft.core.BlockPos"), "ZERO");
                System.arraycopy(arguments, 0, values, 2, arguments.length);
                arguments = values;
            }
            method.setAccessible(true);
            return method.invoke(target, arguments);
        } catch (InvocationTargetException error) {
            Throwable cause = error.getCause();
            if (cause instanceof RuntimeException runtimeException) {
                throw runtimeException;
            }
            if (cause instanceof Error invocationError) {
                throw invocationError;
            }
            throw failure(owner, name, cause);
        } catch (ReflectiveOperationException error) {
            throw failure(owner, name, error);
        }
    }

    private static IllegalStateException failure(Class<?> owner, String member, Throwable cause) {
        return new IllegalStateException(
                "cannot access " + owner.getName() + "#" + member, cause);
    }
}
