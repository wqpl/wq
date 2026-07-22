import unittest

from check_package import PublishError, validate_package_metadata


class PackageMetadataTests(unittest.TestCase):
    PACKAGE = {
        "name": "wq-wasm",
        "version": "0.9.0-preview1",
        "repository": {"url": "git+https://github.com/wqpl/wq.git"},
    }
    GENERATED = {"name": "wq-wasm", "version": "0.9.0-preview1"}
    CARGO = {"packages": [{"name": "wq-wasm", "version": "0.9.0-preview1"}]}

    def test_validates_preview_metadata(self) -> None:
        npm_tag = validate_package_metadata(
            self.PACKAGE,
            self.GENERATED,
            self.CARGO,
            "v0.9.0-preview1",
        )
        self.assertEqual(npm_tag, "preview")

    def test_selects_latest_for_a_stable_version(self) -> None:
        package = {**self.PACKAGE, "version": "1.0.0"}
        generated = {**self.GENERATED, "version": "1.0.0"}
        cargo = {"packages": [{"name": "wq-wasm", "version": "1.0.0"}]}
        self.assertEqual(
            validate_package_metadata(package, generated, cargo, None),
            "latest",
        )

    def test_rejects_a_release_tag_mismatch(self) -> None:
        with self.assertRaisesRegex(PublishError, "release tag"):
            validate_package_metadata(
                self.PACKAGE,
                self.GENERATED,
                self.CARGO,
                "v0.9.0-preview2",
            )

    def test_rejects_missing_cargo_package(self) -> None:
        with self.assertRaisesRegex(PublishError, "does not contain"):
            validate_package_metadata(self.PACKAGE, self.GENERATED, {"packages": []})


if __name__ == "__main__":
    unittest.main()
