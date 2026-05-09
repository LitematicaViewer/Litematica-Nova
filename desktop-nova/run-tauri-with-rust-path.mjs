import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import path, { delimiter } from "node:path";
import { fileURLToPath } from "node:url";

const currentFile = fileURLToPath(import.meta.url);
const projectRoot = path.dirname(currentFile);
const tauriWorkspaceRoot = path.resolve(projectRoot, "..");
const userHome = process.env.USERPROFILE || process.env.HOME || "";
const cargoHome = process.env.CARGO_HOME || path.join(userHome, ".cargo");
const rustupHome = process.env.RUSTUP_HOME || path.join(userHome, ".rustup");
const cargoBin = path.join(cargoHome, "bin");
const tauriBin = process.platform === "win32"
    ? path.join(projectRoot, "node_modules", ".bin", "tauri.cmd")
    : path.join(projectRoot, "node_modules", ".bin", "tauri");

if (!existsSync(tauriBin)) {
    console.error(`Tauri CLI not found at ${tauriBin}`);
    process.exit(1);
}

const env = { ...process.env };
const currentPath = env.Path || env.PATH || "";
const pathEntries = currentPath.split(delimiter).filter(Boolean);
const normalizedCargoBin = path.resolve(cargoBin).toLowerCase();
const hasCargoBin = pathEntries.some((entry) => path.resolve(entry).toLowerCase() === normalizedCargoBin);

if (existsSync(cargoBin) && !hasCargoBin) {
    const nextPath = [cargoBin, ...pathEntries].join(delimiter);
    env.Path = nextPath;
    env.PATH = nextPath;
}

if (existsSync(cargoHome)) {
    env.CARGO_HOME = cargoHome;
}

if (existsSync(rustupHome)) {
    env.RUSTUP_HOME = rustupHome;
}

const child = spawn(tauriBin, process.argv.slice(2), {
    cwd: tauriWorkspaceRoot,
    env,
    stdio: "inherit"
});

child.on("error", (error) => {
    console.error(error);
    process.exit(1);
});

child.on("exit", (code, signal) => {
    if (signal) {
        process.kill(process.pid, signal);
        return;
    }
    process.exit(code ?? 1);
});