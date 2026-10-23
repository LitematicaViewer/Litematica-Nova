import { t } from "./i18n";
import { aiChatCompletion, readWorkspaceFile } from "./backend";
import { getAllBlocks, getBlockProperties, hasBlockDatabase } from "./blockstateDb";
import { GenerateFormState, ProjectionPlan, normalizePlanBlockStates } from "./generateService";
import { templateToForm } from "./generationTemplates";

export interface GenerationContext {
  defaultSize: [number, number, number];
  minecraftDataVersion: number;
}

export interface AiProjectionProvider {
  id: string;
  name: string;
  generatePlan(prompt: string, context: GenerationContext): Promise<ProjectionPlan>;
}

export interface AiPlanMessage {
  role: "user" | "assistant" | "system";
  content: string;
  planSnapshot?: ProjectionPlan;
  timestamp?: number;
}

export interface AiPlanSession {
  id: string;
  messages: AiPlanMessage[];
  currentPlan?: ProjectionPlan;
  lastUserPrompt?: string;
  createdAt: number;
  updatedAt: number;
}

export interface AiPlanNormalizeResult {
  plan: ProjectionPlan;
  warnings: string[];
  errors: string[];
}

const OPERATION_TYPES = new Set([
  "fill_box",
  "hollow_box",
  "floor",
  "wall",
  "pillar",
  "cylinder",
  "sphere",
  "outline_box",
  "checkerboard_floor",
  "roof_gable",
  "ring",
]);

export const SUPPORTED_OPERATION_TYPES = Array.from(OPERATION_TYPES);
export const SUPPORTED_MATERIAL_TYPES = ["legacy block", "single", "weighted_random"];

const CHINESE_RE = /[\u3400-\u9fff]/;

export async function buildWebPrompt(userPrompt: string, currentPlan?: ProjectionPlan): Promise<string> {
  const promptConfig = await readWorkspaceFile("data/ai-projection/prompt_config.md");
  const firstRegion = currentPlan?.regions?.[0];
  const context = {
    minecraft_data_version: currentPlan?.minecraft_data_version || 3953,
    current_region_origin: firstRegion?.origin || [0, 0, 0],
    current_region_size: firstRegion?.size || [32, 16, 32],
    supported_operations: SUPPORTED_OPERATION_TYPES,
    supported_materials: SUPPORTED_MATERIAL_TYPES,
    current_plan: currentPlan || null,
    user_request: userPrompt,
    output_requirement: "Return one copyable text box/code block that contains only the complete projection_plan.json. Do not output natural language before or after it. Do not explain.",
  };
  return `${promptConfig.trim()}\n\n## Current Context\n\n${JSON.stringify(context, null, 2)}\n\nGenerate the next projection_plan.json now. Your entire visible answer must be a single copyable text box/code block containing only valid JSON. Do not write natural language.`;
}

export async function buildWrappedPrompt(userText: string, currentPlan?: ProjectionPlan): Promise<string> {
  return buildWebPrompt(userText, currentPlan);
}

export function extractPlanFromAiText(text: string): ProjectionPlan {
  try {
    return parseAiPlanText(text);
  } catch {
    const fenced = text.match(/```(?:json)?\s*([\s\S]*?)\s*```/i);
    if (fenced) return JSON.parse(fenced[1].trim());
    const extracted = extractFirstJsonObject(text);
    if (extracted) {
      return JSON.parse(extracted);
    }
    throw new Error(t("native.ai.noJson"));
  }
}

function extractFirstJsonObject(text: string): string | null {
  for (let start = text.indexOf("{"); start >= 0; start = text.indexOf("{", start + 1)) {
    let depth = 0;
    let inString = false;
    let escaped = false;
    for (let i = start; i < text.length; i += 1) {
      const ch = text[i];
      if (inString) {
        if (escaped) {
          escaped = false;
        } else if (ch === "\\") {
          escaped = true;
        } else if (ch === "\"") {
          inString = false;
        }
        continue;
      }
      if (ch === "\"") {
        inString = true;
      } else if (ch === "{") {
        depth += 1;
      } else if (ch === "}") {
        depth -= 1;
        if (depth === 0) return text.slice(start, i + 1);
      }
    }
  }
  return null;
}

