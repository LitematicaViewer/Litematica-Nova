package net.litematicaba.mc26oracle;

import java.io.IOException;
import java.io.InputStream;
import java.io.ByteArrayOutputStream;
import java.lang.reflect.Field;
import java.lang.reflect.Constructor;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.zip.ZipFile;

/** Reflection against either named vanilla or that exact client's official mappings.
 * Kept Java 8 compatible so registry extraction uses each client's supported JVM.
 * No neighbouring-version mappings or field-position/name heuristics are used. */
public final class MappingAccess {
    private final Map<String, String> classes = new HashMap<String, String>();
    private final Map<String, String> originalClasses = new HashMap<String, String>();
    private final Map<String, String> fields = new HashMap<String, String>();
    private final Map<String, List<Member>> methods = new HashMap<String, List<Member>>();

    private static final class Member {
        final String runtimeName;
        final String[] parameters;
        Member(String runtimeName, String[] parameters) {
            this.runtimeName = runtimeName;
            this.parameters = parameters;
        }
    }

    public MappingAccess(Path mappingFile) throws IOException {
        if (mappingFile == null) return;
        String owner = null;
        for (String line : Files.readAllLines(mappingFile, StandardCharsets.UTF_8)) {
            if (line.length() == 0 || line.charAt(0) == '#') continue;
            int arrow = line.indexOf(" -> ");
            if (arrow < 0) continue;
            if (!Character.isWhitespace(line.charAt(0))) {
                owner = line.substring(0, arrow).trim();
                String runtime = line.substring(arrow + 4).trim();
                if (!runtime.endsWith(":")) throw new IOException("invalid mapping class: " + line);
                runtime = runtime.substring(0, runtime.length() - 1);
                classes.put(owner, runtime);
                originalClasses.put(runtime, owner);
                continue;
            }
            if (owner == null) throw new IOException("mapping member without class: " + line);
            String left = line.substring(0, arrow).trim().replaceFirst("^\\d+:\\d+:", "");
            String runtimeName = line.substring(arrow + 4).trim();
            int open = left.indexOf('(');
            if (open < 0) {
                int split = left.lastIndexOf(' ');
                if (split > 0) fields.put(owner + "#" + left.substring(split + 1), runtimeName);
            } else {
                int close = left.indexOf(')', open);
                int split = left.lastIndexOf(' ', open);
                if (close < 0 || split < 0) throw new IOException("invalid method mapping: " + line);
                String name = left.substring(split + 1, open);
                // Inlined source frames are not declarations of this owner.
                if (name.indexOf('.') >= 0) continue;
                String parameters = left.substring(open + 1, close);
                String[] types = parameters.length() == 0 ? new String[0] : parameters.split(",");
                String key = owner + "#" + name + "#" + types.length;
                List<Member> rows = methods.get(key);
                if (rows == null) { rows = new ArrayList<Member>(); methods.put(key, rows); }
                rows.add(new Member(runtimeName, types));
            }
        }
        if (classes.isEmpty()) throw new IOException("no class mappings in " + mappingFile);
    }

