import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed dev port and does not want the browser auto-opening.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    // Chokidar takes one inotify watch per file *and* per directory, and it
    // does not read .gitignore. Left alone it tries to watch ~109k entries
    // here, almost all of them Rust build output: target/ is ~54k and the
    // agent worktrees under .claude/ carry a target/ tree each (~47k more).
    // That exhausts fs.inotify.max_user_watches and vite dies on ENOSPC
    // before it ever serves a page. These are additive to vite's own
    // defaults (.git, node_modules, the outDir).
    watch: {
      ignored: ["**/target/**", "**/.claude/**"],
    },
  },
  build: {
    target: "es2021",
    outDir: "dist",
  },
});
