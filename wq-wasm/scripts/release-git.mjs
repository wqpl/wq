export function parsePublishRemotes(configuredRemotes) {
  const remotes = configuredRemotes.split(/\s+/).filter(Boolean);
  const duplicate = remotes.find(
    (remote, index) => remotes.indexOf(remote) !== index,
  );
  if (duplicate) {
    throw new Error(
      `publishing remote '${duplicate}' is configured more than once`,
    );
  }
  return remotes;
}

export function orderPublishRemotes(remotes, githubRemote) {
  if (!remotes.includes(githubRemote)) {
    throw new Error(`GitHub remote '${githubRemote}' is not a publishing remote`);
  }
  return [...remotes.filter((remote) => remote !== githubRemote), githubRemote];
}

export function pushCommand(remote, branch, tag) {
  return [
    "git",
    "push",
    "--atomic",
    remote,
    `HEAD:refs/heads/${branch}`,
    `refs/tags/${tag}:refs/tags/${tag}`,
  ];
}
