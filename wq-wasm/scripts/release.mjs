import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { createInterface } from "node:readline/promises";
import { fileURLToPath } from "node:url";

import {
  nextVersion,
  parseVersion,
  requireVersionAdvance,
  updateCargoManifest,
  workspaceVersion,
} from "./release-version.mjs";
import {
  orderPublishRemotes,
  parsePublishRemotes,
  pushCommand,
} from "./release-git.mjs";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const crateDirectory = resolve(scriptDirectory, "..");
const workspaceDirectory = resolve(crateDirectory, "..");
const cargoManifestPath = resolve(workspaceDirectory, "Cargo.toml");
const cargoLockPath = resolve(workspaceDirectory, "Cargo.lock");
const packageManifestPath = resolve(crateDirectory, "package.json");
const releaseFiles = ["Cargo.toml", "Cargo.lock", "wq-wasm/package.json"];

function displayCommand(command, args) {
  return [command, ...args]
    .map((part) => (/\s/.test(part) ? JSON.stringify(part) : part))
    .join(" ");
}

function run(command, args, options = {}) {
  console.log(`\n$ ${displayCommand(command, args)}`);
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? workspaceDirectory,
    env: { ...process.env, ...options.env },
    stdio: options.stdio ?? "inherit",
    encoding: "utf8",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} exited with status ${result.status}`);
  }
  return result.stdout?.trim() ?? "";
}

function capture(command, args, options = {}) {
  return run(command, args, { ...options, stdio: ["inherit", "pipe", "inherit"] });
}

function hasRemote(name) {
  const result = spawnSync("git", ["remote", "get-url", name], {
    cwd: workspaceDirectory,
    stdio: "ignore",
  });
  return result.status === 0;
}

function configuredPublishRemotes() {
  const result = spawnSync(
    "git",
    ["config", "--get-all", "remotes.publish"],
    { cwd: workspaceDirectory, encoding: "utf8" },
  );
  if (result.error) throw result.error;
  if (result.status === 1) return [];
  if (result.status !== 0) {
    throw new Error(`git config exited with status ${result.status}`);
  }
  return parsePublishRemotes(result.stdout);
}

function defaultPublishRemotes() {
  const configured = configuredPublishRemotes();
  if (configured.length > 0) return configured;
  return [hasRemote("github") ? "github" : "origin"];
}

function parseArguments(args) {
  const remotes = [];
  let version = null;
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === "--help" || argument === "-h") {
      return { help: true, remotes: [], version: null };
    }
    if (argument === "--remote") {
      const remote = args[index + 1];
      if (!remote) throw new Error("--remote requires a name");
      remotes.push(remote);
      index += 1;
      continue;
    }
    if (argument.startsWith("--")) {
      throw new Error(`unknown option '${argument}'`);
    }
    if (version) throw new Error("provide at most one target version");
    version = argument.replace(/^v/, "");
  }
  return { help: false, remotes, version };
}

function printHelp() {
  console.log(`Usage: npm run release -- [VERSION] [--remote NAME]...

Without VERSION, increment a trailing prerelease number or the stable patch.
The command updates versions, runs release checks, commits, and tags locally.
It asks before atomic pushes to the remotes in 'remotes.publish'. Non-GitHub
mirrors are pushed first, and GitHub is pushed last to trigger publishing.
'--remote' can be repeated to override the configured publishing remotes.

