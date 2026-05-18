import { resolve } from "node:path";

import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
    plugins: [react()],
    clearScreen: false,
    server: {
        strictPort: true,
        watch: {
            ignored: ["**/tauri/**"]
        }
    },
    build: {
        rollupOptions: {
            input: {
                main: resolve(__dirname, "index.html"),
                materialList: resolve(__dirname, "material_list.html"),
                enumerator: resolve(__dirname, "enumerator.html"),
                redenLibrary: resolve(__dirname, "reden_library.html"),
                localLibraryFolders: resolve(__dirname, "local_library_folders.html"),
                assetManager: resolve(__dirname, "asset_manager.html"),
                demoWindow: resolve(__dirname, "demo_window.html")
            }
        }
    }
});