    /** MCP's exact target SRG/TSRG supplies obfuscation; CSV only resolves its
     * stable member identifiers to descriptive names, never to neighbouring bytecode. */
    public static MappingAccess fromMcp(Path srgArchive, Path csvArchive) throws IOException {
        MappingAccess result = new MappingAccess(null);
        Map<String, String> names = new HashMap<String, String>();
        for (String entry : new String[]{"methods.csv", "fields.csv"}) {
            for (String line : archiveText(csvArchive, entry).split("\\r?\\n")) {
                String[] columns = line.split(",", 3);
                if (columns.length >= 2) names.put(columns[0], columns[1]);
            }
        }
        String text;
        boolean tsrg;
        try (ZipFile archive = new ZipFile(srgArchive.toFile())) {
            tsrg = archive.getEntry("joined.srg") == null;
        }
        text = archiveText(srgArchive, tsrg ? "config/joined.tsrg" : "joined.srg");
        String runtimeOwner = null, owner = null;
        for (String line : text.split("\\r?\\n")) {
            if (line.isEmpty() || line.startsWith("#") || line.startsWith("PK:")) continue;
            String[] columns = line.trim().split("\\s+");
            if ((!tsrg && line.startsWith("CL:")) || (tsrg && !Character.isWhitespace(line.charAt(0)))) {
                runtimeOwner = columns[tsrg ? 0 : 1].replace('/', '.');
                owner = columns[tsrg ? 1 : 2].replace('/', '.');
                result.classes.put(owner, runtimeOwner);
                result.originalClasses.put(runtimeOwner, owner);
                continue;
            }
            if (!tsrg && line.startsWith("FD:")) {
                String canonical = columns[2];
                String canonicalOwner = canonical.substring(0, canonical.lastIndexOf('/')).replace('/', '.');
                String srg = canonical.substring(canonical.lastIndexOf('/') + 1);
                String runtime = columns[1].substring(columns[1].lastIndexOf('/') + 1);
                result.fields.put(canonicalOwner + "#" + srg, runtime);
                if (names.containsKey(srg)) result.fields.put(canonicalOwner + "#" + names.get(srg), runtime);
            } else if (!tsrg && line.startsWith("MD:")) {
                String canonical = columns[3];
                String canonicalOwner = canonical.substring(0, canonical.lastIndexOf('/')).replace('/', '.');
                String srg = canonical.substring(canonical.lastIndexOf('/') + 1);
                String runtime = columns[1].substring(columns[1].lastIndexOf('/') + 1);
                result.addMcpMethod(canonicalOwner, srg, names.get(srg), runtime, descriptorParameters(columns[4]));
            } else if (tsrg && Character.isWhitespace(line.charAt(0))) {
                if (owner == null) throw new IOException("TSRG member without class");
                if (columns.length == 2) {
                    String srg = columns[1];
                    result.fields.put(owner + "#" + srg, columns[0]);
                    if (names.containsKey(srg)) result.fields.put(owner + "#" + names.get(srg), columns[0]);
                } else if (columns.length == 3 && columns[1].startsWith("(")) {
                    // TSRG descriptors use obfuscated names; store then normalize
                    // after the complete class map has been read below.
                    result.addMcpMethod(owner, columns[2], names.get(columns[2]), columns[0], descriptorParameters(columns[1]));
                }
            }
        }
        if (tsrg) {
            for (List<Member> rows : result.methods.values()) for (Member row : rows) {
                for (int i = 0; i < row.parameters.length; i++) {
                    String type = row.parameters[i]; int suffix = type.indexOf('[');
                    String base = suffix < 0 ? type : type.substring(0, suffix);
                    String canonical = result.originalClasses.get(base);
                    if (canonical != null) row.parameters[i] = canonical + (suffix < 0 ? "" : type.substring(suffix));
                }
            }
        }
        if (result.classes.isEmpty()) throw new IOException("no exact MCP class mappings in " + srgArchive);
        return result;
    }

    private void addMcpMethod(String owner, String srg, String resolved, String runtime, String[] parameters) {
        addMethod(owner + "#" + srg + "#" + parameters.length, new Member(runtime, parameters));
        if (resolved != null) addMethod(owner + "#" + resolved + "#" + parameters.length, new Member(runtime, parameters.clone()));
    }

    private void addMethod(String key, Member member) {
        List<Member> rows = methods.get(key);
        if (rows == null) { rows = new ArrayList<Member>(); methods.put(key, rows); }
        rows.add(member);
    }