Examples:
  npm run release
  npm run release -- 0.10.0-preview1
  npm run release -- 0.10.0 --remote codeberg --remote github`);
}

function assertCleanWorktree() {
  const status = capture("git", ["status", "--porcelain"]);
  if (status) {
    throw new Error(`worktree must be clean:\n${status}`);
  }
}

function assertTagAbsent(tag) {
  const result = spawnSync(
    "git",
    ["show-ref", "--verify", "--quiet", `refs/tags/${tag}`],
    { cwd: workspaceDirectory, stdio: "ignore" },
  );
  if (result.status === 0) throw new Error(`tag '${tag}' already exists`);
  if (result.status !== 1) {
    throw new Error(`could not check whether tag '${tag}' exists`);
  }
}

function verifyWorkspaceVersions(metadata, targetVersion) {
  const workspaceMembers = new Set(metadata.workspace_members);
  const mismatches = metadata.packages.filter(
    (item) => workspaceMembers.has(item.id) && item.version !== targetVersion,
  );
  if (mismatches.length > 0) {
    const details = mismatches
      .map((item) => `${item.name}@${item.version}`)
      .join(", ");
    throw new Error(`workspace versions did not update: ${details}`);
  }
}

function verifyChangedFiles() {
  const changed = capture("git", ["status", "--porcelain"])
    .split("\n")
    .filter(Boolean)
    .map((line) => line.slice(3));
  const expected = new Set(releaseFiles);
  const unexpected = changed.filter((path) => !expected.has(path));
  const missing = releaseFiles.filter((path) => !changed.includes(path));
  if (unexpected.length > 0 || missing.length > 0) {
    throw new Error(
      `unexpected release changes; extra: ${unexpected.join(", ") || "none"}; missing: ${missing.join(", ") || "none"}`,
    );
  }
}

async function askToPush(remotes, branch, tag) {
  const commands = remotes.map((remote) => pushCommand(remote, branch, tag));
  const instructions = commands
    .map((command) => `  ${displayCommand(command[0], command.slice(1))}`)
    .join("\n");
  const remoteList = remotes.map((remote) => `'${remote}'`).join(", ");

  if (!process.stdin.isTTY || !process.stdout.isTTY) {
    console.log(`\nNot pushing from a non-interactive terminal.`);
    console.log(`Publish later with:\n${instructions}`);
    return;
  }

  const prompt = createInterface({ input: process.stdin, output: process.stdout });
  const answer = await prompt.question(
    `\nPush ${tag} and ${branch} to ${remoteList} now? ` +
      `The GitHub tag push triggers package publishing. [y/N] `,
  );
  prompt.close();

  if (!/^(?:y|yes)$/i.test(answer.trim())) {
    console.log(`Not pushed. Publish later with:\n${instructions}`);
    return;
  }
  for (const command of commands) {
    run(command[0], command.slice(1));
  }
}

async function main() {
  const options = parseArguments(process.argv.slice(2));
  if (options.help) {
    printHelp();
    return;
  }

  assertCleanWorktree();
  const branch = capture("git", ["branch", "--show-current"]);
  if (!branch) throw new Error("release requires a checked-out branch");

  const remotes = parsePublishRemotes(
    (options.remotes.length > 0 ? options.remotes : defaultPublishRemotes()).join(
      "\n",
    ),
  );
  const remoteUrls = new Map();
  for (const remote of remotes) {
    if (!hasRemote(remote)) {
      throw new Error(`git remote '${remote}' does not exist`);
    }
    remoteUrls.set(
      remote,
      capture("git", ["remote", "get-url", "--push", remote]),
    );
  }
  const githubRemotes = remotes.filter((remote) =>
    /github\.com[:/]wqpl\/wq(?:\.git)?$/.test(remoteUrls.get(remote)),
  );
  if (githubRemotes.length !== 1) {
    throw new Error(
      `expected one publishing remote for the GitHub wq repository, got ${githubRemotes.length}`,
    );
  }
  const orderedRemotes = orderPublishRemotes(remotes, githubRemotes[0]);

  const originalCargoManifest = readFileSync(cargoManifestPath, "utf8");
  const originalCargoLock = readFileSync(cargoLockPath, "utf8");
  const originalPackageManifest = readFileSync(packageManifestPath, "utf8");
  const packageManifest = JSON.parse(originalPackageManifest);
  const currentVersion = workspaceVersion(originalCargoManifest);
  if (packageManifest.version !== currentVersion) {
    throw new Error(
      `Cargo version ${currentVersion} does not match npm version ${packageManifest.version}`,
    );
  }

  const targetVersion = options.version ?? nextVersion(currentVersion);
  parseVersion(targetVersion);
  requireVersionAdvance(currentVersion, targetVersion);
  const tag = `v${targetVersion}`;
  assertTagAbsent(tag);

  console.log(`Preparing ${tag} from ${branch}. Publishing remotes in push order:`);
  for (const remote of orderedRemotes) {
    console.log(`  ${remote}: ${remoteUrls.get(remote)}`);
  }
  const updatedCargoManifest = updateCargoManifest(
    originalCargoManifest,
    currentVersion,
    targetVersion,
  );
  if (updatedCargoManifest.pathDependencyUpdates === 0) {
    throw new Error("no versioned workspace path dependencies were updated");
  }
  packageManifest.version = targetVersion;

  writeFileSync(cargoManifestPath, updatedCargoManifest.contents);
  writeFileSync(packageManifestPath, `${JSON.stringify(packageManifest, null, 2)}\n`);

  let restoreOnFailure = true;
  try {
    const metadata = JSON.parse(
      capture("cargo", ["metadata", "--format-version", "1", "--offline"]),
    );
    verifyWorkspaceVersions(metadata, targetVersion);

    run("cargo", ["+nightly", "fmt", "--check"]);
    run("cargo", ["clippy", "--all-targets", "--", "-D", "warnings"]);
    run("cargo", ["test", "-p", "wq-wasm"]);
    run("cargo", [
      "publish",
      "--workspace",
      "--locked",
      "--dry-run",
      "--allow-dirty",
    ]);
    run("npm", ["run", "build"], { cwd: crateDirectory });
    run("npm", ["run", "check"], {
      cwd: crateDirectory,
      env: { RELEASE_TAG: tag },
    });
    run("npm", ["test"], { cwd: crateDirectory });
    run("npm", ["pack", "--dry-run"], { cwd: crateDirectory });
    verifyChangedFiles();

    restoreOnFailure = false;
    run("git", ["add", ...releaseFiles]);
    run("git", [
      "commit",
      "-m",
      `release ${tag}`,
      "-m",
      "Bump Cargo and npm package versions for the release.\n\nRelease Notes:\n\n- N/A",
    ]);
    run("git", ["tag", "-a", tag, "-m", tag]);
  } catch (error) {
    if (restoreOnFailure) {
      writeFileSync(cargoManifestPath, originalCargoManifest);
      writeFileSync(cargoLockPath, originalCargoLock);
      writeFileSync(packageManifestPath, originalPackageManifest);
      console.error("Restored version files after the failed release check.");
    }
    throw error;
  }

  console.log(`\nCreated release commit and local tag ${tag}.`);
  await askToPush(orderedRemotes, branch, tag);
}

main().catch((error) => {
  console.error(`\nRelease failed: ${error.message}`);
  process.exitCode = 1;
});