export async function sendAiChatMessage(input: { messages: AiPlanMessage[] }): Promise<AiPlanMessage> {
  const response = await aiChatCompletion({
    messages: input.messages.map((message) => ({
      role: message.role,
      content: message.content,
    })),
  });
  return {
    role: "assistant",
    content: response.content,
    timestamp: Date.now(),
  };
}

export function parseAiPlanText(text: string): ProjectionPlan {
  const trimmedInput = text.trim();
  const fenced = trimmedInput.match(/```(?:json)?\s*([\s\S]*?)\s*```/i);
  const trimmed = (fenced ? fenced[1] : trimmedInput).trim();
  return JSON.parse(trimmed);
}

export function normalizeAiPlanForImport(plan: ProjectionPlan): AiPlanNormalizeResult {
  const blockNormalized = normalizePlanBlockStates(plan);
  const geometryNormalized = normalizeAiPlanGeometry(blockNormalized.plan);
  return {
    plan: geometryNormalized.plan,
    warnings: [...blockNormalized.warnings, ...geometryNormalized.warnings],
    errors: geometryNormalized.errors,
  };
}

export function normalizeAiPlanGeometry(plan: ProjectionPlan): AiPlanNormalizeResult {
  const normalized: ProjectionPlan = JSON.parse(JSON.stringify(plan));
  const warnings: string[] = [];
  const errors: string[] = [];

  for (const [regionIndex, region] of (normalized.regions || []).entries()) {
    if (!Array.isArray(region.size) || region.size.length !== 3) continue;
    const size = region.size.map((value) => Math.max(1, Math.floor(Number(value)))) as [number, number, number];
    region.size = size;
    const max = [size[0] - 1, size[1] - 1, size[2] - 1] as [number, number, number];
    for (const [operationIndex, operation] of (region.operations || []).entries()) {
      const path = `regions[${regionIndex}].operations[${operationIndex}]`;
      normalizeFromTo(operation, max, path, warnings);
      normalizeCenterRadiusHeight(operation, size, max, path, warnings, errors);
    }
  }

  return { plan: normalized, warnings, errors };
}

function normalizeFromTo(operation: any, max: [number, number, number], path: string, warnings: string[]) {
  if (!["fill_box", "hollow_box", "floor", "wall", "outline_box", "checkerboard_floor", "roof_gable"].includes(operation?.type)) return;
  if (!Array.isArray(operation.from) || !Array.isArray(operation.to) || operation.from.length !== 3 || operation.to.length !== 3) return;
  const oldFrom = [...operation.from];
  const oldTo = [...operation.to];
  const nextFrom: number[] = [];
  const nextTo: number[] = [];
  for (let axis = 0; axis < 3; axis += 1) {
    const a = clampInt(operation.from[axis], 0, max[axis]);
    const b = clampInt(operation.to[axis], 0, max[axis]);
    nextFrom[axis] = Math.min(a, b);
    nextTo[axis] = Math.max(a, b);
  }
  operation.from = nextFrom;
  operation.to = nextTo;
  if (!sameArray(oldFrom, nextFrom) || !sameArray(oldTo, nextTo)) {
    warnings.push(t("native.ai.boundsSorted", { path }));
  }
}

