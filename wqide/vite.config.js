import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, searchForWorkspaceRoot } from "vite";
import { viteStaticCopy } from "vite-plugin-static-copy";

const rootDir = dirname(fileURLToPath(import.meta.url));
const wqWasmPkgDir = resolve(rootDir, "../wq-wasm/pkg");
const wqWasmEntry = resolve(wqWasmPkgDir, "wq_wasm.js");
const docsArticlesDir = resolve(rootDir, "../d/articles");
const fsAllowList = [
  searchForWorkspaceRoot(rootDir),
  wqWasmPkgDir,
  docsArticlesDir,
];

if (!existsSync(wqWasmEntry)) {
  throw new Error(
    "Missing wq-wasm generated package. Run `npm run build:wasm` from wqide/.",
  );
}

const htmlEntries = [
  "index.html",
  "article.html",
  "playground.html",
  "viz.html",
  "repl.html",
  "more.html",
  "subfolder.html",
];

export default defineConfig({
  base: "./",
  resolve: {
    alias: {
      "wq-wasm": wqWasmEntry,
    },
  },
  server: {
    fs: {
      allow: fsAllowList,
    },
  },
  optimizeDeps: {
    exclude: ["wq-wasm"],
  },
  plugins: [
    viteStaticCopy({
      targets: [
        { src: "../d/articles/**/*", dest: ".", rename: { stripBase: 1 } },
        { src: "manifest.json", dest: "." },
        { src: "favicon.png", dest: "." },
        { src: "wq_transparent_bg.png", dest: "." },
        { src: "CNAME", dest: "." },
      ],
    }),
  ],
  build: {
    rollupOptions: {
      input: Object.fromEntries(
        htmlEntries.map((file) => [
          file.replace(/\.html$/, ""),
          resolve(rootDir, file),
        ]),
      ),
    },
  },
});
