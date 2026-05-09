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
                materialList: resolve(__dirname, "material_list.html")
            }
        }
    }
});
