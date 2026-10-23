package net.litematicaba.mc26oracle;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.lang.reflect.Array;
import java.math.BigDecimal;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashSet;
import java.util.HexFormat;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.TreeMap;

public final class CanonicalJsonl {
    private CanonicalJsonl() {}

    public record Result(int recordCount, String recordsSha256) {}

    public static Result write(
            Path output, List<? extends Map<String, ?>> records, List<String> keyFields)
            throws IOException {
        Objects.requireNonNull(output, "output");
        Objects.requireNonNull(records, "records");
        if (keyFields == null || keyFields.isEmpty()) {
            throw new IllegalArgumentException("keyFields must not be empty");
        }

        List<Map<String, Object>> ordered = new ArrayList<>(records.size());
        Set<String> uniqueKeys = new HashSet<>();
        for (Map<String, ?> record : records) {
            Map<String, Object> canonical = canonicalMap(Objects.requireNonNull(record, "record"));
            String signature = keySignature(canonical, keyFields);
            if (!uniqueKeys.add(signature)) {
                throw new IllegalArgumentException("duplicate JSONL key: " + signature);
            }
            ordered.add(canonical);
        }
        ordered.sort(recordComparator(keyFields));

        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        for (Map<String, Object> record : ordered) {
            bytes.write(json(record).getBytes(StandardCharsets.UTF_8));
            bytes.write('\n');
        }
        byte[] payload = bytes.toByteArray();
        writeAtomically(output, payload);
        return new Result(ordered.size(), sha256(payload));
    }

    public static String json(Object value) {
        StringBuilder output = new StringBuilder();
        appendJson(output, canonical(value));
        return output.toString();
    }

    private static Comparator<Map<String, Object>> recordComparator(List<String> keyFields) {
        return (left, right) -> {
            for (String field : keyFields) {
                int comparison = compareKeyValue(requiredKey(left, field), requiredKey(right, field));
                if (comparison != 0) {
                    return comparison;
                }
            }
            return 0;
        };
    }

    private static int compareKeyValue(Object left, Object right) {
        if (left instanceof Number leftNumber && right instanceof Number rightNumber) {
            return number(leftNumber).compareTo(number(rightNumber));
        }
        if (left instanceof Boolean leftBoolean && right instanceof Boolean rightBoolean) {
            return Boolean.compare(leftBoolean, rightBoolean);
        }
        if (left instanceof String leftString && right instanceof String rightString) {
            return leftString.compareTo(rightString);
        }
        return json(left).compareTo(json(right));
    }

    private static String keySignature(Map<String, Object> record, List<String> keyFields) {
        StringBuilder signature = new StringBuilder();
        for (String field : keyFields) {
            String value = json(requiredKey(record, field));
            signature.append(field.length()).append(':').append(field).append('=');
            signature.append(value.length()).append(':').append(value).append(';');
        }
        return signature.toString();
    }

    private static Object requiredKey(Map<String, Object> record, String field) {
        if (!record.containsKey(field) || record.get(field) == null) {
            throw new IllegalArgumentException("missing JSONL key field: " + field);
        }
        return record.get(field);
    }

    private static Object canonical(Object value) {
        if (value == null || value instanceof String || value instanceof Boolean) {
            return value;
        }
        if (value instanceof Number number) {
            number(number);
            return number;
        }
        if (value instanceof Map<?, ?> map) {
            TreeMap<String, Object> result = new TreeMap<>();
            for (Map.Entry<?, ?> entry : map.entrySet()) {
                if (!(entry.getKey() instanceof String key)) {
                    throw new IllegalArgumentException("JSON object keys must be strings");
                }
                result.put(key, canonical(entry.getValue()));
            }
            return result;
        }
        if (value instanceof Iterable<?> iterable) {
            List<Object> result = new ArrayList<>();
            iterable.forEach(item -> result.add(canonical(item)));
            return result;
        }
        if (value.getClass().isArray()) {
            List<Object> result = new ArrayList<>(Array.getLength(value));
            for (int index = 0; index < Array.getLength(value); index++) {
                result.add(canonical(Array.get(value, index)));
            }
            return result;
        }
        throw new IllegalArgumentException("unsupported JSON value: " + value.getClass().getName());
    }

