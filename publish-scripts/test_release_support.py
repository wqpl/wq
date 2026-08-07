import unittest

from release_support import (
    PublishError,
    changed_paths,
    next_version,
    order_publish_remotes,
    parse_publish_remotes,
    push_command,
    require_version_advance,
    update_cargo_manifest,
    update_json_version,
    workspace_version,
)


class VersionTests(unittest.TestCase):
    def test_increments_numbered_prereleases_and_stable_patches(self) -> None:
        self.assertEqual(next_version("0.9.0-preview1"), "0.9.0-preview2")
        self.assertEqual(next_version("1.2.3"), "1.2.4")
        self.assertEqual(next_version("1.2.3-beta"), "1.2.3-beta.1")

    def test_requires_semver_precedence_to_advance(self) -> None:
        require_version_advance("0.9.0-preview1", "0.9.0-preview2")
        require_version_advance("0.9.0-preview1", "0.9.0")
        require_version_advance("0.9.0-1", "0.9.0-preview")
        require_version_advance("0.9.0-preview", "0.9.0-preview.1")

        for current, target in (
            ("0.9.0", "0.9.0-preview1"),
            ("1.0.0", "0.9.0"),
            ("0.9.0-preview2", "0.9.0-preview1"),
            ("0.9.0-preview.1", "0.9.0-preview"),
            ("0.9.0+build.1", "0.9.0+build.2"),
        ):
            with (
                self.subTest(current=current, target=target),
                self.assertRaisesRegex(PublishError, "not newer"),
            ):
                require_version_advance(current, target)

    def test_rejects_invalid_semver(self) -> None:
        for version in ("1", "1.2", "01.2.3", "1.2.3-01", "1.2.3-"):
            with (
                self.subTest(version=version),
                self.assertRaisesRegex(PublishError, "invalid version"),
            ):
                next_version(version)


class CargoManifestTests(unittest.TestCase):
    SOURCE = """[workspace]
members = []

[workspace.package]
version = "0.9.0-preview1"

[workspace.dependencies]
wqpl = { version = "0.9.0-preview1", path = "wqpl" }
external = "0.9.0-preview1"
"""

    def test_reads_and_updates_workspace_versions(self) -> None:
        self.assertEqual(workspace_version(self.SOURCE), "0.9.0-preview1")

        result = update_cargo_manifest(
            self.SOURCE,
            "0.9.0-preview1",
            "0.9.0-preview2",
        )

        self.assertEqual(result.path_dependency_updates, 1)
        self.assertIn('version = "0.9.0-preview2"', result.contents)
        self.assertIn(
            'wqpl = { version = "0.9.0-preview2", path = "wqpl" }',
            result.contents,
        )
        self.assertIn('external = "0.9.0-preview1"', result.contents)

    def test_preserves_line_endings(self) -> None:
        source = self.SOURCE.replace("\n", "\r\n")
        result = update_cargo_manifest(
            source,
            "0.9.0-preview1",
            "0.9.0-preview2",
        )
        self.assertNotIn("\n", result.contents.replace("\r\n", ""))

    def test_rejects_an_inconsistent_path_dependency(self) -> None:
        source = self.SOURCE.replace(
            'wqpl = { version = "0.9.0-preview1"',
            'wqpl = { version = "0.8.0"',
        )
        with self.assertRaisesRegex(PublishError, "expected 0.9.0-preview1"):
            update_cargo_manifest(source, "0.9.0-preview1", "0.9.0-preview2")


class JsonManifestTests(unittest.TestCase):
    def test_updates_top_level_and_nested_versions(self) -> None:
        package: dict[str, object] = {"version": "0.9.0-preview1"}
        grammar: dict[str, object] = {
            "metadata": {"version": "0.9.0-preview1"},
        }

        update_json_version(
            package,
            ("version",),
            "0.9.0-preview1",
            "0.9.0-preview2",
            label="wq-ts package",
        )
        update_json_version(
            grammar,
            ("metadata", "version"),
            "0.9.0-preview1",
            "0.9.0-preview2",
            label="wq-ts grammar",
        )

        self.assertEqual(package["version"], "0.9.0-preview2")
        self.assertEqual(
            grammar,
            {"metadata": {"version": "0.9.0-preview2"}},
        )

    def test_rejects_an_inconsistent_json_version(self) -> None:
        package: dict[str, object] = {"version": "0.8.0"}

        with self.assertRaisesRegex(
            PublishError,
            "expected wq-ts package version 0.9.0-preview1, got 0.8.0",
        ):
            update_json_version(
                package,
                ("version",),
                "0.9.0-preview1",
                "0.9.0-preview2",
                label="wq-ts package",
            )


class GitTests(unittest.TestCase):
    def test_parses_unique_publish_remotes(self) -> None:
        self.assertEqual(
            parse_publish_remotes("github codeberg\nbackup\n"),
            ["github", "codeberg", "backup"],
        )
        with self.assertRaisesRegex(PublishError, "configured more than once"):
            parse_publish_remotes("github codeberg github")

    def test_orders_github_after_mirrors(self) -> None:
        self.assertEqual(
            order_publish_remotes(["github", "codeberg", "backup"], "github"),
            ["codeberg", "backup", "github"],
        )
        with self.assertRaisesRegex(PublishError, "not a publishing remote"):
            order_publish_remotes(["codeberg"], "github")

    def test_builds_an_atomic_branch_and_tag_push(self) -> None:
        self.assertEqual(
            push_command("codeberg", "main", "v0.9.0-preview2"),
            (
                "git",
                "push",
                "--atomic",
                "codeberg",
                "HEAD:refs/heads/main",
                "refs/tags/v0.9.0-preview2:refs/tags/v0.9.0-preview2",
            ),
        )

    def test_parses_nul_delimited_porcelain_status(self) -> None:
        status = b" M Cargo.toml\0?? path with spaces\0R  renamed.txt\0old-name.txt\0"
        self.assertEqual(
            changed_paths(status),
            ["Cargo.toml", "path with spaces", "renamed.txt"],
        )

    def test_rejects_malformed_porcelain_status(self) -> None:
        with self.assertRaisesRegex(PublishError, "malformed Git status"):
            changed_paths(b"M Cargo.toml\0")


if __name__ == "__main__":
    unittest.main()
