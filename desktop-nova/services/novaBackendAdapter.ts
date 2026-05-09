import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

export interface ActiveProjectionFile {
    name: string;
    path: string;
    size: string;
}

export interface ProjectionAnalysis {
    metadata: {
        name: string;
        author: string;
        description: string;
        time_created: number;
        time_modified: number;
        total_blocks: number;
        total_volume: number;
        region_count: number;
        enclosing_size: { x: number; y: number; z: number };
        litematic_version: number;
        litematic_subversion: number;
        minecraft_data_version: number;
    };
    regions: Array<{
        name: string;
        position: { x: number; y: number; z: number };
        size: { x: number; y: number; z: number };
    }>;
    analysis?: {
        total_non_air_blocks?: number;
        entity_counts?: Record<string, number>;
    };
    derived?: {
        building?: {
            density?: number;
            fluid_count?: number;
            fluid_ratio?: number;
            redstone_count?: number;
            redstone_ratio?: number;
            building_type?: string;
            unique_block_types?: number;
        };
        material_counts?: Array<{ key: string; count: number }>;
    };
}

export interface LibraryRecord {
    path: string;
    fileName?: string;
    displayName?: string;
    status?: string;
    lastError?: string;
}

export interface UserConfigInfo {
    config_dir: string;
    default_config_dir: string;
    config: {
        theme: string;
        render_display_mode: string;
        preview_mode: string;
    };
}

const baseName = (path: string) => path.split(/[\\/]/).pop() || path;

export async function getUserConfig(): Promise<UserConfigInfo> {
    return await invoke("get_user_config");
}

export async function chooseLitematicFile(): Promise<ActiveProjectionFile | null> {
    const selected = await open({
        multiple: false,
        filters: [{ name: "Litematica schematic", extensions: ["litematic"] }]
    });
    if (!selected || Array.isArray(selected)) {
        return null;
    }
    return {
        name: baseName(selected),
        path: selected,
        size: "-"
    };
}

export async function analyzeProjection(filePath: string): Promise<ProjectionAnalysis> {
    const stdout = await invoke<string>("execute_backend", {
        binaryName: "litematica_core.exe",
        args: ["analyze", filePath]
    });
    return JSON.parse(stdout) as ProjectionAnalysis;
}

export async function startPopupViewer(filePath: string, displayMode = "full"): Promise<void> {
    await invoke("start_native_viewer", { filePath, displayMode });
}

export async function generatePreview(filePath: string, displayMode = "normal") {
    return await invoke<{ preview_path: string; data_url: string; stdout: string; stderr: string; exit_code: number | null }>(
        "generate_preview_image",
        { filePath, displayMode }
    );
}

export async function openFileFolder(filePath: string): Promise<void> {
    await invoke("open_file_parent_dir", { filePath });
}

export async function executeBackend(binaryName: string, args: string[]): Promise<string> {
    return await invoke("execute_backend", { binaryName, args });
}

export async function writeUserConfigFile(relativePath: string, content: string): Promise<void> {
    await invoke("write_user_config_file", { relativePath, content });
}

export async function getUserConfigFilePath(relativePath: string): Promise<string> {
    return await invoke("get_user_config_file_path", { relativePath });
}

export async function runProjectionPlan(planJson: string, outputPath: string, dryRun: boolean): Promise<string> {
    const relativePath = `render/nova_plan_${Date.now()}.json`;
    await writeUserConfigFile(relativePath, planJson);
    const planPath = await getUserConfigFilePath(relativePath);
    const args = ["generate", "--plan", planPath, "--output", outputPath];
    if (dryRun) {
        args.push("--dry-run");
    }
    return await executeBackend("litematica_core.exe", args);
}

export async function runReplaceBlocks(
    inputPath: string,
    rulesJson: string,
    outputPath: string,
    dryRun: boolean
): Promise<string> {
    const relativePath = `render/nova_replace_rules_${Date.now()}.json`;
    await writeUserConfigFile(relativePath, rulesJson);
    const rulesPath = await getUserConfigFilePath(relativePath);
    const args = ["replace-blocks", inputPath, "--rules", rulesPath, "--output", outputPath];
    if (dryRun) {
        args.push("--dry-run");
    }
    return await executeBackend("litematica_core.exe", args);
}

export async function startCacheBuild(filePath: string, buildMode: string) {
    return await invoke("start_cache_build_task", { filePath, buildMode });
}

export async function loadProjectionLibrary(): Promise<LibraryRecord[]> {
    try {
        const raw = await invoke<string>("read_user_config_file", {
            relativePath: "projection-library/js_library.json"
        });
        const parsed = JSON.parse(raw) as { records?: LibraryRecord[] };
        return parsed.records || [];
    } catch {
        return [];
    }
}

export function metricsFromAnalysis(analysis: ProjectionAnalysis | null) {
    const building = analysis?.derived?.building;
    if (!analysis || !building) {
        return [
            { label: "红石占比", value: "-" },
            { label: "建筑类型", value: "-" },
            { label: "流体占比", value: "-" },
            { label: "密度", value: "-" },
            { label: "唯一方块数", value: "-" }
        ];
    }
    return [
        { label: "红石占比", value: `${((building.redstone_ratio || 0) * 100).toFixed(2)}%` },
        { label: "建筑类型", value: building.building_type || "-" },
        { label: "流体占比", value: `${((building.fluid_ratio || 0) * 100).toFixed(2)}%` },
        { label: "密度", value: `${((building.density || 0) * 100).toFixed(2)}%` },
        { label: "唯一方块数", value: String(building.unique_block_types ?? "-") }
    ];
}
