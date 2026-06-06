import { existsSync, realpathSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, searchForWorkspaceRoot } from "vite";
import { viteStaticCopy } from "vite-plugin-static-copy";

const rootDir = dirname(fileURLToPath(import.meta.url));
const wqPackageDir = resolve(rootDir, "node_modules/wq-wasm");
const fsAllowList = [searchForWorkspaceRoot(rootDir)];

// `npm link wqpl` resolves to a real path outside this repo, so Vite must be
// told that serving the package wasm from that location is intentional.
if (existsSync(wqPackageDir)) {
  fsAllowList.push(realpathSync(wqPackageDir));
}

const htmlEntries = [
  "index.html",
  "article.html",
  "playground.html",
  "repl.html",
  "more.html",
  "subfolder.html",
];

export default defineConfig({
  base: "./",
  server: {
    fs: {
      allow: fsAllowList,
    },
  },
  plugins: [
    viteStaticCopy({
      targets: [
        { src: "articles/**/*", dest: "." },
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
