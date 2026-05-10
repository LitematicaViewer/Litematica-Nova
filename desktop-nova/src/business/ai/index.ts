export type {
  AiChatCompletionInput,
  AiChatCompletionOutput,
  AiChatMessageWire,
  AiPublicConfig,
  AiSaveConfigInput,
  AiTestResult,
} from "../../services/backend";
export {
  aiChatCompletion,
  aiClearKey,
  aiGetConfig,
  aiSaveConfig,
  aiTestConnection,
} from "../../services/backend";
export type {
  AiPlanMessage,
  AiPlanNormalizeResult,
  AiPlanSession,
  AiProjectionProvider,
  GenerationContext,
} from "../../services/aiProjection";
export {
  aiGeneratePlan,
  aiProjectionProviders,
  applyAiPlanToGenerateForm,
  buildWebPrompt,
  buildWrappedPrompt,
  createMockPlan,
  extractPlanFromAiText,
  getAiPlanWarnings,
  mockProjectionProvider,
  normalizeAiPlanForImport,
  normalizeAiPlanGeometry,
  parseAiPlanText,
  sendAiChatMessage,
  SUPPORTED_MATERIAL_TYPES,
  SUPPORTED_OPERATION_TYPES,
  validateAiPlanBasic,
} from "../../services/aiProjection";

