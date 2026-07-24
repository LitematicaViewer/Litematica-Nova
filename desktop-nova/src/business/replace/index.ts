// Re-export services the ReplacePage needs directly, avoiding duplicate
// exports when used through facade.ts.
export type { PathInfo } from "../../services/backend";
export {
  checkFileExists,
  executeBackend,
  getUserConfigFilePath,
  writeUserConfigFile,
} from "../../services/backend";
export {
  getAllBlocks,
  getBlockProperties,
  translateKey,
  translateValue,
} from "../../services/blockstateDb";

// New v2 replace types and business logic.
export type {
  ReplaceEntry,
  ReplaceOutputEntry,
  ReplaceUnit,
  SerialSeparator,
  ReplaceItem,
  ReplacePreset,
  OutputDistEntry,
  UnitPreviewSummary,
  ReplacePreviewSummary,
} from "./types";
export { isSeparatorItem } from "./types";
export {
  dryRunReplaceUnits,
  applyReplaceUnits,
  listReplacePresets,
  saveReplacePreset,
  loadReplacePreset,
  deleteReplacePreset,
  openReplacePresetFolder,
} from "./actions";
