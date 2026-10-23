import { t } from "./i18n";
import { generateProject } from "../platform/applicationOperations";
import type { ProjectAnalysis } from "../platform/applicationRender";
import { getPathInfo, PathInfo } from "./backend";
import { getDefaultProperties } from "./blockstateDb";

const PLAN_OPERATION_TYPES = new Set([
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
const CHINESE_RE = /[\u3400-\u9fff]/;

export interface BlockStateSpec {
  name: string;
  properties?: Record<string, string>;
}

export interface MaterialEntrySpec {
  id: string;
  weight: number;
  block: BlockStateSpec;
}

export type MaterialSpec =
  | { type: "single"; block: BlockStateSpec }
  | { type: "weighted_random"; seed: number; entries: MaterialEntrySpec[] };

export type OperationType =
  | "fill_box"
  | "hollow_box"
  | "floor"
  | "wall"
  | "pillar"
  | "cylinder"
  | "sphere"
  | "outline_box"
  | "checkerboard_floor"
  | "roof_gable"
  | "ring";

export interface OperationFormState {
  id: string;
  type: OperationType;
  from: [number, number, number];
  to: [number, number, number];
  base: [number, number, number];
  center: [number, number, number];
  height: number;
  radius: number;
  thickness: number;
  filled: boolean;
  block: BlockStateSpec;
  material?: MaterialSpec;
  materialA?: MaterialSpec;
  materialB?: MaterialSpec;
  axis: "x" | "z";
  overhang: number;
}

export interface GenerateFormState {
  metadata: {
    name: string;
    author: string;
    description: string;
    minecraftDataVersion: number;
  };
  region: {
    name: string;
    origin: [number, number, number];
    size: [number, number, number];
  };
  operations: OperationFormState[];
}

export interface ProjectionPlan {
  version: 1;
  metadata: {
    name: string;
    author: string;
    description: string;
  };
  minecraft_data_version: number;
  regions: Array<{
    name: string;
    origin: [number, number, number];
    size: [number, number, number];
    operations: any[];
  }>;
}

export interface GenerateSummary {
  dry_run: boolean;
  regions_count: number;
  operation_count?: number;
  total_volume: number;
  estimated_non_air_blocks: number;
  estimated_palette_count?: number;
  palette_entries_per_region: Array<{ region: string; palette_entries: number }>;
  operations?: Array<{
    region: string;
    index: number;
    name: string | null;
    operation_type: string;
    affected_block_count: number;
    material_summary: Array<{ block_id: string; properties: Record<string, string>; count: number }>;
  }>;
  material_summary: Array<{ block_id: string; properties: Record<string, string>; count: number }>;
  warnings: string[];
  errors: string[];
  wrote_file: boolean;
  output_size: number | null;
  verify_load_result: string | null;
}

export interface GenerateResult {
  summary: GenerateSummary;
  analysis: ProjectAnalysis | null;
  normalizedOutputPath?: string;
  diagnostics: string;
}

export interface PlanNormalizationResult {
  plan: ProjectionPlan;
  warnings: string[];
}

export interface OutputPathValidation {
  ok: boolean;
  error?: string;
  normalizedPath: string;
  parentDir: string;
  parentExists: boolean;
  outputHasLitematicExt: boolean;
  outputIsAbsolute: boolean;
  outputExists: boolean;
  outputIsDir: boolean;
  rawInfo: PathInfo;
  normalizedInfo?: PathInfo;
}

export function createDefaultOperation(type: OperationType = "hollow_box"): OperationFormState {
  return {
    id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
    type,
    from: [0, 0, 0],
    to: [11, 7, 11],
    base: [0, 0, 0],
    center: [6, 0, 6],
    height: 8,
    radius: 3,
    thickness: 1,
    filled: true,
    block: { name: "minecraft:stone_bricks", properties: {} },
    material: undefined,
    materialA: { type: "single", block: { name: "minecraft:white_concrete", properties: {} } },
    materialB: { type: "single", block: { name: "minecraft:black_concrete", properties: {} } },
    axis: "x",
    overhang: 0,
  };
}

export function buildProjectionPlan(formState: GenerateFormState): ProjectionPlan {
  const operations = formState.operations.map((operation) => {
    const materialPatch = buildMaterialPatch(operation);
    switch (operation.type) {
      case "fill_box":
        return { type: operation.type, from: operation.from, to: operation.to, ...materialPatch };
      case "hollow_box":
        return { type: operation.type, from: operation.from, to: operation.to, thickness: operation.thickness, ...materialPatch };
      case "floor":
        return { type: operation.type, from: operation.from, to: operation.to, ...materialPatch };
      case "wall":
        return { type: operation.type, from: operation.from, to: operation.to, thickness: operation.thickness, ...materialPatch };
      case "pillar":
        return { type: operation.type, base: operation.base, height: operation.height, ...materialPatch };
      case "cylinder":
        return { type: operation.type, center: operation.center, radius: operation.radius, height: operation.height, filled: operation.filled, ...materialPatch };
      case "sphere":
        return { type: operation.type, center: operation.center, radius: operation.radius, filled: operation.filled, ...materialPatch };
      case "outline_box":
        return { type: operation.type, from: operation.from, to: operation.to, ...materialPatch };
      case "checkerboard_floor":
        return {
          type: operation.type,
          from: operation.from,
          to: operation.to,
          material_a: normalizeMaterial(operation.materialA || { type: "single", block: { name: "minecraft:white_concrete", properties: {} } }),
          material_b: normalizeMaterial(operation.materialB || { type: "single", block: { name: "minecraft:black_concrete", properties: {} } }),
        };
      case "roof_gable":
        return { type: operation.type, from: operation.from, to: operation.to, axis: operation.axis || "x", overhang: operation.overhang || 0, ...materialPatch };
      case "ring":
        return { type: operation.type, center: operation.center, radius: operation.radius, height: operation.height, thickness: operation.thickness, ...materialPatch };
      default:
        return { type: operation.type, ...materialPatch };
    }
  });

  return {
    version: 1,
    metadata: {
      name: formState.metadata.name || t("native.plan.defaultName"),
      author: formState.metadata.author || "Litematica-Nova",
      description: formState.metadata.description || t("native.plan.defaultDescription"),
    },
    minecraft_data_version: Number(formState.metadata.minecraftDataVersion) || 3953,
    regions: [
      {
        name: formState.region.name || "main",
        origin: formState.region.origin,
        size: formState.region.size,
        operations,
      },
    ],
  };
}

export function validatePlanClientSide(plan: ProjectionPlan): string[] {
  const errors: string[] = [];
  if (plan.version !== 1) errors.push(t("native.plan.version"));
  if (!plan.regions.length) errors.push(t("native.plan.regions"));
  plan.regions.forEach((region, regionIndex) => {
    const regionPath = `regions[${regionIndex}]`;
    if (!region.name.trim()) errors.push(t("native.plan.nameEmpty", { path: regionPath }));
    region.size.forEach((value, axis) => {
      if (!Number.isFinite(value) || value <= 0) errors.push(t("native.plan.sizePositive", { path: regionPath, axis: axis }));
    });
    region.operations.forEach((operation, operationIndex) => {
      const opPath = `${regionPath}.operations[${operationIndex}]`;
      if (!operation.type) errors.push(t("native.plan.typeEmpty", { path: opPath }));
      else if (!PLAN_OPERATION_TYPES.has(operation.type)) errors.push(t("native.plan.operationUnsupported", { path: opPath, type: operation.type }));
      validateOperationMaterial(operation, opPath, errors);
      validateOperationBounds(operation, region.size, opPath, errors);
    });
  });
  return errors;
}

export async function dryRunGenerate(plan: ProjectionPlan): Promise<GenerateResult> {
  const result = await generateProject<GenerateSummary, ProjectionPlan>({
    plan,
    outputPath: null,
    dryRun: true,
  });
  return {
    summary: result.summary,
    analysis: result.analysis,
    normalizedOutputPath: result.outputPath || undefined,
    diagnostics: result.diagnostics,
  };
}

export async function applyGenerate(plan: ProjectionPlan, outputPath: string): Promise<GenerateResult> {
  const validation = await validateOutputPath(outputPath);
  if (!validation.ok) throw new Error(validation.error || t("native.plan.outputInvalid"));

  const result = await generateProject<GenerateSummary, ProjectionPlan>({
    plan,
    outputPath: validation.normalizedPath,
    dryRun: false,
  });
  return {
    summary: result.summary,
    analysis: result.analysis,
    normalizedOutputPath: result.outputPath || validation.normalizedPath,
    diagnostics: result.diagnostics,
  };
}

export function defaultOutputName(name: string): string {
  return `${sanitizeFileName(name || t("native.plan.defaultName"))}.litematic`;
}

export function sanitizeFileName(name: string): string {
  return (name || t("native.plan.defaultName")).replace(/[<>:"/\\|?*]/g, "_").trim() || t("native.plan.defaultName");
}

export function ensureLitematicExtension(path: string): string {
  const trimmed = path.trim();
  return trimmed.toLowerCase().endsWith(".litematic") ? trimmed : `${trimmed}.litematic`;
}

export async function validateOutputPath(rawPath: string): Promise<OutputPathValidation> {
  if (!rawPath.trim()) {
    const rawInfo = await getPathInfo("");
    return validationFromInfo(false, t("native.plan.outputEmpty"), "", rawInfo, rawInfo);
  }

  const rawInfo = await getPathInfo(rawPath.trim());
  if (rawInfo.is_dir) {
    return validationFromInfo(false, t("native.plan.outputIsDir"), rawInfo.normalized, rawInfo, rawInfo);
  }

  const normalizedCandidate = ensureLitematicExtension(rawInfo.normalized);
  const normalizedInfo = await getPathInfo(normalizedCandidate);
  if (!normalizedInfo.parent_exists) {
    return validationFromInfo(false, t("native.plan.outputMissing", { path: normalizedInfo.parent_dir }), normalizedCandidate, rawInfo, normalizedInfo);
  }
  if (normalizedInfo.is_dir) {
    return validationFromInfo(false, t("native.plan.outputIsDir"), normalizedCandidate, rawInfo, normalizedInfo);
  }
  if (normalizedInfo.exists) {
    return validationFromInfo(false, t("native.plan.outputExists"), normalizedCandidate, rawInfo, normalizedInfo);
  }
  if (!normalizedInfo.is_absolute) {
    return validationFromInfo(false, t("native.plan.outputAbsolute"), normalizedCandidate, rawInfo, normalizedInfo);
  }
  if (!normalizedInfo.has_litematic_ext) {
    return validationFromInfo(false, t("native.plan.outputExtension"), normalizedCandidate, rawInfo, normalizedInfo);
  }

  return validationFromInfo(true, undefined, normalizedCandidate, rawInfo, normalizedInfo);
}

function validationFromInfo(
  ok: boolean,
  error: string | undefined,
  normalizedPath: string,
  rawInfo: PathInfo,
  normalizedInfo: PathInfo,
): OutputPathValidation {
  return {
    ok,
    error,
    normalizedPath,
    parentDir: normalizedInfo.parent_dir,
    parentExists: normalizedInfo.parent_exists,
    outputHasLitematicExt: normalizedInfo.has_litematic_ext,
    outputIsAbsolute: normalizedInfo.is_absolute,
    outputExists: normalizedInfo.exists,
    outputIsDir: normalizedInfo.is_dir,
    rawInfo,
    normalizedInfo,
  };
}

export function formatGeneratePathLog(values: Record<string, unknown>): string {
  return `[LBA_GENERATE_OUTPUT_PATH]\n${Object.entries(values)
    .map(([key, value]) => `${key}=${Array.isArray(value) ? JSON.stringify(value) : String(value)}`)
    .join("\n")}`;
}

export function normalizePlanBlockStates(plan: ProjectionPlan): PlanNormalizationResult {
  const warnings: string[] = [];
  const normalized: ProjectionPlan = JSON.parse(JSON.stringify(plan));
  for (const [regionIndex, region] of (normalized.regions || []).entries()) {
    for (const [operationIndex, operation] of (region.operations || []).entries()) {
      const path = `regions[${regionIndex}].operations[${operationIndex}]`;
      normalizeOperationBlocks(operation, path, warnings);
    }
  }
  return { plan: normalized, warnings };
}

function normalizeOperationBlocks(operation: any, path: string, warnings: string[]) {
  if (operation?.type === "checkerboard_floor") {
    normalizeMaterialBlocks(operation.material_a, `${path}.material_a`, warnings);
    normalizeMaterialBlocks(operation.material_b, `${path}.material_b`, warnings);
    return;
  }
  if (operation?.material) {
    normalizeMaterialBlocks(operation.material, `${path}.material`, warnings);
    return;
  }
  if (operation?.block) {
    operation.block = normalizeBlock(operation.block, `${path}.block`, warnings);
  }
}

function normalizeMaterialBlocks(material: any, path: string, warnings: string[]) {
  if (!material) return;
  if (material.name) {
    const normalized = normalizeBlock(material, path, warnings);
    Object.assign(material, normalized);
    return;
  }
  if (material.type === "single" && material.block) {
    material.block = normalizeBlock(material.block, `${path}.block`, warnings);
    return;
  }
  if (material.type === "weighted_random") {
    for (const [index, entry] of (material.entries || []).entries()) {
      if (entry.block) {
        entry.block = normalizeBlock(entry.block, `${path}.entries[${index}].block`, warnings);
      }
    }
  }
}

function normalizeBlock(block: BlockStateSpec, path?: string, warnings?: string[]): BlockStateSpec {
  const rawName = block.name.trim();
  const name = rawName.includes(":") ? rawName : `minecraft:${rawName}`;
  const properties: Record<string, string> = Object.fromEntries(Object.entries(block.properties || {}).filter(([, value]) => value !== ""));
  applyCriticalDefaultProperties(name, properties, path, warnings);
  return Object.keys(properties).length ? { name, properties } : { name, properties: {} };
}

function applyCriticalDefaultProperties(name: string, properties: Record<string, string>, path?: string, warnings?: string[]) {
  const defaults = criticalDefaultProperties(name);
  for (const [key, value] of Object.entries(defaults)) {
    if (properties[key] === undefined || properties[key] === "") {
      properties[key] = value;
      if (path && warnings) warnings.push(t("native.plan.propertiesCompleted", { path, key, value }));
    }
  }
}

function criticalDefaultProperties(blockId: string): Record<string, string> {
  const local = blockId.split(":").pop() || blockId;
  const dbDefaults = getDefaultProperties(blockId);
  const defaults: Record<string, string> = {};
  const add = (key: string, fallback: string) => {
    defaults[key] = dbDefaults[key] || fallback;
  };

  if (local === "piston" || local === "sticky_piston") {
    add("extended", "false");
    add("facing", "north");
  } else if (local === "observer") {
    add("powered", "false");
    add("facing", "north");
  } else if (local === "repeater") {
    add("delay", "1");
    add("facing", "north");
    add("locked", "false");
    add("powered", "false");
  } else if (local === "comparator") {
    add("facing", "north");
    add("mode", "compare");
    add("powered", "false");
  } else if (local === "dispenser" || local === "dropper") {
    add("facing", "north");
    add("triggered", "false");
  } else if (local === "hopper") {
    add("enabled", "true");
    add("facing", "down");
  } else if (local === "lever") {
    add("face", "wall");
    add("facing", "north");
    add("powered", "false");
  } else if (local.endsWith("_button") || local === "stone_button" || local === "polished_blackstone_button") {
    add("face", "wall");
    add("facing", "north");
    add("powered", "false");
  } else if (local.endsWith("_trapdoor") || local === "iron_trapdoor") {
    add("facing", "north");
    add("half", "bottom");
    add("open", "false");
    add("powered", "false");
    add("waterlogged", "false");
  } else if (local.endsWith("_door") || local === "iron_door") {
    add("facing", "north");
    add("half", "lower");
    add("hinge", "left");
    add("open", "false");
    add("powered", "false");
  } else if (local.endsWith("_slab")) {
    add("type", "bottom");
    add("waterlogged", "false");
  } else if (local.endsWith("_stairs")) {
    add("facing", "north");
    add("half", "bottom");
    add("shape", "straight");
    add("waterlogged", "false");
  } else if (
    local.endsWith("_fence")
    || local.endsWith("_wall")
    || local.endsWith("_pane")
    || local === "iron_bars"
    || local === "nether_brick_fence"
  ) {
    if (dbDefaults.waterlogged !== undefined || local.endsWith("_wall") || local.endsWith("_pane") || local === "iron_bars") {
      add("waterlogged", "false");
    }
  }
  return defaults;
}

function buildMaterialPatch(operation: OperationFormState): { block: BlockStateSpec } | { material: any } {
  if (operation.material?.type === "weighted_random") {
    return {
      material: {
        type: "weighted_random",
        seed: Number(operation.material.seed) || 0,
        entries: operation.material.entries.map((entry) => ({
          weight: Number(entry.weight) || 1,
          block: normalizeBlock(entry.block),
        })),
      },
    };
  }
  if (operation.material?.type === "single") {
    return {
      material: {
        type: "single",
        block: normalizeBlock(operation.material.block),
      },
    };
  }
  return { block: normalizeBlock(operation.block) };
}

function normalizeMaterial(material: MaterialSpec): any {
  if (material.type === "weighted_random") {
    return {
      type: "weighted_random",
      seed: Number(material.seed) || 0,
      entries: material.entries.map((entry) => ({
        weight: Number(entry.weight) || 1,
        block: normalizeBlock(entry.block),
      })),
    };
  }
  return {
    type: "single",
    block: normalizeBlock(material.block),
  };
}

function validateOperationMaterial(operation: any, path: string, errors: string[]) {
  if (operation.type === "checkerboard_floor") {
    validatePlanMaterial(operation.material_a, `${path}.material_a`, errors, true);
    validatePlanMaterial(operation.material_b, `${path}.material_b`, errors, true);
    return;
  }

  if (operation.material) {
    validatePlanMaterial(operation.material, `${path}.material`, errors, true);
    return;
  }

  validatePlanBlock(operation.block, `${path}.block`, errors, true);
}

function validatePlanMaterial(material: any, path: string, errors: string[], required: boolean) {
  if (!material) {
    if (required) errors.push(t("native.plan.required", { path: path }));
    return;
  }
  if (material.name) {
    validatePlanBlock(material, path, errors, true);
    return;
  }
  if (material.type === "single") {
    validatePlanBlock(material.block, `${path}.block`, errors, true);
    return;
  }
  if (material.type === "weighted_random") {
    if (!Array.isArray(material.entries) || material.entries.length === 0) {
      errors.push(t("native.plan.entriesEmpty", { path: path }));
      return;
    }
    material.entries.forEach((entry: any, index: number) => {
      if (!Number.isFinite(entry.weight) || Number(entry.weight) <= 0) errors.push(t("native.plan.weightPositive", { path: path, index: index }));
      validatePlanBlock(entry.block, `${path}.entries[${index}].block`, errors, true);
    });
    return;
  }
  errors.push(t("native.plan.typeUnsupported", { path: path }));
}

function validatePlanBlock(block: any, path: string, errors: string[], required: boolean) {
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
  } else if (CHINESE_RE.test(block.name)) {
    errors.push(t("native.plan.nameChinese", { path: path }));
  }
  const properties = block.properties || {};
  if (typeof properties !== "object" || Array.isArray(properties)) {
    errors.push(t("native.plan.propertiesObject", { path: path }));
    return;
  }
  Object.entries(properties).forEach(([key, value]) => {
    if (typeof key !== "string" || typeof value !== "string") errors.push(t("native.plan.propertiesStrings", { path: path }));
    if (CHINESE_RE.test(key) || CHINESE_RE.test(String(value))) errors.push(t("native.plan.propertiesChinese", { path: path }));
  });
}

function validateOperationBounds(operation: any, size: [number, number, number], path: string, errors: string[]) {
  const checkPoint = (point: number[] | undefined, name: string) => {
    if (!point || point.length !== 3) {
      errors.push(t("native.plan.vector", { path: path, name: name }));
      return;
    }
    point.forEach((value, axis) => {
      if (!Number.isFinite(value)) errors.push(t("native.plan.number", { path: path, name: name, axis: axis }));
      if (value < 0 || value >= size[axis]) errors.push(t("native.plan.outside", { path: path, name: name, axis: axis, maximum: size[axis] - 1 }));
    });
  };

  if (["fill_box", "hollow_box", "floor", "wall", "outline_box", "checkerboard_floor", "roof_gable"].includes(operation.type)) {
    checkPoint(operation.from, "from");
    checkPoint(operation.to, "to");
  }
  if (operation.type === "pillar") {
    checkPoint(operation.base, "base");
    if (operation.height <= 0) errors.push(t("native.plan.heightPositive", { path: path }));
    if (operation.base && operation.base[1] + operation.height - 1 >= size[1]) errors.push(t("native.plan.heightOutside", { path: path }));
  }
  if (operation.type === "cylinder") {
    checkPoint(operation.center, "center");
    if (operation.radius < 0) errors.push(t("native.plan.radiusNegative", { path: path }));
    if (operation.height <= 0) errors.push(t("native.plan.heightPositive", { path: path }));
    if (operation.center && operation.center[0] - operation.radius < 0) errors.push(t("native.plan.radiusminx", { path: path }));
    if (operation.center && operation.center[0] + operation.radius >= size[0]) errors.push(t("native.plan.radiusmaxx", { path: path }));
    if (operation.center && operation.center[2] - operation.radius < 0) errors.push(t("native.plan.radiusminz", { path: path }));
    if (operation.center && operation.center[2] + operation.radius >= size[2]) errors.push(t("native.plan.radiusmaxz", { path: path }));
    if (operation.center && operation.center[1] + operation.height - 1 >= size[1]) errors.push(t("native.plan.heightOutside", { path: path }));
  }
  if (operation.type === "sphere") {
    checkPoint(operation.center, "center");
    if (operation.radius < 0) errors.push(t("native.plan.radiusNegative", { path: path }));
    if (operation.center && operation.center[0] - operation.radius < 0) errors.push(t("native.plan.radiusminx", { path: path }));
    if (operation.center && operation.center[0] + operation.radius >= size[0]) errors.push(t("native.plan.radiusmaxx", { path: path }));
    if (operation.center && operation.center[1] - operation.radius < 0) errors.push(t("native.plan.radiusminy", { path: path }));
    if (operation.center && operation.center[1] + operation.radius >= size[1]) errors.push(t("native.plan.radiusmaxy", { path: path }));
    if (operation.center && operation.center[2] - operation.radius < 0) errors.push(t("native.plan.radiusminz", { path: path }));
    if (operation.center && operation.center[2] + operation.radius >= size[2]) errors.push(t("native.plan.radiusmaxz", { path: path }));
  }
  if (operation.type === "ring") {
    checkPoint(operation.center, "center");
    if (operation.radius < 0) errors.push(t("native.plan.radiusNegative", { path: path }));
    if (operation.height <= 0) errors.push(t("native.plan.heightPositive", { path: path }));
    if (operation.thickness <= 0) errors.push(t("native.plan.thicknessPositive", { path: path }));
    if (operation.center && operation.center[0] - operation.radius < 0) errors.push(t("native.plan.radiusminx", { path: path }));
    if (operation.center && operation.center[0] + operation.radius >= size[0]) errors.push(t("native.plan.radiusmaxx", { path: path }));
    if (operation.center && operation.center[2] - operation.radius < 0) errors.push(t("native.plan.radiusminz", { path: path }));
    if (operation.center && operation.center[2] + operation.radius >= size[2]) errors.push(t("native.plan.radiusmaxz", { path: path }));
    if (operation.center && operation.center[1] + operation.height - 1 >= size[1]) errors.push(t("native.plan.heightOutside", { path: path }));
  }
  if (["hollow_box", "wall"].includes(operation.type) && operation.thickness <= 0) {
    errors.push(t("native.plan.thicknessPositive", { path: path }));
  }
}
