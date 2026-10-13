#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const defaultRootDir = path.resolve(scriptDir, "../..");

const SOURCE_EXTENSIONS = new Set([
  ".c",
  ".cc",
  ".cpp",
  ".cxx",
  ".h",
  ".hh",
  ".hpp",
  ".js",
  ".jsx",
  ".mjs",
  ".rs",
  ".ts",
  ".tsx",
]);

const FRONTEND_EXTENSIONS = new Set([".js", ".jsx", ".mjs", ".ts", ".tsx"]);
const SKIPPED_DIRECTORIES = new Set([
  ".git",
  ".next",
  ".sandbox-git",
  ".vite",
  ".worktrees",
  "build",
  "cache",
  "coverage",
  "dist",
  "node_modules",
  "out",
  "target",
  "third_party",
  "vendor",
]);

const FRONTEND_RUNTIME_RULES = [
  {
    pattern:
      /\b(?:RenderServiceSnapshot|BuildSnapshot|ArtifactSnapshot|ViewerSnapshot|RenderPreviewOutput|RenderPreviewRequest|SceneContent|SectionCommit|GpuSceneArtifact|SceneBuildIdentity|RenderBuildIdentity|PresentationIdentity|SessionIdentity)\b/g,
    description: "raw render snapshot or identity type",
  },
  {
    pattern:
      /\b(?:sceneBuildIdentity|renderBuildIdentity|presentationIdentity|sessionIdentity|artifactPath|artifact_path|artifactDigest|artifact_digest|artifactBytes|artifact_bytes|artifactId|artifact_id|buildId|build_id|viewerId|viewer_id|workerPid|worker_pid|viewerPid|viewer_pid|pid|PID)\b/g,
    description: "renderer-owned identity, artifact location, or process identifier",
  },
  {
    pattern:
      /\b(?:sectionsUploaded|sections_uploaded|sectionsAvailable|sections_available|sectionsResident|sections_resident|sectionsGpuAccepted|sections_gpu_accepted|sectionsGpuCompleted|sections_gpu_completed|residentSlots|resident_slots|ResidentSlot|CommitSeq|commitSeq|commit_seq|DirectoryIndex|directoryIndex|directory_index|uploadAck|upload_ack|gpuFence|gpu_fence|descriptorState|descriptor_state)\b/g,
    description: "renderer-internal progress or runtime counter",
  },
];

const APPLICATION_MODEL_RULES = [
  {
    pattern: /\bnova_render_protocol\b/g,
    description: "render protocol dependency",
  },
  {
    pattern: /\btauri(?:::|_)/g,
    description: "Tauri dependency",
  },
  {
    pattern: /\b(?:windows(?:::|_)|Win32\b)/g,
    description: "Windows platform dependency",
  },
  {
    pattern: /\b(?:renderer(?:_|::)|render_v\d+|ash::|vulkan::)/gi,
    description: "renderer implementation dependency",
  },
  {
    pattern:
      /\b(?:ResidentSlot|CommitSeq|DirectoryIndex|GpuFence|UploadAck|DescriptorState|ArtifactOffset|ViewerGpuRuntimeHandle)\b/g,
    description: "renderer runtime type",
  },
  {
    pattern: /\bstd::process::(?:Child|Command)\b/g,
    description: "OS process ownership dependency",
  },
];

const RENDER_SERVICE_PROCESS_RULES = [
  {
    pattern: /\b(?:std::process::)?Child\b/g,
    description: "render service must not own child process handles",
  },
  {
    pattern: /\bCommand\s*::\s*new\s*\(/g,
    description: "render service must request process launch from the Application Host",
  },
  {
    pattern: /\.\s*kill\s*\(/g,
    description: "render service must request termination from the Application Host",
  },
];

const FRONTEND_HELPER_RULES = [
  {
    pattern: /\b(?:execute_backend(?:_trace)?|executeBackend(?:Trace)?|BackendTrace)\b/g,
    description: "generic helper execution contract",
  },
  {
    pattern: /\blitematica_core\.exe\b/gi,
    description: "core helper executable identity",
  },
  {
    pattern: /\b(?:RunCore|run_core|ExtractProjectionPreview|extract_projection_preview|application_render_release|releaseApplicationRender|ReleaseViewer)\b/g,
    description: "helper or render-runtime lifecycle command",
  },
];

const HOST_SUPERVISION_DATA_RULES = [
  {
    pattern: /\b(?:SceneContent|ArtifactSnapshot|GpuSceneArtifact|SectionCommit)\b/g,
    description: "scene or artifact data-plane type",
  },
  {
    pattern: /(?:\bserde_json::Value\b|\bVec\s*<\s*u8\s*>)/g,
    description: "opaque or byte-scale payload",
  },
];

function parseArguments(argv) {
  let rootDir = defaultRootDir;

  for (const argument of argv) {
    if (argument.startsWith("--root-dir=")) {
      rootDir = path.resolve(argument.slice("--root-dir=".length));
    } else {
      throw new Error(`Unknown application boundary check argument: ${argument}`);
    }
  }

  return { rootDir };
}

function toPosix(filePath) {
  return filePath.split(path.sep).join("/");
}

function isInside(filePath, parentPath) {
  const relative = path.relative(parentPath, filePath);
  return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
}

function walkSourceFiles(directory) {
  if (!fs.existsSync(directory)) {
    return [];
  }

  const files = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    if (entry.isSymbolicLink()) {
      continue;
    }

    const fullPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      if (!SKIPPED_DIRECTORIES.has(entry.name.toLowerCase())) {
        files.push(...walkSourceFiles(fullPath));
      }
    } else if (SOURCE_EXTENSIONS.has(path.extname(entry.name).toLowerCase())) {
      files.push(fullPath);
    }
  }
  return files;
}

