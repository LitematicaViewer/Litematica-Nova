package net.litematicaba.mc26oracle;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

final class RegistryOracleRecords {
    private RegistryOracleRecords() {}

    static List<Map<String, Object>> tagMembers(
            String registryId,
            String tagId,
            List<String> memberIds,
            List<String> definitionSourcePackIds) {
        requireId(registryId, "registryId");
        requireId(tagId, "tagId");
        List<String> sources = List.copyOf(definitionSourcePackIds);
        sources.forEach(source -> requireId(source, "definitionSourcePackId"));

        if (memberIds.isEmpty()) {
            Map<String, Object> record = new LinkedHashMap<>();
            record.put("definition_source_pack_ids", sources);
            record.put("member_id", null);
            record.put("member_index", -1);
            record.put("registry_id", registryId);
            record.put("tag_id", tagId);
            return List.of(record);
        }

        List<Map<String, Object>> records = new ArrayList<>(memberIds.size());
        for (int index = 0; index < memberIds.size(); index++) {
            String memberId = memberIds.get(index);
            requireId(memberId, "memberId");
            Map<String, Object> record = new LinkedHashMap<>();
            record.put("definition_source_pack_ids", sources);
            record.put("member_id", memberId);
            record.put("member_index", index);
            record.put("registry_id", registryId);
            record.put("tag_id", tagId);
            records.add(record);
        }
        return List.copyOf(records);
    }

    private static void requireId(String value, String name) {
        if (value == null || value.isBlank()) {
            throw new IllegalArgumentException(name + " must not be blank");
        }
    }
}