    private static Map<String, Object> canonicalMap(Map<String, ?> value) {
        Object canonical = canonical(value);
        @SuppressWarnings("unchecked")
        Map<String, Object> result = (Map<String, Object>) canonical;
        return result;
    }

    private static void appendJson(StringBuilder output, Object value) {
        if (value == null) {
            output.append("null");
        } else if (value instanceof String string) {
            appendString(output, string);
        } else if (value instanceof Boolean bool) {
            output.append(bool);
        } else if (value instanceof Number number) {
            output.append(number(number).toPlainString());
        } else if (value instanceof Map<?, ?> map) {
            output.append('{');
            boolean first = true;
            for (Map.Entry<?, ?> entry : map.entrySet()) {
                if (!first) {
                    output.append(',');
                }
                first = false;
                appendString(output, (String) entry.getKey());
                output.append(':');
                appendJson(output, entry.getValue());
            }
            output.append('}');
        } else if (value instanceof List<?> list) {
            output.append('[');
            for (int index = 0; index < list.size(); index++) {
                if (index > 0) {
                    output.append(',');
                }
                appendJson(output, list.get(index));
            }
            output.append(']');
        } else {
            throw new IllegalArgumentException("value was not canonicalized: " + value);
        }
    }

    private static void appendString(StringBuilder output, String value) {
        output.append('"');
        for (int index = 0; index < value.length(); index++) {
            char character = value.charAt(index);
            switch (character) {
                case '"' -> output.append("\\\"");
                case '\\' -> output.append("\\\\");
                case '\b' -> output.append("\\b");
                case '\f' -> output.append("\\f");
                case '\n' -> output.append("\\n");
                case '\r' -> output.append("\\r");
                case '\t' -> output.append("\\t");
                default -> {
                    if (character < 0x20) {
                        output.append(String.format("\\u%04x", (int) character));
                    } else {
                        output.append(character);
                    }
                }
            }
        }
        output.append('"');
    }

    private static BigDecimal number(Number value) {
        if (value instanceof Double doubleValue && !Double.isFinite(doubleValue)) {
            throw new IllegalArgumentException("non-finite JSON number: " + value);
        }
        if (value instanceof Float floatValue && !Float.isFinite(floatValue)) {
            throw new IllegalArgumentException("non-finite JSON number: " + value);
        }
        try {
            return new BigDecimal(value.toString());
        } catch (NumberFormatException error) {
            throw new IllegalArgumentException("invalid JSON number: " + value, error);
        }
    }

    private static void writeAtomically(Path output, byte[] payload) throws IOException {
        Path absolute = output.toAbsolutePath().normalize();
        Path parent = absolute.getParent();
        if (parent == null) {
            throw new IOException("output has no parent: " + output);
        }
        Files.createDirectories(parent);
        Path temporary = Files.createTempFile(parent, absolute.getFileName().toString(), ".tmp");
        boolean moved = false;
        try {
            Files.write(temporary, payload);
            try {
                Files.move(
                        temporary,
                        absolute,
                        StandardCopyOption.ATOMIC_MOVE,
                        StandardCopyOption.REPLACE_EXISTING);
            } catch (AtomicMoveNotSupportedException ignored) {
                Files.move(temporary, absolute, StandardCopyOption.REPLACE_EXISTING);
            }
            moved = true;
        } finally {
            if (!moved) {
                Files.deleteIfExists(temporary);
            }
        }
    }

    private static String sha256(byte[] payload) {
        try {
            return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(payload));
        } catch (NoSuchAlgorithmException error) {
            throw new IllegalStateException("SHA-256 is unavailable", error);
        }
    }

    static String sha256(Path input) throws IOException {
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            try (InputStream stream = Files.newInputStream(input)) {
                byte[] buffer = new byte[64 * 1024];
                int read;
                while ((read = stream.read(buffer)) != -1) {
                    digest.update(buffer, 0, read);
                }
            }
            return HexFormat.of().formatHex(digest.digest());
        } catch (NoSuchAlgorithmException error) {
            throw new IllegalStateException("SHA-256 is unavailable", error);
        }
    }
}
