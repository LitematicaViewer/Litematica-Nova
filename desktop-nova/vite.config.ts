import { resolve } from "node:path";

import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

export default defineConfig({
    plugins: [react(), tailwindcss()],
    resolve: { alias: { "@": resolve(__dirname, "ui") } },
    clearScreen: false,
    server: {
        strictPort: true,
        watch: {
            // tauri dev runs `cargo build` in parallel with this dev server. The
            // linker writes src-tauri/target/debug/deps/<app>.exe while the build
            // is in flight, and chokidar then fails with
            //   EBUSY: resource busy or locked, watch '...\deps\desktop_nova.exe'
            // which kills the dev server and therefore the whole tauri dev run.
            // The previous pattern `**/tauri/**` never matched `src-tauri`, so the
            // whole Cargo target tree was being watched.
            ignored: ["**/src-tauri/**"]
        }
    },
    build: {
        rollupOptions: {
            input: {
                main: resolve(__dirname, "index.html"),
                materialList: resolve(__dirname, "material_list.html"),
                enumerator: resolve(__dirname, "enumerator.html"),
                assetManager: resolve(__dirname, "asset_manager.html"),
                redenLibrary: resolve(__dirname, "reden_library.html"),
                localLibraryFolders: resolve(__dirname, "local_library_folders.html"),
                demoWindow: resolve(__dirname, "demo_window.html")
            }
        }
    }
});
