import tempfile
import unittest
from pathlib import Path
from unittest import mock

import hotchoco


class HotchocoHarnessTests(unittest.TestCase):
    def test_run_one_test_timeout(self) -> None:
        calls = []

        def fake_run(cmd, **kwargs):
            calls.append((cmd, kwargs))
            return subprocess_result(stdout="ok\n")

        test = {
            "group": "wq",
            "test": "tiny",
            "mode": "exec",
            "source": "tiny.wq",
            "flags": [],
            "capture": "stdout",
            "output_extension": "",
        }
        with tempfile.TemporaryDirectory() as tmp_dir:
            with mock.patch.object(hotchoco.subprocess, "run", side_effect=fake_run):
                hotchoco.run_one_test(test, {}, Path(tmp_dir))

        self.assertEqual(calls[0][1]["timeout"], 30)

    def test_build_wq_cli_uses_debug_build_command(self) -> None:
        with mock.patch.object(hotchoco.subprocess, "run") as run:
            with mock.patch.object(hotchoco.sys, "stderr"):
                run.return_value = subprocess_result(returncode=0)
                hotchoco.build_wq_cli()

        run.assert_called_once_with(
            ["cargo", "build", "-p", "wq-cli"],
            cwd=hotchoco.PROJECT_ROOT,
        )

    def test_compute_diff_labels_changes_without_bare_plus_minus(self) -> None:
        diff = hotchoco.compute_diff("-golden\n=\n", "+actual\n=\n", "wq/tiny/exec")

        self.assertIn("golden: wq/tiny/exec", diff)
        self.assertIn("actual: wq/tiny/exec", diff)
        self.assertIn("-", diff)
        self.assertIn("+", diff)
        self.assertNotIn("\n--golden", diff)
        self.assertNotIn("\n++actual", diff)


def subprocess_result(stdout="", stderr="", returncode=0):
    return mock.Mock(stdout=stdout, stderr=stderr, returncode=returncode)


if __name__ == "__main__":
    unittest.main()
