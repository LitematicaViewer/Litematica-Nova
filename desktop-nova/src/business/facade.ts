export * from "./actions";
export * from "./resourcePreflightUnits";
// Replace-group rules (units format, presets, session persistence).
export * from "./replace/actions";
export type * from "./replace/types";
export { openDialog, saveDialog } from "../platform/dialogs";
export * from "../services/aiProjection";
export * from "../services/backend";
export * from "../services/blockIconResolver";
export * from "../services/blockstateDb";
export * from "../services/generateService";
export * from "../services/generationTemplates";
export * from "../services/i18n";
export * from "../services/libraryStore";
export * from "../services/redenLibrary";
export * from "../services/renderMode";
export * from "../services/renderService";
// Read-only core results cached in the Rust process (shared across windows).
export * from "../services/coreReadCache";
export * from "../services/renderCacheStore";
// NOTE: `services/layerService.ts` is restored for parity with the pre-merge tree,
// but its slice/palette surface is superseded by `services/flakeService.ts`
// (same five type names). Only the live implementation is re-exported here.
export * from "../platform/applicationRender";
export * from "../services/statsService";
export * from "../services/userConfig";

export * from "../services/flakeService";
export * from "../services/containerService";
export * from "../services/enumeratorService";
// Game-resource registry (L1): consumers above already read through it; the
// L3 asset-manager UI binds to these APIs.
export * from "../services/gameResources";
export {
  openEnumeratorWindow,
} from "../platform/nativeViewer";
export { resolveFlakeLayerBlockImage, extractLayerPaletteStates } from "../services/flakeStateHintResolver";

export * from "../platform/quickPreview";
