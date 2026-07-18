import assert from "node:assert/strict";
import test from "node:test";

import {
  orderPublishRemotes,
  parsePublishRemotes,
  pushCommand,
} from "./release-git.mjs";

test("reads publishing remotes from whitespace-separated Git config", () => {
  assert.deepEqual(parsePublishRemotes("github codeberg\nbackup\n"), [
    "github",
    "codeberg",
    "backup",
  ]);
  assert.throws(
    () => parsePublishRemotes("github codeberg github"),
    /configured more than once/,
  );
});

test("pushes mirrors before the GitHub publishing remote", () => {
  assert.deepEqual(
    orderPublishRemotes(["github", "codeberg", "backup"], "github"),
    ["codeberg", "backup", "github"],
  );
  assert.throws(
    () => orderPublishRemotes(["codeberg"], "github"),
    /is not a publishing remote/,
  );
});

test("builds an atomic branch and tag push", () => {
  assert.deepEqual(pushCommand("codeberg", "main", "v0.9.0-preview2"), [
    "git",
    "push",
    "--atomic",
    "codeberg",
    "HEAD:refs/heads/main",
    "refs/tags/v0.9.0-preview2:refs/tags/v0.9.0-preview2",
  ]);
});