    private static String archiveText(Path path, String name) throws IOException {
        try (ZipFile zip = new ZipFile(path.toFile())) {
            if (zip.getEntry(name) == null) throw new IOException("missing " + name + " in " + path);
            try (InputStream input = zip.getInputStream(zip.getEntry(name))) {
                ByteArrayOutputStream bytes = new ByteArrayOutputStream();
                byte[] buffer = new byte[65536];
                for (int count; (count = input.read(buffer)) >= 0;) bytes.write(buffer, 0, count);
                return new String(bytes.toByteArray(), StandardCharsets.UTF_8);
            }
        }
    }

    private static String[] descriptorParameters(String descriptor) throws IOException {
        List<String> result = new ArrayList<String>();
        int i = 1;
        while (descriptor.charAt(i) != ')') {
            int dimensions = 0;
            while (descriptor.charAt(i) == '[') { dimensions++; i++; }
            char kind = descriptor.charAt(i++);
            String name;
            if (kind == 'L') {
                int end = descriptor.indexOf(';', i);
                if (end < 0) throw new IOException("invalid JVM descriptor " + descriptor);
                name = descriptor.substring(i, end).replace('/', '.'); i = end + 1;
            } else {
                int primitive = "ZBCSIJFD".indexOf(kind);
                if (primitive < 0) throw new IOException("invalid JVM parameter " + descriptor);
                name = new String[]{"boolean", "byte", "char", "short", "int", "long", "float", "double"}[primitive];
            }
            while (dimensions-- > 0) name += "[]";
            result.add(name);
        }
        return result.toArray(new String[result.size()]);
    }

    public Class<?> type(String name) {
        Class<?> result = typeOrNull(name);
        if (result == null) throw new IllegalStateException("missing target runtime class " + name);
        return result;
    }

    public Class<?> uninitializedType(String name) {
        try { return Class.forName(runtimeName(name), false, MappingAccess.class.getClassLoader()); }
        catch (ClassNotFoundException missing) { throw new IllegalStateException("missing target runtime class " + name, missing); }
    }

    public Class<?> typeOrNull(String name) {
        try { return Class.forName(runtimeName(name)); }
        catch (ClassNotFoundException missing) { return null; }
    }

    public String canonicalName(Class<?> type) {
        String mapped = originalClasses.get(type.getName());
        return mapped == null ? type.getName() : mapped;
    }

    public String simpleName(Class<?> type) {
        String name = canonicalName(type);
        return name.substring(Math.max(name.lastIndexOf('.'), name.lastIndexOf('$')) + 1);
    }

    /** Explicit API-role aliases supplied by a verified version adapter. The
     * original MCP class identity and member map remain intact. */
    public void aliasClass(String canonicalAlias, String actualCanonical) {
        String runtime = classes.get(actualCanonical);
        if (runtime == null) throw new IllegalArgumentException("alias target has no exact mapping: " + actualCanonical);
        classes.put(canonicalAlias, runtime);
    }

    public void aliasMethod(String canonicalOwner, String alias, String actualOwner, String actualName) {
        String targetOwner = originalClasses.get(runtimeName(canonicalOwner));
        if (targetOwner == null) targetOwner = canonicalOwner;
        boolean found = false;
        String prefix = actualOwner + "#" + actualName + "#";
        Map<String, List<Member>> additions = new HashMap<String, List<Member>>();
        for (Map.Entry<String, List<Member>> entry : methods.entrySet()) {
            if (entry.getKey().startsWith(prefix)) {
                additions.put(targetOwner + "#" + alias + "#" + entry.getKey().substring(prefix.length()), new ArrayList<Member>(entry.getValue()));
                found = true;
            }
        }
        if (!found) throw new IllegalArgumentException("method alias target has no exact mapping: " + actualOwner + "#" + actualName);
        methods.putAll(additions);
    }

    public void aliasField(String canonicalOwner, String alias, String actualOwner, String actualName) {
        String runtime = fields.get(actualOwner + "#" + actualName);
        if (runtime == null) throw new IllegalArgumentException("field alias target has no exact mapping: " + actualOwner + "#" + actualName);
        String targetOwner = originalClasses.get(runtimeName(canonicalOwner));
        if (targetOwner == null) targetOwner = canonicalOwner;
        fields.put(targetOwner + "#" + alias, runtime);
    }