function normalizeCenterRadiusHeight(operation: any, size: [number, number, number], max: [number, number, number], path: string, warnings: string[], errors: string[]) {
  if (!["sphere", "ring", "cylinder"].includes(operation?.type)) return;
  if (!Array.isArray(operation.center) || operation.center.length !== 3) {
    errors.push(t("native.plan.centerVector", { path: path }));
    return;
  }
  const oldCenter = [...operation.center];
  operation.center = operation.center.map((value: unknown, axis: number) => clampInt(value, 0, max[axis]));
  if (!sameArray(oldCenter, operation.center)) {
    warnings.push(t("native.ai.centerAdjusted", { path, before: oldCenter.join(","), after: operation.center.join(",") }));
  }

  const oldRadius = operation.radius;
  let radius = Math.max(1, Math.floor(Number(operation.radius) || 1));
  const axes = operation.type === "cylinder" ? [0, 2] : [0, 1, 2];
  for (const axis of axes) {
    if (max[axis] < 2) {
      errors.push(t("native.plan.radiusNoRoom", { path: path, axis: axis }));
      continue;
    }
    const axisMaxRadius = Math.max(1, Math.floor(max[axis] / 2));
    if (radius > axisMaxRadius) radius = axisMaxRadius;
  }

  for (const axis of axes) {
    const before = operation.center[axis];
    operation.center[axis] = clampInt(operation.center[axis], radius, max[axis] - radius);
    if (before !== operation.center[axis]) {
      const label = ["x", "y", "z"][axis];
      warnings.push(t("native.ai.axisAdjusted", { path, axis: label, before, after: operation.center[axis] }));
    }
  }
  operation.radius = Math.max(1, radius);
  if (Number(oldRadius) !== operation.radius) {
    warnings.push(t("native.ai.radiusAdjusted", { path, before: oldRadius, after: operation.radius }));
  }

  if (operation.type === "ring" || operation.type === "cylinder") {
    const oldHeight = operation.height;
    const maxHeight = Math.max(1, size[1] - operation.center[1]);
    operation.height = clampInt(operation.height ?? 1, 1, maxHeight);
    if (Number(oldHeight ?? 1) !== operation.height) {
      warnings.push(t("native.ai.heightAdjusted", { path, before: oldHeight ?? 1, after: operation.height }));
    }
  }
}

function clampInt(value: unknown, min: number, max: number): number {
  const numeric = Math.floor(Number(value));
  if (!Number.isFinite(numeric)) return min;
  return Math.min(max, Math.max(min, numeric));
}

function sameArray(a: any[], b: any[]): boolean {
  return a.length === b.length && a.every((value, index) => Number(value) === Number(b[index]));
}

export function validateAiPlanBasic(plan: any): string[] {
  const errors: string[] = [];
  if (!plan || typeof plan !== "object") return [t("native.plan.object")];
  if (plan.version !== 1) errors.push(t("native.plan.version"));
  if (!Array.isArray(plan.regions) || plan.regions.length === 0) errors.push(t("native.plan.regionArray"));

  for (const [regionIndex, region] of (plan.regions || []).entries()) {
    const regionPath = `regions[${regionIndex}]`;
    if (!Array.isArray(region.size) || region.size.length !== 3) {
      errors.push(t("native.plan.sizeVector", { path: regionPath }));
    } else {
      region.size.forEach((value: unknown, axis: number) => {
        if (!Number.isFinite(value) || Number(value) <= 0) errors.push(t("native.plan.sizePositive", { path: regionPath, axis: axis }));
      });
    }
    if (!Array.isArray(region.operations)) {
      errors.push(t("native.plan.operationsArray", { path: regionPath }));
      continue;
    }
    for (const [operationIndex, operation] of region.operations.entries()) {
      validateOperation(operation, `${regionPath}.operations[${operationIndex}]`, errors);
    }
  }
  return errors;
}

export function getAiPlanWarnings(plan: any): string[] {
  const warnings: string[] = [];
  if (!plan || typeof plan !== "object" || !Array.isArray(plan.regions)) return warnings;

  let operationCount = 0;
  let totalVolume = 0;
  const knownBlocks = new Set(getAllBlocks());
  const dbAvailable = hasBlockDatabase();
  for (const [regionIndex, region] of plan.regions.entries()) {
    operationCount += Array.isArray(region.operations) ? region.operations.length : 0;
    if (Array.isArray(region.size) && region.size.length === 3) {
      const size = region.size.map((value: unknown) => Number(value));
      if (size.every((value: number) => Number.isFinite(value) && value > 0)) {
        size.forEach((value: number, axis: number) => {
          if (value > 128) warnings.push(t("native.ai.recommendedSize", { index: regionIndex, axis }));
        });
        totalVolume += size[0] * size[1] * size[2];
      }
    }
    for (const [operationIndex, operation] of (Array.isArray(region.operations) ? region.operations : []).entries()) {
      collectOperationWarnings(operation, `regions[${regionIndex}].operations[${operationIndex}]`, warnings, knownBlocks, dbAvailable);
    }
  }
  if (operationCount > 128) warnings.push(t("native.ai.recommendedOperations", { count: operationCount }));
  if (totalVolume > 1_000_000) warnings.push(t("native.ai.recommendedVolume", { volume: totalVolume }));
  return warnings;
}

