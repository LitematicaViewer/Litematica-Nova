import catalog from "../../../data/minecraft-versions.json";

/** Minecraft DataVersion, not the Litematica format version or Nova version. */
export function minecraftVersionLabel(dataVersion: number): string {
  const match = catalog.versions.find((version) => version.dataVersion === dataVersion);
  return match ? `Minecraft ${match.version}` : "未知 Minecraft 版本";
}

export const minecraftVersions = catalog.versions;
