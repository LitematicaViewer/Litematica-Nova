package net.litematicaba.mc26oracle;

import java.util.Map;
import java.util.Objects;
import java.util.StringJoiner;
import java.util.TreeMap;

public final class StateIds {
    private StateIds() {}

    public static String canonical(String ownerId, Map<String, String> properties) {
        if (ownerId == null || ownerId.isBlank()) {
            throw new IllegalArgumentException("ownerId must not be blank");
        }
        Objects.requireNonNull(properties, "properties");
        if (properties.isEmpty()) {
            return ownerId;
        }

        TreeMap<String, String> ordered = new TreeMap<>();
        for (Map.Entry<String, String> entry : properties.entrySet()) {
            if (entry.getKey() == null || entry.getKey().isBlank()) {
                throw new IllegalArgumentException("property name must not be blank");
            }
            if (entry.getValue() == null || entry.getValue().isBlank()) {
                throw new IllegalArgumentException("property value must not be blank");
            }
            ordered.put(entry.getKey(), entry.getValue());
        }

        StringJoiner values = new StringJoiner(",");
        ordered.forEach((name, value) -> values.add(name + "=" + value));
        return ownerId + "[" + values + "]";
    }
}