export function applyAiPlanToGenerateForm(plan: ProjectionPlan): GenerateFormState {
  const normalized = normalizeAiPlanForImport(plan).plan;
  return templateToForm({
    id: "ai-imported",
    name_zh: normalized.metadata?.name || t("native.ai.importName"),
    description_zh: normalized.metadata?.description || t("native.ai.importDescription"),
    tags: ["ai"],
    plan: normalized,
  });
}

export async function createMockPlan(prompt: string, context: GenerationContext = { defaultSize: [24, 8, 24], minecraftDataVersion: 3953 }): Promise<ProjectionPlan> {
  const [sx, _sy, sz] = context.defaultSize;
  return {
    version: 1,
    metadata: {
      name: prompt.trim() ? `Mock - ${prompt.trim().slice(0, 24)}` : t("native.ai.mockName"),
      author: "Litematica-Nova",
      description: t("native.ai.mockDescription"),
    },
    minecraft_data_version: context.minecraftDataVersion,
    regions: [
      {
        name: "main",
        origin: [0, 0, 0],
        size: context.defaultSize,
        operations: [
          {
            type: "floor",
            from: [0, 0, 0],
            to: [Math.max(0, sx - 1), 0, Math.max(0, sz - 1)],
            block: { name: "minecraft:stone_bricks", properties: {} },
          },
        ],
      },
    ],
  };
}

export async function aiGeneratePlan(request: { provider: string; prompt: string; currentPlan?: ProjectionPlan; context: GenerationContext }): Promise<ProjectionPlan> {
  if (request.provider !== "mock") {
    throw new Error(t("native.ai.notWired"));
  }
  const plan = await createMockPlan(request.prompt, request.context);
  const normalized = normalizePlanBlockStates(plan).plan;
  const errors = validateAiPlanBasic(normalized);
  if (errors.length) throw new Error(errors.join("\n"));
  return normalized;
}

export const mockProjectionProvider: AiProjectionProvider = {
  id: "mock",
  get name() { return t("native.ai.mockProvider"); },
  async generatePlan(prompt: string, context: GenerationContext): Promise<ProjectionPlan> {
    return createMockPlan(prompt, context);
  },
};

export const aiProjectionProviders: AiProjectionProvider[] = [
  mockProjectionProvider,
  {
    id: "openai-compatible-disabled",
    get name() { return t("native.ai.openAiDisabled"); },
    async generatePlan() {
      throw new Error(t("native.ai.openAiUnavailable"));
    },
  },
  {
    id: "gemini-compatible-disabled",
    get name() { return t("native.ai.geminiDisabled"); },
    async generatePlan() {
      throw new Error(t("native.ai.geminiUnavailable"));
    },
  },
];

function validateOperation(operation: any, path: string, errors: string[]) {
  if (!OPERATION_TYPES.has(operation?.type)) {
    errors.push(t("native.plan.operationUnsupported", { path: path, type: String(operation?.type) }));
    return;
  }
  if (operation.type === "checkerboard_floor") {
    validateMaterial(operation.material_a, `${path}.material_a`, errors, true);
    validateMaterial(operation.material_b, `${path}.material_b`, errors, true);
    return;
  }
  if (operation.material) {
    validateMaterial(operation.material, `${path}.material`, errors, true);
    return;
  }
  validateBlock(operation.block, `${path}.block`, errors, true);
}

