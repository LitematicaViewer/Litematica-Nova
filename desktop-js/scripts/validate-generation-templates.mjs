import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const templateRoot = path.join(repoRoot, "data", "generation-templates");
const indexPath = path.join(templateRoot, "index.json");
const coreExe = path.join(repoRoot, "bin", "viewer-backend", "litematica_core.exe");
const workDir = path.join(repoRoot, ".tmp", "template-validation");

function run(args) {
  const result = spawnSync(coreExe, args, {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  return {
    status: result.status ?? 1,
    stdout: result.stdout || "",
    stderr: result.stderr || "",
  };
}

function fail(id, phase, result) {
  console.error(`[template:${id}] ${phase} failed`);
  console.error("stdout:");
  console.error(result.stdout);
  console.error("stderr:");
  console.error(result.stderr);
  process.exitCode = 1;
}

if (!existsSync(coreExe)) {
  console.error(`missing backend exe: ${coreExe}`);
  process.exit(1);
}

await rm(workDir, { recursive: true, force: true });
await mkdir(workDir, { recursive: true });

const templateFiles = JSON.parse((await readFile(indexPath, "utf8")).replace(/^\uFEFF/, ""));
const templates = [];

for (const file of templateFiles) {
  const fullPath = path.join(templateRoot, file);
  const template = JSON.parse((await readFile(fullPath, "utf8")).replace(/^\uFEFF/, ""));
  templates.push({ file, template });
  const planPath = path.join(workDir, `${template.id}.plan.json`);
  await writeFile(planPath, JSON.stringify(template.plan, null, 2), "utf8");
  const dry = run(["generate", "--plan", planPath, "--dry-run", "--json"]);
  if (dry.status !== 0) {
    fail(template.id, "dry-run", dry);
    continue;
  }
  try {
    const summary = JSON.parse(dry.stdout);
    console.log(`[template:${template.id}] dry-run ok non_air=${summary.estimated_non_air_blocks} ops=${summary.operation_count ?? "n/a"}`);
  } catch (error) {
    fail(template.id, `dry-run json parse: ${error}`, dry);
  }
}

const samples = templates.filter(({ template }) => ["simple-cottage", "random-stone-brick-wall"].includes(template.id));
for (const { template } of samples) {
  const planPath = path.join(workDir, `${template.id}.plan.json`);
  const outputPath = path.join(workDir, `${template.id}.litematic`);
  await rm(outputPath, { force: true });
  const apply = run(["generate", "--plan", planPath, "--output", outputPath]);
  if (apply.status !== 0) {
    fail(template.id, "apply", apply);
    continue;
  }
  const analyze = run(["analyze", outputPath]);
  if (analyze.status !== 0) {
    fail(template.id, "analyze", analyze);
    continue;
  }
  console.log(`[template:${template.id}] apply+analyze ok`);
}

if (process.exitCode) process.exit(process.exitCode);
console.log(`validated ${templates.length} generation templates`);
