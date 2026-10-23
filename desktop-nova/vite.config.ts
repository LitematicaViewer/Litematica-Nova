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
            // The old entry named a "tauri" directory that does not exist; the
            // real one is "src-tauri", so Vite watched src-tauri/target/** and
            // died with EBUSY whenever cargo relinked desktop_nova.exe. That
            // relink happens on every build, because build.rs rewrites
            // gen/block-icons and then watches that directory.
            ignored: ["**/src-tauri/**"]
        }
    },
    build: {
        rollupOptions: {
            input: {
                main: resolve(__dirname, "index.html"),
                materialList: resolve(__dirname, "material_list.html"),
                redenLibrary: resolve(__dirname, "reden_library.html"),
                localLibraryFolders: resolve(__dirname, "local_library_folders.html"),
                assetManager: resolve(__dirname, "asset_manager.html"),
                irisOptions: resolve(__dirname, "iris_options.html"),
                enumerator: resolve(__dirname, "enumerator.html"),
                demoWindow: resolve(__dirname, "demo_window.html")
            }
        }
    }
});