function validateMaterial(material: any, path: string, errors: string[], required: boolean) {
  if (!material) {
    if (required) errors.push(t("native.plan.required", { path: path }));
    return;
  }
  if (material.name) {
    validateBlock(material, path, errors, true);
    return;
  }
  if (material.type === "single") {
    validateBlock(material.block, `${path}.block`, errors, true);
  } else if (material.type === "weighted_random") {
    if (!Array.isArray(material.entries) || material.entries.length === 0) errors.push(t("native.plan.entriesEmpty", { path: path }));
    for (const [index, entry] of (material.entries || []).entries()) {
      if (!Number.isFinite(entry.weight) || Number(entry.weight) <= 0) errors.push(t("native.plan.weightPositive", { path: path, index: index }));
      validateBlock(entry.block, `${path}.entries[${index}].block`, errors, true);
    }
  } else {
    errors.push(t("native.plan.typeUnsupported", { path: path }));
  }
}

function validateBlock(block: any, path: string, errors: string[], required: boolean) {
  if (!block) {
    if (required) errors.push(t("native.plan.required", { path: path }));
    return;
  }
  if (typeof block.name !== "string") {
    errors.push(t("native.plan.nameString", { path: path }));
  } else if (!block.name.trim()) {
    errors.push(t("native.plan.nameEmpty", { path: path }));
  } else if (!block.name.startsWith("minecraft:")) {
    errors.push(t("native.plan.nameNamespace", { path: path }));
  }
  if (typeof block.name === "string" && CHINESE_RE.test(block.name)) {
    errors.push(t("native.plan.nameChinese", { path: path }));
  }
  const properties = block.properties || {};
  if (typeof properties !== "object" || Array.isArray(properties)) {
    errors.push(t("native.plan.propertiesObject", { path: path }));
    return;
  }
  for (const [key, value] of Object.entries(properties)) {
    if (typeof key !== "string" || typeof value !== "string") errors.push(t("native.plan.propertiesStrings", { path: path }));
    if (CHINESE_RE.test(key) || CHINESE_RE.test(String(value))) errors.push(t("native.plan.propertiesChinese", { path: path }));
  }
}

function collectOperationWarnings(operation: any, path: string, warnings: string[], knownBlocks: Set<string>, dbAvailable: boolean) {
  if (operation?.type === "checkerboard_floor") {
    collectMaterialWarnings(operation.material_a, `${path}.material_a`, warnings, knownBlocks, dbAvailable);
    collectMaterialWarnings(operation.material_b, `${path}.material_b`, warnings, knownBlocks, dbAvailable);
    return;
  }
  if (operation?.material) {
    collectMaterialWarnings(operation.material, `${path}.material`, warnings, knownBlocks, dbAvailable);
    return;
  }
  collectBlockWarnings(operation?.block, `${path}.block`, warnings, knownBlocks, dbAvailable);
}

function collectMaterialWarnings(material: any, path: string, warnings: string[], knownBlocks: Set<string>, dbAvailable: boolean) {
  if (!material) return;
  if (material.name) {
    collectBlockWarnings(material, path, warnings, knownBlocks, dbAvailable);
    return;
  }
  if (material.type === "single") {
    collectBlockWarnings(material.block, `${path}.block`, warnings, knownBlocks, dbAvailable);
    return;
  }
  if (material.type === "weighted_random") {
    for (const [index, entry] of (material.entries || []).entries()) {
      collectBlockWarnings(entry.block, `${path}.entries[${index}].block`, warnings, knownBlocks, dbAvailable);
    }
  }
}

function collectBlockWarnings(block: any, path: string, warnings: string[], knownBlocks: Set<string>, dbAvailable: boolean) {
  if (!block || typeof block.name !== "string" || !block.name.startsWith("minecraft:")) return;
  if (!dbAvailable) return;
  if (!knownBlocks.has(block.name)) {
    warnings.push(t("native.ai.unknownBlock", { path, block: block.name }));
    return;
  }
  const knownProperties = getBlockProperties(block.name);
  for (const [key, value] of Object.entries(block.properties || {})) {
    if (!Object.prototype.hasOwnProperty.call(knownProperties, key)) {
      warnings.push(t("native.ai.unknownProperty", { path, property: key, block: block.name }));
      continue;
    }
    if (!knownProperties[key].includes(String(value))) {
      warnings.push(t("native.ai.unknownValue", { path, property: key, value: String(value), block: block.name }));
    }
  }
}
