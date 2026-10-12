export type { BlockStateDb, I18nDb } from "../../services/blockstateDb";
export {
  getAllBlocks,
  getBlockProperties,
  getDefaultProperties,
  hasBlockDatabase,
  loadDatabases,
  translateKey,
  translateValue,
} from "../../services/blockstateDb";
export { getBlockIconDataUrl } from "../../services/blockIconResolver";
export {
  initI18n,
  translateBlockId,
  translateBuildingType,
} from "../../services/i18n";

