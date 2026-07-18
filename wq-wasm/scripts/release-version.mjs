const VERSION_PATTERN =
  /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/;

export function parseVersion(version) {
  const match = VERSION_PATTERN.exec(version);
  if (!match) {
    throw new Error(`invalid version '${version}'`);
  }

  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
    prerelease: match[4] ?? null,
  };
}

export function nextVersion(version) {
  const parsed = parseVersion(version);
  if (!parsed.prerelease) {
    return `${parsed.major}.${parsed.minor}.${parsed.patch + 1}`;
  }

  const numberedPrerelease = /^(.*?)(\d+)$/.exec(parsed.prerelease);
  const prerelease = numberedPrerelease
    ? `${numberedPrerelease[1]}${Number(numberedPrerelease[2]) + 1}`
    : `${parsed.prerelease}.1`;
  return `${parsed.major}.${parsed.minor}.${parsed.patch}-${prerelease}`;
}

export function requireVersionAdvance(currentVersion, targetVersion) {
  const current = parseVersion(currentVersion);
  const target = parseVersion(targetVersion);
  if (currentVersion === targetVersion) {
    throw new Error(`version is already ${currentVersion}`);
  }

  const currentCore = [current.major, current.minor, current.patch];
  const targetCore = [target.major, target.minor, target.patch];
  for (let index = 0; index < currentCore.length; index += 1) {
    if (targetCore[index] > currentCore[index]) return;
    if (targetCore[index] < currentCore[index]) {
      throw new Error(
        `target version ${targetVersion} is older than ${currentVersion}`,
      );
    }
  }

  if (!current.prerelease) {
    throw new Error(
      `target version ${targetVersion} is older than stable ${currentVersion}`,
    );
  }
  if (!target.prerelease) return;

  const currentNumbered = /^(.*?)(\d+)$/.exec(current.prerelease);
  const targetNumbered = /^(.*?)(\d+)$/.exec(target.prerelease);
  if (
    currentNumbered &&
    targetNumbered &&
    currentNumbered[1] === targetNumbered[1]
  ) {
    if (Number(targetNumbered[2]) > Number(currentNumbered[2])) return;
    throw new Error(
      `target version ${targetVersion} is not newer than ${currentVersion}`,
    );
  }

  const currentParts = current.prerelease.split(".");
  const targetParts = target.prerelease.split(".");
  const length = Math.max(currentParts.length, targetParts.length);
  for (let index = 0; index < length; index += 1) {
    const currentPart = currentParts[index];
    const targetPart = targetParts[index];
    if (currentPart === undefined) return;
    if (targetPart === undefined) break;
    if (currentPart === targetPart) continue;

    const currentNumber = /^\d+$/.test(currentPart);
    const targetNumber = /^\d+$/.test(targetPart);
    if (currentNumber && targetNumber) {
      if (Number(targetPart) > Number(currentPart)) return;
      break;
    }
    if (currentNumber !== targetNumber) {
      if (!targetNumber) return;
      break;
    }
    if (targetPart > currentPart) return;
    break;
  }
  throw new Error(
    `target version ${targetVersion} is not newer than ${currentVersion}`,
  );
}

export function workspaceVersion(cargoManifest) {
  let section = "";
  for (const line of cargoManifest.split("\n")) {
    const heading = /^\s*\[([^\]]+)\]\s*$/.exec(line);
    if (heading) {
      section = heading[1];
      continue;
    }
    if (section !== "workspace.package") continue;

    const version = /^\s*version\s*=\s*"([^"]+)"\s*$/.exec(line);
    if (version) return version[1];
  }
  throw new Error("Cargo.toml does not define workspace.package.version");
}

export function updateCargoManifest(
  cargoManifest,
  currentVersion,
  targetVersion,
) {
  let section = "";
  let workspaceUpdates = 0;
  let pathDependencyUpdates = 0;
  const contents = cargoManifest
    .split("\n")
    .map((line) => {
      const heading = /^\s*\[([^\]]+)\]\s*$/.exec(line);
      if (heading) {
        section = heading[1];
        return line;
      }

      const versionField = /(\bversion\s*=\s*")([^"]+)(")/;
      const match = versionField.exec(line);
      if (!match) return line;

      const isWorkspaceVersion = section === "workspace.package";
      const isPathDependency =
        section === "workspace.dependencies" && /\bpath\s*=/.test(line);
      if (!isWorkspaceVersion && !isPathDependency) return line;
      if (match[2] !== currentVersion) {
        throw new Error(
          `expected ${currentVersion} in Cargo.toml, got ${match[2]}`,
        );
      }

      if (isWorkspaceVersion) workspaceUpdates += 1;
      if (isPathDependency) pathDependencyUpdates += 1;
      return line.replace(
        versionField,
        `${match[1]}${targetVersion}${match[3]}`,
      );
    })
    .join("\n");

  if (workspaceUpdates !== 1) {
    throw new Error(
      `expected one workspace version, updated ${workspaceUpdates}`,
    );
  }

  return { contents, pathDependencyUpdates };
}
