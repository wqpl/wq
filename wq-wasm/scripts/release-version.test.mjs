import assert from "node:assert/strict";
import test from "node:test";

import {
  nextVersion,
  requireVersionAdvance,
  updateCargoManifest,
  workspaceVersion,
} from "./release-version.mjs";

test("increments numbered previews and stable patches", () => {
  assert.equal(nextVersion("0.9.0-preview1"), "0.9.0-preview2");
  assert.equal(nextVersion("1.2.3"), "1.2.4");
});

test("requires a forward version change", () => {
  assert.doesNotThrow(() =>
    requireVersionAdvance("0.9.0-preview1", "0.9.0-preview2"),
  );
  assert.doesNotThrow(() =>
    requireVersionAdvance("0.9.0-preview1", "0.9.0"),
  );
  assert.throws(
    () => requireVersionAdvance("0.9.0", "0.9.0-preview1"),
    /older than stable/,
  );
  assert.throws(
    () => requireVersionAdvance("1.0.0", "0.9.0"),
    /older than/,
  );
  assert.throws(
    () => requireVersionAdvance("0.9.0-preview2", "0.9.0-preview1"),
    /not newer than/,
  );
});

test("updates workspace and path dependency versions only", () => {
  const source = `[workspace]
members = []

[workspace.package]
version = "0.9.0-preview1"

[workspace.dependencies]
wqpl = { version = "0.9.0-preview1", path = "wqpl" }
external = "0.9.0-preview1"
`;

  assert.equal(workspaceVersion(source), "0.9.0-preview1");
  const result = updateCargoManifest(
    source,
    "0.9.0-preview1",
    "0.9.0-preview2",
  );

  assert.equal(result.pathDependencyUpdates, 1);
  assert.match(result.contents, /version = "0\.9\.0-preview2"/);
  assert.match(
    result.contents,
    /wqpl = \{ version = "0\.9\.0-preview2", path = "wqpl" \}/,
  );
  assert.match(result.contents, /external = "0\.9\.0-preview1"/);
});