    private String runtimeName(String name) {
        String mapped = classes.get(name);
        return mapped == null ? name : mapped;
    }

    public Object staticField(Class<?> owner, String name) { return field(null, owner, name); }

    public Field fieldHandle(Class<?> owner, String name) {
        for (Class<?> current : hierarchy(owner)) {
            String mapped = fields.get(canonicalName(current) + "#" + name);
            try {
                Field field = current.getDeclaredField(mapped == null ? name : mapped);
                field.setAccessible(true);
                return field;
            } catch (NoSuchFieldException absent) { /* Check the real declaration chain. */ }
        }
        throw failure(owner, name, new NoSuchFieldException(name));
    }

    public boolean hasField(Class<?> owner, String name) {
        try { fieldHandle(owner, name); return true; }
        catch (IllegalStateException absent) {
            if (absent.getCause() instanceof NoSuchFieldException) return false;
            throw absent;
        }
    }

    public Method method(Class<?> owner, String name, Class<?>... parameterTypes) {
        List<Method> exact = new ArrayList<Method>();
        for (Method method : findMethods(owner, name, parameterTypes.length, null, false)) {
            Class<?>[] actual = method.getParameterTypes();
            boolean same = true;
            for (int i = 0; i < actual.length; i++) if (actual[i] != parameterTypes[i]) { same = false; break; }
            if (same) exact.add(method);
        }
        if (exact.size() != 1) throw failure(owner, name,
                new NoSuchMethodException("expected one exact target signature, found " + exact));
        Method result = exact.get(0);
        result.setAccessible(true);
        return result;
    }

    public Object field(Object instance, Class<?> owner, String name) {
        for (Class<?> current : hierarchy(owner)) {
            String mapped = fields.get(canonicalName(current) + "#" + name);
            try {
                Field field = current.getDeclaredField(mapped == null ? name : mapped);
                if (instance == null && !Modifier.isStatic(field.getModifiers())) continue;
                field.setAccessible(true);
                return field.get(instance);
            } catch (NoSuchFieldException absent) {
                // A Java member may genuinely be declared by a superclass.
            } catch (ReflectiveOperationException error) {
                throw failure(current, name, error);
            }
        }
        throw failure(owner, name, new NoSuchFieldException(name));
    }

    public boolean hasMethod(Class<?> owner, String name, int parameterCount) {
        return !findMethods(owner, name, parameterCount, null, false).isEmpty();
    }

    public Object invokeStatic(Class<?> owner, String name, Object... arguments) {
        return call(null, owner, name, arguments);
    }

    public Object invoke(Object instance, Class<?> owner, String name, Object... arguments) {
        if (instance == null) throw new IllegalArgumentException("null receiver for " + name);
        return call(instance, owner, name, arguments);
    }

    public Object construct(Class<?> owner, Object... arguments) {
        Constructor<?> selected = null;
        for (Constructor<?> constructor : owner.getDeclaredConstructors()) {
            if (constructor.getParameterTypes().length != arguments.length || !accepts(constructor.getParameterTypes(), arguments)) continue;
            if (selected != null) throw failure(owner, "<init>", new NoSuchMethodException("ambiguous target constructor"));
            selected = constructor;
        }
        if (selected == null) throw failure(owner, "<init>", new NoSuchMethodException("no applicable target constructor"));
        try {
            selected.setAccessible(true);
            return selected.newInstance(arguments);
        } catch (InvocationTargetException error) {
            Throwable cause = error.getCause();
            if (cause instanceof RuntimeException) throw (RuntimeException) cause;
            if (cause instanceof Error) throw (Error) cause;
            throw failure(owner, "<init>", cause);
        } catch (ReflectiveOperationException error) { throw failure(owner, "<init>", error); }
    }