function lineNumberAt(text, index) {
  let line = 1;
  for (let cursor = 0; cursor < index; cursor += 1) {
    if (text.charCodeAt(cursor) === 10) {
      line += 1;
    }
  }
  return line;
}

function blankRange(characters, source, start, end) {
  for (let index = start; index < end; index += 1) {
    if (source[index] !== "\r" && source[index] !== "\n") {
      characters[index] = " ";
    }
  }
}

function quotedEnd(source, start, quote) {
  let index = start + 1;
  while (index < source.length) {
    if (source[index] === "\\") {
      index += 2;
    } else if (source[index] === quote) {
      return index + 1;
    } else {
      index += 1;
    }
  }
  return source.length;
}

function rustRawStringEnd(source, start) {
  let marker = start;
  if (source[marker] === "b") {
    marker += 1;
  }
  if (source[marker] !== "r") {
    return null;
  }

  marker += 1;
  let hashCount = 0;
  while (source[marker] === "#") {
    hashCount += 1;
    marker += 1;
  }
  if (source[marker] !== '"') {
    return null;
  }

  const terminator = `"${"#".repeat(hashCount)}`;
  const closing = source.indexOf(terminator, marker + 1);
  return closing < 0 ? source.length : closing + terminator.length;
}

function maskNonCode(source, extension) {
  const characters = source.split("");
  const isJavaScript = FRONTEND_EXTENSIONS.has(extension);
  const isRust = extension === ".rs";
  let index = 0;

  while (index < source.length) {
    if (source[index] === "/" && source[index + 1] === "/") {
      const end = source.indexOf("\n", index + 2);
      const stop = end < 0 ? source.length : end;
      blankRange(characters, source, index, stop);
      index = stop;
      continue;
    }

    if (source[index] === "/" && source[index + 1] === "*") {
      const end = source.indexOf("*/", index + 2);
      const stop = end < 0 ? source.length : end + 2;
      blankRange(characters, source, index, stop);
      index = stop;
      continue;
    }

    if (isRust && (source[index] === "r" || source[index] === "b")) {
      const previous = source[index - 1] ?? "";
      if (!/[A-Za-z0-9_]/.test(previous)) {
        const end = rustRawStringEnd(source, index);
        if (end !== null) {
          blankRange(characters, source, index, end);
          index = end;
          continue;
        }
      }
    }

    if (source[index] === '"' || (isJavaScript && (source[index] === "'" || source[index] === "`"))) {
      const end = quotedEnd(source, index, source[index]);
      blankRange(characters, source, index, end);
      index = end;
      continue;
    }

    if (!isJavaScript && source[index] === "'") {
      const end = quotedEnd(source, index, "'");
      if (end - index <= 8) {
        blankRange(characters, source, index, end);
        index = end;
        continue;
      }
    }

    index += 1;
  }

  return characters.join("");
}

function maskComments(source, extension) {
  const characters = source.split("");
  const isJavaScript = FRONTEND_EXTENSIONS.has(extension);
  let index = 0;

  while (index < source.length) {
    if (source[index] === '"' || (isJavaScript && (source[index] === "'" || source[index] === "`"))) {
      index = quotedEnd(source, index, source[index]);
      continue;
    }

    if (source[index] === "/" && source[index + 1] === "/") {
      const end = source.indexOf("\n", index + 2);
      const stop = end < 0 ? source.length : end;
      blankRange(characters, source, index, stop);
      index = stop;
      continue;
    }

    if (source[index] === "/" && source[index + 1] === "*") {
      const end = source.indexOf("*/", index + 2);
      const stop = end < 0 ? source.length : end + 2;
      blankRange(characters, source, index, stop);
      index = stop;
      continue;
    }

    index += 1;
  }

  return characters.join("");
}

function collectMatches(text, pattern) {
  pattern.lastIndex = 0;
  return [...text.matchAll(pattern)];
}

