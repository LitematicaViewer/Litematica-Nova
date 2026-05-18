export type { BlockStateDb, I18nDb } from "../../services/blockstateDb";
export {
  getAllBlocks,
  getBlockProperties,
  getDefaultProperties,
  hasBlockDatabase,
  invalidateBlockstateDbCache,
  loadDatabases,
  translateKey,
  translateValue,
} from "../../services/blockstateDb";
export { getBlockIconDataUrl, invalidateBlockIconCache } from "../../services/blockIconResolver";
export {
  invalidateI18nCache,
  initI18n,
  translateBlockId,
  translateBuildingType,
} from "../../services/i18n";
export type {
  BlockIconSlot,
  GameResourceEntry,
  GameResourceHealthRow,
  GameResourceKind,
  GameResourceSnapshot,
} from "../../services/gameResources";
export {
  activateGameResource,
  checkGameResourceHealth,
  deleteGameResource,
  getActiveIconSearchRoots,
  getActiveResourceFingerprint,
  importGameDataResource,
  importLanguageResource,
  listGameResourceRegistry,
  readActiveGameDataResource,
  readActiveLanguageResource,
  registerExternalIconDirectory,
  registerGameResource,
} from "../../services/gameResources";

