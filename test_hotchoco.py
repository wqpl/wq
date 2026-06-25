import tempfile
import unittest
from pathlib import Path
from unittest import mock

import hotchoco


class HotchocoHarnessTests(unittest.TestCase):
    def test_expected_exit_code_for_prefers_specific_override(self) -> None:
        testcase = {
            "expected_exit_code": 4,
            "expected_exit_codes": {
                "tiny": 1,
                "tiny/exec": 2,
            },
        }
        mode = {"name": "exec", "expected_exit_code": 3}

        self.assertEqual(hotchoco.expected_exit_code_for(testcase, mode, "tiny"), 2)
        self.assertEqual(hotchoco.expected_exit_code_for(testcase, mode, "other"), 3)
        self.assertEqual(
            hotchoco.expected_exit_code_for({"expected_exit_code": 4}, {"name": "x"}, "y"),
            4,
        )
        self.assertEqual(hotchoco.expected_exit_code_for({}, {"name": "x"}, "y"), 0)

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

    def test_run_one_test_reports_exit_code_mismatch(self) -> None:
        def fake_run(cmd, **kwargs):
            return subprocess_result(stdout="same output\n", returncode=1)

        test = {
            "group": "wq",
            "test": "tiny",
            "mode": "exec",
            "source": "tiny.wq",
            "flags": [],
            "capture": "stdout",
            "output_extension": "",
            "expected_exit_code": 0,
        }
        with tempfile.TemporaryDirectory() as tmp_dir:
            with mock.patch.object(hotchoco.subprocess, "run", side_effect=fake_run):
                result = hotchoco.run_one_test(test, {}, Path(tmp_dir))

            actual = Path(result["output_path"]).read_text()

        self.assertEqual(result["return_code"], 1)
        self.assertEqual(result["expected_exit_code"], 0)
        self.assertIn("same output", actual)
        self.assertNotIn("EXIT CODE MISMATCH", actual)
        self.assertEqual(hotchoco.exit_code_note(result), "exit code 1, expected 0")

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
