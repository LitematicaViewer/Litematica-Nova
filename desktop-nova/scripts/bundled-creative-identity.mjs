import { createHash } from "node:crypto";
import { readFileSync, statSync } from "node:fs";
import { join } from "node:path";

// The package index is written by the game-data importer. Bind only the
// placement inputs used to derive the checked-in masks, not unrelated textures.
export function bundledCreativeIdentity(defaultRoot) {
  try {
    const masks = JSON.parse(readFileSync(new URL("../src/services/creativeInventoryBundled.json", import.meta.url), "utf8"));
    const manifest = JSON.parse(readFileSync(join(defaultRoot, "manifest.json"), "utf8"));
    if (manifest.schema_version !== 1 || manifest.source !== "bundled-default" ||
      manifest.game_version !== masks.baseVersion || !/^[0-9a-f]{64}$/.test(manifest.content_id ?? "")) return null;
    const index = JSON.parse(readFileSync(join(defaultRoot, "index.json"), "utf8"));
    const versions = Object.values(masks.versionsByMask).flat().filter(v => v !== masks.baseVersion);
    const prefix = "derived/version-adapters/";
    const storeIndex = index[prefix + "index.json"];
    let store;
    if (storeIndex) {
      const data = readFileSync(join(defaultRoot, prefix, "index.json"));
      if (data.length !== storeIndex.bytes || createHash("sha256").update(data).digest("hex") !== storeIndex.sha256) return null;
      store = JSON.parse(data);
      const archive = statSync(join(defaultRoot, prefix, "shared.zip"));
      if (![1, 2, 3].includes(store.schema_version) || store.format !== "nova.version-adapter-store" ||
          !archive.isFile() || archive.size !== index[prefix + "shared.zip"].bytes) return null;
    }
    const paths = ["derived/blockdb/blocks.dat.z", "derived/inventory.json",
      ...versions.map(v => `derived/version-adapters/${v}.zip`)].sort();
    const rows = paths.map(p => {
      if (store && p.startsWith(prefix)) {
        // The generator checked the source ZIP's entire logical content before
        // recording this provenance. The pin is independent of its new layout.
        const file = p.slice(prefix.length), pin = store.legacy_archives?.[file]?.sha256;
        if (!/^[0-9a-f]{64}$/.test(pin ?? "") || !store.versions[file.slice(0, -4)]) throw Error("invalid adapter pin");
        return p + ":" + pin;
      }
      const entry = index[p], file = statSync(join(defaultRoot, p));
      if (!file.isFile() || file.size !== entry.bytes || !/^[0-9a-f]{64}$/.test(entry.sha256)) throw Error("invalid index");
      return p + ":" + entry.sha256;
    });
    return createHash("sha256").update(rows.join("\n")).digest("hex") === masks.placementIndexSha256 ? manifest.content_id : null;
  } catch { return null; } // No matching pack: only use the real registry query.
}
