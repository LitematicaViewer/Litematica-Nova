package net.litematicaba.mc26oracle;

import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.HexFormat;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;

final class VoxelShapeRecords {
    private static final Comparator<Rectangle> RECTANGLE_ORDER =
            Comparator.comparingDouble(Rectangle::minU)
                    .thenComparingDouble(Rectangle::minV)
                    .thenComparingDouble(Rectangle::maxU)
                    .thenComparingDouble(Rectangle::maxV);

    private VoxelShapeRecords() {}

    static Rectangle rectangle(double minU, double minV, double maxU, double maxV) {
        double normalizedMinU = coordinate(minU);
        double normalizedMinV = coordinate(minV);
        double normalizedMaxU = coordinate(maxU);
        double normalizedMaxV = coordinate(maxV);
        if (!(normalizedMinU < normalizedMaxU) || !(normalizedMinV < normalizedMaxV)) {
            throw new IllegalArgumentException(
                    "degenerate face rectangle: ["
                            + normalizedMinU
                            + ","
                            + normalizedMinV
                            + ","
                            + normalizedMaxU
                            + ","
                            + normalizedMaxV
                            + "]");
        }
        return new Rectangle(
                normalizedMinU, normalizedMinV, normalizedMaxU, normalizedMaxV);
    }

    static List<Rectangle> canonicalRectangles(List<Rectangle> input) {
        Objects.requireNonNull(input, "input");
        List<Rectangle> ordered = new ArrayList<>(input.size());
        for (Rectangle rectangle : input) {
            ordered.add(
                    VoxelShapeRecords.rectangle(
                            rectangle.minU(),
                            rectangle.minV(),
                            rectangle.maxU(),
                            rectangle.maxV()));
        }
        ordered.sort(RECTANGLE_ORDER);
        List<Rectangle> unique = new ArrayList<>(ordered.size());
        Rectangle previous = null;
        for (Rectangle rectangle : ordered) {
            if (!rectangle.equals(previous)) {
                unique.add(rectangle);
                previous = rectangle;
            }
        }
        return List.copyOf(unique);
    }

    static double coordinate(double value) {
        if (!Double.isFinite(value)) {
            throw new IllegalArgumentException("non-finite face coordinate: " + value);
        }
        return value == 0.0D ? 0.0D : value;
    }

    static final class Collector {
        private final Map<ShapeKey, String> idsByShape = new HashMap<>();
        private final Map<String, ShapeKey> shapesById = new HashMap<>();

        String add(String kind, List<Rectangle> rectangles) {
            ShapeKey shape = ShapeKey.create(kind, rectangles);
            String existing = idsByShape.get(shape);
            if (existing != null) {
                return existing;
            }
            String id = "sha256:" + sha256(CanonicalJsonl.json(shape.payload()));
            ShapeKey collision = shapesById.putIfAbsent(id, shape);
            if (collision != null && !collision.equals(shape)) {
                throw new IllegalStateException("SHA-256 collision for face shape " + id);
            }
            idsByShape.put(shape, id);
            return id;
        }

        List<Map<String, ?>> records() {
            List<Map<String, ?>> records = new ArrayList<>(shapesById.size());
            for (Map.Entry<String, ShapeKey> entry : shapesById.entrySet()) {
                Map<String, Object> record = new LinkedHashMap<>();
                record.put("kind", entry.getValue().kind());
                record.put("rectangles", entry.getValue().rectangleRecords());
                record.put("shape_id", entry.getKey());
                records.add(record);
            }
            return List.copyOf(records);
        }

        int size() {
            return shapesById.size();
        }
    }

    record Rectangle(double minU, double minV, double maxU, double maxV) {
        Map<String, Object> asRecord() {
            Map<String, Object> record = new LinkedHashMap<>();
            record.put("max_u", maxU);
            record.put("max_v", maxV);
            record.put("min_u", minU);
            record.put("min_v", minV);
            return record;
        }
    }

    private record ShapeKey(String kind, List<Rectangle> rectangles) {
        static ShapeKey create(String kind, List<Rectangle> rectangles) {
            if (!List.of("empty", "block", "rects").contains(kind)) {
                throw new IllegalArgumentException("unknown face shape kind: " + kind);
            }
            List<Rectangle> canonical = canonicalRectangles(rectangles);
            if (("empty".equals(kind) || "block".equals(kind)) && !canonical.isEmpty()) {
                throw new IllegalArgumentException(kind + " face shape cannot contain rectangles");
            }
            return new ShapeKey(kind, canonical);
        }

        Map<String, Object> payload() {
            Map<String, Object> payload = new LinkedHashMap<>();
            payload.put("kind", kind);
            payload.put("rectangles", rectangleRecords());
            return payload;
        }

        List<Map<String, Object>> rectangleRecords() {
            return rectangles.stream().map(Rectangle::asRecord).toList();
        }
    }

    private static String sha256(String value) {
        try {
            return HexFormat.of()
                    .formatHex(
                            MessageDigest.getInstance("SHA-256")
                                    .digest(value.getBytes(StandardCharsets.UTF_8)));
        } catch (NoSuchAlgorithmException error) {
            throw new IllegalStateException("SHA-256 is unavailable", error);
        }
    }
}