    private Object call(Object instance, Class<?> owner, String name, Object[] args) {
        List<Method> matches = findMethods(owner, name, args.length, args, instance == null);
        if (matches.size() != 1) {
            throw failure(owner, name, new NoSuchMethodException("expected one applicable target method, found " + matches));
        }
        Method method = matches.get(0);
        try {
            method.setAccessible(true);
            return method.invoke(instance, args);
        } catch (InvocationTargetException error) {
            Throwable cause = error.getCause();
            if (cause instanceof RuntimeException) throw (RuntimeException) cause;
            if (cause instanceof Error) throw (Error) cause;
            throw failure(owner, name, cause);
        } catch (ReflectiveOperationException error) { throw failure(owner, name, error); }
    }

    private List<Method> findMethods(Class<?> owner, String name, int count, Object[] arguments, boolean requireStatic) {
        List<Method> matches = new ArrayList<Method>();
        Set<String> seenSignatures = new HashSet<String>();
        for (Class<?> current : hierarchy(owner)) {
            List<Member> mapped = methods.get(canonicalName(current) + "#" + name + "#" + count);
            for (Method method : current.getDeclaredMethods()) {
                if (method.getParameterTypes().length != count) continue;
                if (requireStatic && !Modifier.isStatic(method.getModifiers())) continue;
                if (method.isBridge() || method.isSynthetic()) continue;
                boolean named = mapped == null && method.getName().equals(name);
                if (mapped != null) {
                    for (Member row : mapped) {
                        if (!method.getName().equals(row.runtimeName)) continue;
                        boolean sameTypes = true;
                        for (int i = 0; i < count; i++) {
                            String actual = method.getParameterTypes()[i].getTypeName();
                            String canonical = row.parameters[i];
                            String expected = canonical.endsWith("[]")
                                    ? runtimeName(canonical.substring(0, canonical.length() - 2)) + "[]"
                                    : runtimeName(canonical);
                            if (!actual.equals(expected)) { sameTypes = false; break; }
                        }
                        if (sameTypes) { named = true; break; }
                    }
                }
                if (!named || (arguments != null && !accepts(method.getParameterTypes(), arguments))) continue;
                StringBuilder signature = new StringBuilder(name);
                for (Class<?> parameter : method.getParameterTypes()) signature.append('/').append(parameter.getName());
                // Overridden declarations are the same Java call, not an ambiguity.
                if (seenSignatures.add(signature.toString())) matches.add(method);
            }
        }
        return matches;
    }

    private static boolean accepts(Class<?>[] parameters, Object[] arguments) {
        for (int i = 0; i < parameters.length; i++) {
            if (arguments[i] == null) { if (parameters[i].isPrimitive()) return false; continue; }
            Class<?> expected = parameters[i];
            if (expected.isPrimitive()) {
                if (expected == boolean.class) expected = Boolean.class;
                else if (expected == byte.class) expected = Byte.class;
                else if (expected == short.class) expected = Short.class;
                else if (expected == int.class) expected = Integer.class;
                else if (expected == long.class) expected = Long.class;
                else if (expected == float.class) expected = Float.class;
                else if (expected == double.class) expected = Double.class;
                else if (expected == char.class) expected = Character.class;
            }
            if (!expected.isInstance(arguments[i])) return false;
        }
        return true;
    }

    private static List<Class<?>> hierarchy(Class<?> root) {
        List<Class<?>> result = new ArrayList<Class<?>>();
        hierarchy(root, result);
        return result;
    }

    private static void hierarchy(Class<?> type, List<Class<?>> result) {
        if (type == null || result.contains(type)) return;
        result.add(type);
        hierarchy(type.getSuperclass(), result);
        for (Class<?> implemented : type.getInterfaces()) hierarchy(implemented, result);
    }

    private IllegalStateException failure(Class<?> owner, String member, Throwable cause) {
        return new IllegalStateException("cannot access target " + canonicalName(owner) + "#" + member, cause);
    }
}
