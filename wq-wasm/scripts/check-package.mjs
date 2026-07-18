import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const crateDirectory = resolve(scriptDirectory, "..");
const workspaceDirectory = resolve(crateDirectory, "..");

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function requireEqual(label, actual, expected) {
  if (actual !== expected) {
    throw new Error(`${label}: expected ${expected}, got ${actual}`);
  }
}

const packageManifest = readJson(resolve(crateDirectory, "package.json"));
const generatedManifest = readJson(resolve(crateDirectory, "pkg/package.json"));
const cargoMetadata = JSON.parse(
  execFileSync(
    "cargo",
    ["metadata", "--format-version", "1", "--no-deps", "--locked"],
    { cwd: workspaceDirectory, encoding: "utf8" },
  ),
);
const cargoPackage = cargoMetadata.packages.find(
  (item) => item.name === "wq-wasm",
);

if (!cargoPackage) {
  throw new Error("Cargo metadata does not contain the wq-wasm package");
}

requireEqual("npm package name", packageManifest.name, "wq-wasm");
requireEqual("npm and Cargo versions", packageManifest.version, cargoPackage.version);
requireEqual("generated package name", generatedManifest.name, packageManifest.name);
requireEqual(
  "generated package version",
  generatedManifest.version,
  packageManifest.version,
);
requireEqual(
  "npm repository",
  packageManifest.repository?.url,
  "git+https://github.com/wqpl/wq.git",
);

const releaseTag = process.env.RELEASE_TAG;
if (releaseTag) {
  requireEqual("release tag", releaseTag, `v${packageManifest.version}`);
}

const npmTag = packageManifest.version.includes("-") ? "preview" : "latest";
console.log(`${packageManifest.name}@${packageManifest.version} is ready for npm tag '${npmTag}'`);

if (process.env.GITHUB_OUTPUT) {
  appendFileSync(process.env.GITHUB_OUTPUT, `npm_tag=${npmTag}\n`);
}