const { rootDir } = parseArguments(process.argv.slice(2));
const desktopDir = path.join(rootDir, "desktop-nova");
const frontendRoots = [path.join(desktopDir, "src"), path.join(desktopDir, "ui")];
const applicationModelPath = path.join(desktopDir, "src-tauri", "src", "application", "model.rs");
const tauriEntrypointPath = path.join(desktopDir, "src-tauri", "src", "main.rs");
const hostSupervisionPath = path.join(
  rootDir,
  "tools",
  "viewer-core",
  "crates",
  "nova-render-protocol",
  "src",
  "host_supervision.rs",
);
const allSourceFiles = walkSourceFiles(rootDir);
const violations = [];
const violationKeys = new Set();

function addViolation(filePath, index, rule, message) {
  const relativeFile = toPosix(path.relative(rootDir, filePath));
  const line = lineNumberAt(fs.readFileSync(filePath, "utf8"), index);
  const key = JSON.stringify([relativeFile, line, rule, message]);
  if (violationKeys.has(key)) {
    return;
  }
  violationKeys.add(key);
  violations.push({ file: relativeFile, line, rule, message });
}

for (const filePath of allSourceFiles) {
  const extension = path.extname(filePath).toLowerCase();
  const source = fs.readFileSync(filePath, "utf8");
  const code = maskNonCode(source, extension);

  for (const match of collectMatches(code, /\bSetParent\s*\(/g)) {
    addViolation(
      filePath,
      match.index,
      "no-cross-process-setparent",
      "Product code must keep the native Viewer as a Viewer-owned top-level HWND.",
    );
  }

  if (frontendRoots.some((frontendRoot) => isInside(filePath, frontendRoot)) && FRONTEND_EXTENSIONS.has(extension)) {
    const sourceWithoutComments = maskComments(source, extension);
    for (const match of collectMatches(
      sourceWithoutComments,
      /["'][^"'\r\n]*(?:platform\/renderService|nova[-_]render[-_]protocol|render_service_[a-z0-9_]+)[^"'\r\n]*["']/gi,
    )) {
      addViolation(
        filePath,
        match.index,
        "frontend-no-render-wire-import",
        "Frontend code must depend on the Application semantic API, not the render wire contract.",
      );
    }

    for (const rule of FRONTEND_RUNTIME_RULES) {
      for (const match of collectMatches(code, rule.pattern)) {
        addViolation(
          filePath,
          match.index,
          "frontend-no-render-runtime-contract",
          `Frontend code contains ${rule.description}: ${match[0]}.`,
        );
      }
    }

    for (const rule of FRONTEND_HELPER_RULES) {
      for (const match of collectMatches(sourceWithoutComments, rule.pattern)) {
        addViolation(
          filePath,
          match.index,
          "frontend-no-helper-or-runtime-ownership",
          `Frontend code contains ${rule.description}: ${match[0]}.`,
        );
      }
    }
  }

  if (path.resolve(filePath) === path.resolve(applicationModelPath)) {
    for (const rule of APPLICATION_MODEL_RULES) {
      for (const match of collectMatches(code, rule.pattern)) {
        addViolation(
          filePath,
          match.index,
          "application-model-is-semantic",
          `Application semantic model contains a forbidden ${rule.description}: ${match[0]}.`,
        );
      }
    }
  }

  if (path.resolve(filePath) === path.resolve(tauriEntrypointPath)) {
    for (const match of collectMatches(code, /\b(?:nova_render_protocol|RenderPreviewOutput|RenderPreviewRequest|UiCommand)\b/g)) {
      addViolation(
        filePath,
        match.index,
        "tauri-entrypoint-uses-application-api",
        `Tauri product entrypoint contains render wire type: ${match[0]}.`,
      );
    }
  }


  if (path.resolve(filePath) === path.resolve(hostSupervisionPath)) {
    const productionCode = code.split(/#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/, 1)[0];
    for (const rule of HOST_SUPERVISION_DATA_RULES) {
      for (const match of collectMatches(productionCode, rule.pattern)) {
        addViolation(
          filePath,
          match.index,
          "host-supervision-control-plane-only",
          `Host supervision protocol contains forbidden ${rule.description}: ${match[0]}.`,
        );
      }
    }
  }

  if (path.basename(filePath).toLowerCase() === "render_service.rs") {
    for (const rule of RENDER_SERVICE_PROCESS_RULES) {
      for (const match of collectMatches(code, rule.pattern)) {
        addViolation(filePath, match.index, "render-service-no-os-process-ownership", rule.description);
      }
    }
  }
}

violations.sort(
  (left, right) =>
    left.file.localeCompare(right.file) ||
    left.line - right.line ||
    left.rule.localeCompare(right.rule) ||
    left.message.localeCompare(right.message),
);

if (violations.length === 0) {
  console.log("Application boundary check passed with zero violations.");
  process.exit(0);
}

console.error(`Application boundary check failed with ${violations.length} violation(s):`);
for (const violation of violations) {
  console.error(`${violation.file}:${violation.line} [${violation.rule}] ${violation.message}`);
}
process.exit(1);
