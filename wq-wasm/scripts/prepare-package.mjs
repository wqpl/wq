import { copyFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const crateDirectory = resolve(scriptDirectory, "..");

copyFileSync(
  resolve(crateDirectory, ".npmignore"),
  resolve(crateDirectory, "pkg/.npmignore"),
);
