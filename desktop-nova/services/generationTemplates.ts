import { readWorkspaceFile, writeWorkspaceFile } from "./backend";
import {
  BlockStateSpec,
  GenerateFormState,
  MaterialEntrySpec,
  MaterialSpec,
  OperationFormState,
  OperationType,
  ProjectionPlan,
  createDefaultOperation,
} from "./generateService";

export interface GenerationTemplate {
  id: string;
  name_zh: string;
  description_zh: string;
  tags: string[];
  preview_hint?: string;
  plan: ProjectionPlan;
}

const TEMPLATE_INDEX_PATH = "data/generation-templates/index.json";

function makeId(): string {
  return `${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export async function loadGenerationTemplates(): Promise<GenerationTemplate[]> {
  const rawIndex = await readWorkspaceFile(TEMPLATE_INDEX_PATH);
  const paths = JSON.parse(rawIndex) as string[];
  const templates = await Promise.all(
    paths.map(async (path) => JSON.parse(await readWorkspaceFile(`data/generation-templates/${path}`)) as GenerationTemplate),
  );
  return templates.sort((a, b) => a.name_zh.localeCompare(b.name_zh, "zh-CN"));
}

export function templateToForm(template: GenerationTemplate): GenerateFormState {
  const region = template.plan.regions[0];
  return {
    metadata: {
      name: template.plan.metadata?.name || template.name_zh,
      author: template.plan.metadata?.author || "Litematica-BA",
      description: template.plan.metadata?.description || template.description_zh,
      minecraftDataVersion: template.plan.minecraft_data_version || 3953,
    },
    region: {
      name: region.name || "main",
      origin: region.origin || [0, 0, 0],
      size: region.size || [32, 16, 32],
    },
    operations: (region.operations || []).map(operationToForm),
  };
}

export async function exportTemplate(path: string, form: GenerateFormState, plan: ProjectionPlan) {
  const template: GenerationTemplate = {
    id: `custom-${Date.now()}`,
    name_zh: form.metadata.name || "自定义模板",
    description_zh: form.metadata.description || "从投影生成页导出的自定义模板。",
    tags: ["custom"],
    plan,
  };
  await writeWorkspaceFile(path, JSON.stringify(template, null, 2));
}

function operationToForm(operation: any): OperationFormState {
  const type = (operation.type || "fill_box") as OperationType;
  const base = createDefaultOperation(type);
  const material = materialFromPlan(operation.material);
  const legacyBlock = blockFromPlan(operation.block) || materialBlock(material) || base.block;
  return {
    ...base,
    id: makeId(),
    type,
    from: operation.from || base.from,
    to: operation.to || base.to,
    base: operation.base || base.base,
    center: operation.center || base.center,
    height: operation.height ?? base.height,
    radius: operation.radius ?? base.radius,
    thickness: operation.thickness ?? base.thickness,
    filled: operation.filled ?? base.filled,
    block: legacyBlock,
    material,
    materialA: materialFromPlan(operation.material_a) || base.materialA,
    materialB: materialFromPlan(operation.material_b) || base.materialB,
    axis: operation.axis || base.axis,
    overhang: operation.overhang ?? base.overhang,
  };
}

function blockFromPlan(block: any): BlockStateSpec | undefined {
  if (!block?.name) return undefined;
  return { name: block.name, properties: { ...(block.properties || {}) } };
}

function materialFromPlan(material: any): MaterialSpec | undefined {
  if (!material) return undefined;
  if (material.name) {
    return { type: "single", block: blockFromPlan(material)! };
  }
  if (material.type === "single") {
    const block = blockFromPlan(material.block);
    if (!block) return undefined;
    return { type: "single", block };
  }
  if (material.type === "weighted_random") {
    return {
      type: "weighted_random",
      seed: material.seed ?? 0,
      entries: (material.entries || []).map((entry: any): MaterialEntrySpec => ({
        id: makeId(),
        weight: entry.weight ?? 1,
        block: blockFromPlan(entry.block) || { name: "minecraft:stone", properties: {} },
      })),
    };
  }
  return undefined;
}

function materialBlock(material: MaterialSpec | undefined): BlockStateSpec | undefined {
  if (!material) return undefined;
  if (material.type === "single") return material.block;
  return material.entries[0]?.block;
}
